//! Adding accounts and reconnecting them at start.

use std::sync::{Arc, Mutex};

use abs_api::models::LoginResponse;
use abs_api::reqwest::Client;
use abs_api::{AbsClient, Error as ApiError};

use crate::credentials::{CredentialError, Credentials};
use crate::db::{Account, Db, DbError, NewAccount};

/// Errors of the account flows.
#[derive(Debug, thiserror::Error)]
pub enum AccountError {
    #[error(transparent)]
    Api(#[from] ApiError),
    #[error(transparent)]
    Db(#[from] DbError),
    #[error(transparent)]
    Credentials(#[from] CredentialError),
    /// No refresh token is stored; the user has to log in again.
    #[error("login required")]
    LoginRequired,
    #[error("no URL configured for the server")]
    NoServerUrl,
}

impl AccountError {
    /// True when the server could not be reached or the connection broke.
    pub fn is_network(&self) -> bool {
        matches!(self, Self::Api(ApiError::Network(_)))
    }

    /// True when the UI should show the login form.
    pub fn needs_login(&self) -> bool {
        match self {
            Self::Api(e) => e.needs_login(),
            Self::LoginRequired => true,
            _ => false,
        }
    }
}

/// A connected account: its row, its client and the user payload from the last login.
pub struct Session {
    pub account: Account,
    pub client: Arc<AbsClient>,
    pub login: LoginResponse,
}

/// URL -> `/status` probe -> login -> server, URL and account rows -> refresh token saved.
///
/// Adding an account that exists already logs it in again.
pub async fn add_account(
    db: &Mutex<Db>,
    credentials: &Credentials,
    http: Client,
    url: &str,
    username: &str,
    password: &str,
) -> Result<Session, AccountError> {
    let client = AbsClient::connect(http, url).await?;
    let login = client.login(username, password).await?;
    let token = client.refresh_token().ok_or(AccountError::LoginRequired)?;
    register(db, credentials, client, login, &token, false)
}

/// Like [`add_account`], but logs in with an API key, which is saved instead of a
/// refresh token. For servers that only allow single sign-on.
pub async fn add_account_with_key(
    db: &Mutex<Db>,
    credentials: &Credentials,
    http: Client,
    url: &str,
    api_key: &str,
) -> Result<Session, AccountError> {
    let client = AbsClient::connect(http, url).await?;
    let login = client.login_api_key(api_key).await?;
    register(db, credentials, client, login, api_key, true)
}

/// Writes the server, URL and account rows for a logged-in client and saves its secret.
fn register(
    db: &Mutex<Db>,
    credentials: &Credentials,
    client: AbsClient,
    login: LoginResponse,
    secret: &str,
    api_key: bool,
) -> Result<Session, AccountError> {
    let base = client.base_url().as_str().to_owned();
    let account = lock(db).write(|store| {
        let accounts = store.accounts();
        let server = match accounts.server_by_url(&base)? {
            Some(id) => id,
            None => accounts.add_server(&server_name(&client), &[&base])?,
        };
        let account = match accounts.find(&server, &login.user.id)? {
            Some(existing) => {
                accounts.set_enabled(&existing.id, true)?;
                existing
            }
            None => accounts.add_account(&NewAccount {
                server_id: &server,
                user_id: &login.user.id,
                username: &login.user.username,
            })?,
        };
        accounts.set_api_key(&account.id, api_key)?;
        Ok(Account {
            enabled: true,
            api_key,
            ..account
        })
    })?;
    credentials.save(&account.id, secret)?;
    let client = Arc::new(client.with_token_sink(credentials.sink(&account.id)));
    Ok(Session {
        account,
        client,
        login,
    })
}

/// Reconnects a stored account: first reachable URL, then the stored refresh token or API key.
pub async fn restore_account(
    db: &Mutex<Db>,
    credentials: &Credentials,
    http: Client,
    account: Account,
) -> Result<Session, AccountError> {
    let urls = lock(db)
        .store()
        .accounts()
        .server_urls(&account.server_id)?;
    let client = connect_any(http, &urls).await?;
    let secret = credentials
        .load(&account.id)?
        .ok_or(AccountError::LoginRequired)?;
    let client = client.with_token_sink(credentials.sink(&account.id));
    let login = if account.api_key {
        client.login_api_key(&secret).await?
    } else {
        client.restore_session(&secret).await?
    };
    Ok(Session {
        account,
        client: Arc::new(client),
        login,
    })
}

/// Connects an account again that has a client from earlier in this run. The new client
/// shares that client's tokens, so sessions still using the old one keep working.
pub async fn reconnect_account(
    db: &Mutex<Db>,
    http: Client,
    previous: &AbsClient,
    account: Account,
) -> Result<Session, AccountError> {
    let urls = lock(db)
        .store()
        .accounts()
        .server_urls(&account.server_id)?;
    let client = connect_any(http, &urls).await?.with_auth_of(previous);
    let login = client.authorize().await?;
    Ok(Session {
        account,
        client: Arc::new(client),
        login,
    })
}

/// True when a URL listed before `current` answers, as when the home network is back.
pub async fn earlier_url_answers(http: &Client, urls: &[String], current: &str) -> bool {
    let same = |url: &str| url.trim_end_matches('/') == current.trim_end_matches('/');
    let Some(position) = urls.iter().position(|u| same(u)) else {
        return false;
    };
    for url in &urls[..position] {
        if AbsClient::connect(http.clone(), url).await.is_ok() {
            return true;
        }
    }
    false
}

/// Tries the URLs in order and returns the first server that answers.
async fn connect_any(http: Client, urls: &[String]) -> Result<AbsClient, AccountError> {
    let mut last = AccountError::NoServerUrl;
    for url in urls {
        match AbsClient::connect(http.clone(), url).await {
            Ok(client) => return Ok(client),
            Err(e) => last = e.into(),
        }
    }
    Err(last)
}

fn server_name(client: &AbsClient) -> String {
    client
        .base_url()
        .host_str()
        .unwrap_or("Audiobookshelf")
        .to_owned()
}

pub(crate) fn lock(db: &Mutex<Db>) -> std::sync::MutexGuard<'_, Db> {
    db.lock().unwrap_or_else(|p| p.into_inner())
}
