//! Keeper errors.

/// Why a keeper step failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeeperError {
    /// The series account is missing, of the wrong size, or not a series.
    #[error("series account cannot be decoded: {0}")]
    Decode(&'static str),
    /// The stored series state is rejected by `metalbook-core`.
    #[error("series state rejected: {0:?}")]
    Core(metalbook_core::CoreError),
    /// The price source failed.
    #[error("price source failed: {0}")]
    Source(String),
    /// A price-source response could not be used. Carries only the HTTP status, the body
    /// length and a fixed category, never response content (which could reflect the API key).
    #[error("malformed price-source response (HTTP {status}, {byte_len} bytes): {what}")]
    MalformedResponse {
        /// HTTP status of the response.
        status: u16,
        /// Length of the body, bytes.
        byte_len: usize,
        /// What was wrong, from a fixed set of descriptions.
        what: &'static str,
    },
    /// A chain operation (read, post, settle) failed.
    #[error("chain operation failed: {0}")]
    Chain(String),
    /// A Hermes binary update is malformed or lacks the feed (milestone 7b-2).
    #[error("price update: {0}")]
    Update(&'static str),
    /// A configuration value is missing or invalid.
    #[error("configuration: {0}")]
    Config(&'static str),
}

impl From<metalbook_core::CoreError> for KeeperError {
    fn from(e: metalbook_core::CoreError) -> Self {
        Self::Core(e)
    }
}
