use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use reqwest::{Client, RequestBuilder, Response, StatusCode, Url};
use serde::de::DeserializeOwned;

use crate::auth::{AuthState, TokenSink};
use crate::clock::Clock;
use crate::error::{Error, Result};
use crate::models::Status;
use crate::version::{Feature, ServerVersion};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const PING_TIMEOUT: Duration = Duration::from_secs(10);
const USER_AGENT: &str = concat!("abs-desktop/", env!("CARGO_PKG_VERSION"));

/// HTTP client for one account on one Audiobookshelf server.
///
/// Holds the token state. Share it behind an `Arc`; all methods take `&self`.
pub struct AbsClient {
    http: Client,
    base: Url,
    status: Status,
    version: ServerVersion,
    auth: Mutex<AuthState>,
    refresh_lock: tokio::sync::Mutex<()>,
    sink: Box<dyn TokenSink>,
    clock: Clock,
}

/// A `reqwest` client with the settings this crate expects. Clone it for every account.
pub fn http_client() -> Result<Client> {
    Ok(Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(CONNECT_TIMEOUT)
        .build()?)
}

impl AbsClient {
    /// Probes `GET /status` at `server_url` and returns a logged-out client.
    ///
    /// `server_url` may include a subpath when the server runs with `ROUTER_BASE_PATH`.
    pub async fn connect(http: Client, server_url: &str) -> Result<Self> {
        let base = base_url(server_url)?;
        let response = http.get(join(&base, "status")?).send().await?;
        let clock = Clock::default();
        clock.note(&response);
        let status: Status = decode(check(response).await?).await?;
        let version = status
            .server_version
            .parse::<ServerVersion>()
            .map_err(|v| Error::InvalidUrl(format!("{server_url} reports version {v:?}")))?;
        if version < ServerVersion::MINIMUM {
            return Err(Error::Unsupported(Feature::RefreshTokens));
        }
        Ok(Self {
            http,
            base,
            status,
            version,
            auth: Mutex::default(),
            refresh_lock: tokio::sync::Mutex::new(()),
            sink: Box::new(()),
            clock,
        })
    }

    /// `GET /ping` without auth: fails with [`Error::Network`] when the server is gone.
    pub async fn ping(&self) -> Result<()> {
        let request = self.http.get(self.url("ping")?).timeout(PING_TIMEOUT);
        check(request.send().await?).await.map(drop)
    }

    /// Server time minus local time in milliseconds, from the latest answer.
    pub fn clock_offset_ms(&self) -> Option<i64> {
        self.clock.offset()
    }

    /// Sets where new refresh tokens are stored.
    pub fn with_token_sink(mut self, sink: impl TokenSink + 'static) -> Self {
        self.sink = Box::new(sink);
        self
    }

    /// The server URL, always ending in `/`.
    pub fn base_url(&self) -> &Url {
        &self.base
    }

    /// The `/status` response from [`AbsClient::connect`].
    pub fn status(&self) -> &Status {
        &self.status
    }

    pub fn server_version(&self) -> ServerVersion {
        self.version
    }

    /// True when the server is new enough for `feature`.
    pub fn supports(&self, feature: Feature) -> bool {
        self.version.supports(feature)
    }

    /// An absolute URL for a path relative to the server root.
    pub fn url(&self, path: &str) -> Result<Url> {
        join(&self.base, path)
    }

    /// An absolute URL for a server-provided path such as `contentUrl`, which may
    /// or may not already include the router base path.
    pub fn resolve(&self, server_path: &str) -> Result<Url> {
        resolve(&self.base, server_path)
    }

    /// Sends an authorized request. On 401 it refreshes once and retries once.
    pub(crate) async fn send(&self, request: RequestBuilder) -> Result<Response> {
        self.refresh_if_due().await?;
        let retry = request.try_clone();
        let (response, generation) = self.send_authorized(request).await?;
        if response.status() != StatusCode::UNAUTHORIZED {
            return check(response).await;
        }
        let retry = retry.ok_or(Error::LoginRequired)?;
        self.refresh(generation).await?;
        let (response, _) = self.send_authorized(retry).await?;
        if response.status() == StatusCode::UNAUTHORIZED {
            return Err(Error::LoginRequired);
        }
        check(response).await
    }

    async fn send_authorized(&self, request: RequestBuilder) -> Result<(Response, u64)> {
        let (token, generation) = self.auth_state().bearer().ok_or(Error::LoginRequired)?;
        let response = request.bearer_auth(token).send().await?;
        self.clock.note(&response);
        Ok((response, generation))
    }

    /// Sends an authorized request and decodes the JSON body.
    pub(crate) async fn send_json<T: DeserializeOwned>(
        &self,
        request: RequestBuilder,
    ) -> Result<T> {
        decode(self.send(request).await?).await
    }

    pub(crate) fn http(&self) -> &Client {
        &self.http
    }

    pub(crate) fn auth_state(&self) -> MutexGuard<'_, AuthState> {
        self.auth
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(crate) fn refresh_lock(&self) -> &tokio::sync::Mutex<()> {
        &self.refresh_lock
    }

    pub(crate) fn sink(&self) -> &dyn TokenSink {
        self.sink.as_ref()
    }
}

fn base_url(server_url: &str) -> Result<Url> {
    let trimmed = server_url.trim();
    let mut url = Url::parse(trimmed).map_err(|e| Error::InvalidUrl(format!("{trimmed}: {e}")))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(Error::InvalidUrl(format!("{trimmed}: not http or https")));
    }
    if !url.path().ends_with('/') {
        url.set_path(&format!("{}/", url.path()));
    }
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

fn join(base: &Url, path: &str) -> Result<Url> {
    base.join(path.trim_start_matches('/'))
        .map_err(|e| Error::InvalidUrl(format!("{path}: {e}")))
}

fn resolve(base: &Url, server_path: &str) -> Result<Url> {
    let prefix = base.path().trim_end_matches('/');
    match server_path.strip_prefix(prefix) {
        Some(rest) if !prefix.is_empty() && rest.starts_with('/') => join(base, rest),
        _ => join(base, server_path),
    }
}

/// Turns a non-success response into [`Error::Server`].
pub(crate) async fn check(response: Response) -> Result<Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.unwrap_or_default();
    Err(Error::Server { status, body })
}

/// Reads the body as JSON into `T`.
pub(crate) async fn decode<T: DeserializeOwned>(response: Response) -> Result<T> {
    let what = response.url().path().to_owned();
    let bytes = response.bytes().await?;
    serde_json::from_slice(&bytes).map_err(|source| Error::Decode { what, source })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_url_keeps_subpath() {
        let base = base_url("https://host.example/audiobookshelf").unwrap();
        assert_eq!(base.as_str(), "https://host.example/audiobookshelf/");
        assert_eq!(
            join(&base, "/api/me").unwrap().as_str(),
            "https://host.example/audiobookshelf/api/me"
        );
        assert!(base_url("ftp://host").is_err());
        assert!(base_url("not a url").is_err());
    }

    #[test]
    fn resolves_paths_with_and_without_base_path() {
        let base = base_url("https://host.example/abs").unwrap();
        let expected = "https://host.example/abs/api/items/1/file/2";
        assert_eq!(
            resolve(&base, "/api/items/1/file/2").unwrap().as_str(),
            expected
        );
        assert_eq!(
            resolve(&base, "/abs/api/items/1/file/2").unwrap().as_str(),
            expected
        );
        let root = base_url("http://localhost").unwrap();
        assert_eq!(
            resolve(&root, "/api/x").unwrap().as_str(),
            "http://localhost/api/x"
        );
    }

    #[test]
    fn root_base_url() {
        let base = base_url("http://localhost:13378/?x=1").unwrap();
        assert_eq!(base.as_str(), "http://localhost:13378/");
        assert_eq!(
            join(&base, "login").unwrap().as_str(),
            "http://localhost:13378/login"
        );
    }
}
