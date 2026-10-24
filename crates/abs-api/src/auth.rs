//! Login, token refresh and logout.

use std::time::SystemTime;

use reqwest::StatusCode;
use serde_json::{Value, json};

use crate::client::{AbsClient, check, decode};
use crate::error::{Error, Result, SinkError};
use crate::jwt::Lifetime;
use crate::models::LoginResponse;
use crate::version::Feature;

/// Receives every new refresh token so it survives a restart.
///
/// Called before the client starts using the token. ABS 2.37 rotates the
/// refresh token on every use, so a token that was not stored is lost.
pub trait TokenSink: Send + Sync {
    fn store_refresh_token(&self, token: &str) -> Result<(), SinkError>;
}

impl TokenSink for () {
    fn store_refresh_token(&self, _token: &str) -> Result<(), SinkError> {
        Ok(())
    }
}

impl<F> TokenSink for F
where
    F: Fn(&str) -> Result<(), SinkError> + Send + Sync,
{
    fn store_refresh_token(&self, token: &str) -> Result<(), SinkError> {
        self(token)
    }
}

/// What the client authenticates with.
#[derive(Clone)]
pub(crate) enum Credential {
    Session {
        access: String,
        refresh: String,
        lifetime: Option<Lifetime>,
    },
    ApiKey(String),
}

impl Credential {
    fn bearer(&self) -> &str {
        match self {
            Self::Session { access, .. } => access,
            Self::ApiKey(key) => key,
        }
    }
}

/// Current credential plus a counter that changes with every new credential.
#[derive(Default)]
pub(crate) struct AuthState {
    credential: Option<Credential>,
    generation: u64,
}

impl AuthState {
    pub(crate) fn bearer(&self) -> Option<(String, u64)> {
        let credential = self.credential.as_ref()?;
        Some((credential.bearer().to_owned(), self.generation))
    }

    fn set(&mut self, credential: Option<Credential>) {
        self.credential = credential;
        self.generation += 1;
    }

    fn lifetime(&self) -> Option<Lifetime> {
        match &self.credential {
            Some(Credential::Session { lifetime, .. }) => *lifetime,
            _ => None,
        }
    }
}

impl AbsClient {
    /// Logs in with username and password and keeps the returned tokens.
    pub async fn login(&self, username: &str, password: &str) -> Result<LoginResponse> {
        let response = self
            .http()
            .post(self.url("login")?)
            .header("x-return-tokens", "true")
            .json(&json!({ "username": username, "password": password }))
            .send()
            .await?;
        if response.status() == StatusCode::UNAUTHORIZED {
            return Err(Error::InvalidCredentials);
        }
        let body: Value = decode(check(response).await?).await?;
        self.install_session(&body)?;
        parse_login(body)
    }

    /// Authenticates with an API key. API keys don't expire on their own and are never refreshed.
    pub async fn login_api_key(&self, key: &str) -> Result<LoginResponse> {
        self.auth_state()
            .set(Some(Credential::ApiKey(key.to_owned())));
        match self.authorize().await {
            Err(Error::LoginRequired) => {
                self.auth_state().set(None);
                Err(Error::InvalidCredentials)
            }
            result => result,
        }
    }

    /// Starts from a stored refresh token, as after an app restart.
    pub async fn restore_session(&self, refresh_token: &str) -> Result<LoginResponse> {
        let body = self.refresh_request(refresh_token).await?;
        self.install_session(&body)?;
        parse_login(body)
    }

    /// `POST /api/authorize`: the current user, checked with the current token.
    pub async fn authorize(&self) -> Result<LoginResponse> {
        let request = self.http().post(self.url("api/authorize")?);
        decode(self.send(request).await?).await
    }

    /// Ends the session on the server and forgets all tokens.
    pub async fn logout(&self) -> Result<()> {
        let credential = self.auth_state().credential.clone();
        self.auth_state().set(None);
        let Some(Credential::Session { refresh, .. }) = credential else {
            return Ok(());
        };
        let response = self
            .http()
            .post(self.url("logout")?)
            .header("x-refresh-token", refresh)
            .send()
            .await?;
        check(response).await.map(drop)
    }

    /// When the access token should be refreshed. `None` for API keys or when logged out.
    pub fn refresh_due_at(&self) -> Option<SystemTime> {
        self.auth_state().lifetime().map(|l| l.refresh_at())
    }

    /// The current bearer token, for players that fetch audio URLs themselves.
    pub fn access_token(&self) -> Option<String> {
        self.auth_state().bearer().map(|(token, _)| token)
    }

    /// True while the client holds a credential.
    pub fn is_logged_in(&self) -> bool {
        self.auth_state().credential.is_some()
    }

    /// Refreshes now if the access token is due. A failed refresh is ignored while
    /// the old token is still valid.
    pub async fn refresh_if_due(&self) -> Result<()> {
        let (lifetime, generation) = {
            let state = self.auth_state();
            (state.lifetime(), state.generation)
        };
        let Some(lifetime) = lifetime else {
            return Ok(());
        };
        let now = SystemTime::now();
        if now < lifetime.refresh_at() {
            return Ok(());
        }
        match self.refresh(generation).await {
            Err(Error::Network(_)) if now < lifetime.expires_at => Ok(()),
            result => result,
        }
    }

    /// Refreshes the tokens now, for example after the machine woke from sleep.
    pub async fn refresh_now(&self) -> Result<()> {
        let generation = self.auth_state().generation;
        self.refresh(generation).await
    }

    /// Replaces the access token with an invalid one, to test 401 recovery.
    #[cfg(feature = "integration")]
    #[doc(hidden)]
    pub fn break_access_token(&self) {
        let mut state = self.auth_state();
        if let Some(Credential::Session { access, .. }) = &mut state.credential {
            *access = "invalid".into();
        }
    }

    /// Refreshes the tokens unless another caller already did since `seen_generation`.
    pub(crate) async fn refresh(&self, seen_generation: u64) -> Result<()> {
        let _single_flight = self.refresh_lock().lock().await;
        let refresh = {
            let state = self.auth_state();
            if state.generation != seen_generation {
                return Ok(());
            }
            match &state.credential {
                Some(Credential::Session { refresh, .. }) => refresh.clone(),
                _ => return Err(Error::LoginRequired),
            }
        };
        let body = self.refresh_request(&refresh).await?;
        self.install_session(&body)
    }

    async fn refresh_request(&self, refresh_token: &str) -> Result<Value> {
        let response = self
            .http()
            .post(self.url("auth/refresh")?)
            .header("x-refresh-token", refresh_token)
            .send()
            .await?;
        if matches!(
            response.status(),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
        ) {
            self.auth_state().set(None);
            return Err(Error::LoginRequired);
        }
        decode(check(response).await?).await
    }

    /// Stores the refresh token from a login or refresh body, then switches to the new tokens.
    ///
    /// The new tokens are used even when storing fails, because the server may
    /// already have invalidated the old refresh token. The store error is still returned.
    fn install_session(&self, body: &Value) -> Result<()> {
        let credential = session_credential(body)?;
        let Credential::Session { refresh, .. } = &credential else {
            unreachable!("session_credential returns sessions")
        };
        let stored = self.sink().store_refresh_token(refresh);
        self.auth_state().set(Some(credential));
        stored.map_err(Error::TokenStore)
    }
}

fn session_credential(body: &Value) -> Result<Credential> {
    let user = &body["user"];
    let text = |key: &str| user[key].as_str().filter(|s| !s.is_empty());
    let access = text("accessToken").or_else(|| text("token"));
    let (Some(access), Some(refresh)) = (access, text("refreshToken")) else {
        return Err(Error::Unsupported(Feature::RefreshTokens));
    };
    Ok(Credential::Session {
        access: access.to_owned(),
        refresh: refresh.to_owned(),
        lifetime: Lifetime::of(access),
    })
}

fn parse_login(body: Value) -> Result<LoginResponse> {
    serde_json::from_value(body).map_err(|source| Error::Decode {
        what: "login response".into(),
        source,
    })
}
