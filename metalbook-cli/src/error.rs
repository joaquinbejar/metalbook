//! CLI errors. None carries the RPC URL (providers put API keys in it) or key material.

/// Everything that can go wrong in a CLI command.
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    /// A bad argument value.
    #[error("{0}")]
    Usage(String),
    /// The keypair file could not be read or is not a Solana keypair.
    #[error("keypair file {path}: {what}")]
    Keypair {
        /// The file path.
        path: String,
        /// What went wrong, never the file's content.
        what: &'static str,
    },
    /// The config parameters file is invalid.
    #[error("config parameters file: {0}")]
    ParamsFile(String),
    /// The HTTP request did not complete.
    #[error("RPC transport error: {0}")]
    Transport(&'static str),
    /// The RPC answered with a non-200 status.
    #[error("RPC answered HTTP {0}")]
    HttpStatus(u16),
    /// The RPC answered with a JSON-RPC error (preflight failures carry the program logs).
    #[error("RPC error {code}: {message}")]
    Rpc {
        /// JSON-RPC error code.
        code: i64,
        /// The message, followed by the program logs when the RPC sends them.
        message: String,
    },
    /// The RPC answer does not have the expected shape.
    #[error("malformed RPC response: {0}")]
    Malformed(&'static str),
    /// The transaction landed and failed.
    #[error("transaction {signature} failed: {err}")]
    TxFailed {
        /// The transaction signature.
        signature: String,
        /// The error as reported.
        err: String,
    },
    /// The transaction was not confirmed within the polling budget.
    #[error("transaction {signature} not confirmed after {polls} polls")]
    NotConfirmed {
        /// The transaction signature.
        signature: String,
        /// How many status polls were made.
        polls: u32,
    },
    /// A required account does not exist.
    #[error("account {0} not found")]
    AccountNotFound(String),
    /// An account exists but is not the expected metalbook account.
    #[error("account {address} is not a metalbook {what}")]
    Decode {
        /// The account address.
        address: String,
        /// The expected account kind.
        what: &'static str,
    },
    /// Signing or encoding the transaction failed.
    #[error("transaction encoding: {0}")]
    Encode(&'static str),
}
