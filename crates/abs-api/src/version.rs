use std::fmt;
use std::str::FromStr;

/// Server features that depend on the Audiobookshelf version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feature {
    /// `/auth/refresh` with rotating refresh tokens and API keys.
    RefreshTokens,
    /// `GET /api/me/progress`. Older servers send progress in the login payload.
    MeProgress,
    /// API keys authenticate the realtime socket.
    ApiKeySocket,
}

impl Feature {
    /// First server version that has the feature.
    pub fn since(self) -> ServerVersion {
        match self {
            Self::RefreshTokens => ServerVersion::new(2, 26, 0),
            Self::MeProgress => ServerVersion::new(2, 36, 0),
            Self::ApiKeySocket => ServerVersion::new(2, 37, 0),
        }
    }
}

/// A `major.minor.patch` server version from `/status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ServerVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl ServerVersion {
    /// Oldest server this client talks to.
    pub const MINIMUM: Self = Self::new(2, 26, 0);

    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// True when this version has `feature`.
    pub fn supports(self, feature: Feature) -> bool {
        self >= feature.since()
    }
}

impl FromStr for ServerVersion {
    type Err = String;

    /// Parses `2.37.2`, ignoring a leading `v` and any `-suffix`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let core = s.trim().trim_start_matches('v');
        let core = core.split(['-', '+']).next().unwrap_or_default();
        let mut parts = core.split('.').map(str::parse::<u32>);
        let mut next = || parts.next().unwrap_or(Ok(0)).map_err(|_| s.to_owned());
        Ok(Self::new(next()?, next()?, next()?))
    }
}

impl fmt::Display for ServerVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_orders() {
        let v: ServerVersion = "2.26.3".parse().unwrap();
        assert_eq!(v, ServerVersion::new(2, 26, 3));
        assert_eq!("v2.37".parse(), Ok(ServerVersion::new(2, 37, 0)));
        assert_eq!("2.30.0-beta".parse(), Ok(ServerVersion::new(2, 30, 0)));
        assert!("two".parse::<ServerVersion>().is_err());
        assert!(ServerVersion::new(2, 37, 2) > v);
    }

    #[test]
    fn gates_features() {
        let old = ServerVersion::new(2, 26, 3);
        let new = ServerVersion::new(2, 37, 2);
        assert!(old.supports(Feature::RefreshTokens));
        assert!(!old.supports(Feature::MeProgress));
        assert!(new.supports(Feature::MeProgress));
    }
}
