use reqwest::StatusCode;

use crate::version::Feature;

/// Boxed error from a [`crate::TokenSink`].
pub type SinkError = Box<dyn std::error::Error + Send + Sync>;

/// Everything an [`crate::AbsClient`] call can fail with.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The request did not reach the server or the connection broke.
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
    /// The server URL could not be parsed or joined.
    #[error("invalid server URL: {0}")]
    InvalidUrl(String),
    /// Username or password were rejected at login.
    #[error("invalid username or password")]
    InvalidCredentials,
    /// No valid token is left; the user has to log in again.
    #[error("login required")]
    LoginRequired,
    /// The server answered with a non-success status.
    #[error("server returned {status}: {body}")]
    Server { status: StatusCode, body: String },
    /// The response body did not match the expected shape.
    #[error("could not decode {what}: {source}")]
    Decode {
        what: String,
        source: serde_json::Error,
    },
    /// The server version is too old for this call.
    #[error("server does not support {0:?}")]
    Unsupported(Feature),
    /// Persisting a new refresh token failed; the old one is kept.
    #[error("could not store refresh token: {0}")]
    TokenStore(SinkError),
}

impl Error {
    /// True when the user has to enter credentials again.
    pub fn needs_login(&self) -> bool {
        matches!(self, Self::LoginRequired | Self::InvalidCredentials)
    }

    /// The HTTP status for [`Error::Server`] errors.
    pub fn status(&self) -> Option<StatusCode> {
        match self {
            Self::Server { status, .. } => Some(*status),
            _ => None,
        }
    }
}

/// Result alias for this crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;
