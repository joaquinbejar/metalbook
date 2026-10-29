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
    /// A chain operation (read, post, settle) failed.
    #[error("chain operation failed: {0}")]
    Chain(String),
    /// A configuration value is missing or invalid.
    #[error("configuration: {0}")]
    Config(&'static str),
}

impl From<metalbook_core::CoreError> for KeeperError {
    fn from(e: metalbook_core::CoreError) -> Self {
        Self::Core(e)
    }
}
