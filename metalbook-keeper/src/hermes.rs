//! Hermes configuration. The API key is read only from the environment, never from a file and
//! never printed: `Debug` redacts it.

use std::fmt;

use crate::KeeperError;

/// Environment variable that holds the Hermes API key.
pub const HERMES_API_KEY_VAR: &str = "METALBOOK_HERMES_API_KEY";

/// Environment variable that overrides the Hermes base URL.
pub const HERMES_URL_VAR: &str = "METALBOOK_HERMES_URL";

/// Default Hermes endpoint.
pub const DEFAULT_HERMES_URL: &str = "https://hermes.pyth.network";

/// How to reach Hermes. The HTTP client itself arrives with the devnet keeper (milestone 7b).
#[derive(Clone, PartialEq, Eq)]
pub struct HermesConfig {
    base_url: String,
    api_key: String,
}

impl HermesConfig {
    /// Reads the key from [`HERMES_API_KEY_VAR`] and the URL from `METALBOOK_HERMES_URL`
    /// (default [`DEFAULT_HERMES_URL`]).
    ///
    /// # Errors
    ///
    /// `KeeperError::Config` if the key is missing or empty (Hermes price endpoints return 401
    /// without one, `docs/PLAN.md` verified facts).
    pub fn from_env() -> Result<Self, KeeperError> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// As [`HermesConfig::from_env`], with an explicit variable lookup (for tests).
    ///
    /// # Errors
    ///
    /// As [`HermesConfig::from_env`].
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, KeeperError> {
        let api_key = lookup(HERMES_API_KEY_VAR)
            .filter(|k| !k.trim().is_empty())
            .ok_or(KeeperError::Config("METALBOOK_HERMES_API_KEY is not set"))?;
        let base_url = lookup(HERMES_URL_VAR).unwrap_or_else(|| DEFAULT_HERMES_URL.to_string());
        Ok(Self { base_url, api_key })
    }

    /// Base URL.
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// The API key, for the request header only.
    #[must_use]
    pub fn api_key(&self) -> &str {
        &self.api_key
    }
}

impl fmt::Debug for HermesConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HermesConfig")
            .field("base_url", &self.base_url)
            .field("api_key", &"<redacted>")
            .finish()
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::disallowed_macros
)]
mod tests {
    use super::*;

    #[test]
    fn test_key_comes_from_the_environment_and_is_redacted() {
        let missing = HermesConfig::from_lookup(|_| None);
        assert!(matches!(missing, Err(KeeperError::Config(_))));
        let empty =
            HermesConfig::from_lookup(|n| (n == HERMES_API_KEY_VAR).then(|| " ".to_string()));
        assert!(matches!(empty, Err(KeeperError::Config(_))));
        let c =
            HermesConfig::from_lookup(|n| (n == HERMES_API_KEY_VAR).then(|| "s3cr3t".to_string()))
                .unwrap();
        assert_eq!((c.base_url(), c.api_key()), (DEFAULT_HERMES_URL, "s3cr3t"));
        let shown = format!("{c:?}");
        assert!(!shown.contains("s3cr3t") && shown.contains("<redacted>"));
    }
}
