//! Reads the time claims of a JWT without checking its signature.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;

/// Shortest gap left between a scheduled refresh and the token's expiry.
const MIN_MARGIN: Duration = Duration::from_secs(30);
/// Margin used when the token has no `iat` claim.
const DEFAULT_MARGIN: Duration = Duration::from_secs(60);

#[derive(Deserialize)]
struct Claims {
    iat: Option<u64>,
    exp: Option<u64>,
}

/// Issue and expiry time of a token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lifetime {
    pub issued_at: Option<SystemTime>,
    pub expires_at: SystemTime,
}

impl Lifetime {
    /// Reads `iat` and `exp`. `None` for tokens without `exp`, such as API keys.
    pub fn of(token: &str) -> Option<Self> {
        let payload = token.split('.').nth(1)?;
        let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
        let claims: Claims = serde_json::from_slice(&bytes).ok()?;
        Some(Self {
            issued_at: claims.iat.map(epoch),
            expires_at: epoch(claims.exp?),
        })
    }

    /// When to refresh: a tenth of the lifetime before expiry, at least 30 s before.
    pub fn refresh_at(&self) -> SystemTime {
        let margin = self
            .issued_at
            .and_then(|iat| self.expires_at.duration_since(iat).ok())
            .map_or(DEFAULT_MARGIN, |lifetime| lifetime / 10)
            .max(MIN_MARGIN);
        self.expires_at - margin
    }
}

fn epoch(secs: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(claims: &str) -> String {
        format!("e30.{}.sig", URL_SAFE_NO_PAD.encode(claims))
    }

    #[test]
    fn reads_claims() {
        let life = Lifetime::of(&token(r#"{"iat":1000,"exp":4600}"#)).unwrap();
        assert_eq!(life.issued_at, Some(epoch(1000)));
        assert_eq!(life.expires_at, epoch(4600));
        assert_eq!(life.refresh_at(), epoch(4600 - 360));
    }

    #[test]
    fn short_or_unknown_lifetimes_keep_a_margin() {
        let short = Lifetime::of(&token(r#"{"iat":1000,"exp":1100}"#)).unwrap();
        assert_eq!(short.refresh_at(), epoch(1070));
        let no_iat = Lifetime::of(&token(r#"{"exp":5000}"#)).unwrap();
        assert_eq!(no_iat.refresh_at(), epoch(4940));
    }

    #[test]
    fn api_keys_and_garbage_have_no_lifetime() {
        assert_eq!(Lifetime::of(&token(r#"{"keyId":"k","iat":1}"#)), None);
        assert_eq!(Lifetime::of("not a jwt"), None);
    }
}
