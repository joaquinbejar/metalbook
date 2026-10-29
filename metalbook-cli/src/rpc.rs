//! Solana JSON-RPC over one blocking HTTP client: the four methods the CLI needs.
//!
//! The URL is never put in an error (RPC providers carry API keys in it), and transport errors
//! are reduced to their kind for the same reason.

use std::time::Duration;

use base64::Engine;
use metalbook_sdk::Address;
use serde_json::{json, Value};
use solana_hash::Hash;
use solana_transaction::Transaction;

use crate::error::CliError;
use crate::{AccountInfo, Rpc};

/// Blocking HTTP POST of a JSON body.
pub trait HttpPost {
    /// Posts `body` (JSON) to `url`; returns the status and the body text.
    ///
    /// # Errors
    ///
    /// [`CliError::Transport`] when the request does not complete.
    fn post_json(&self, url: &str, body: &str) -> Result<(u16, String), CliError>;
}

/// [`HttpPost`] through `ureq`, every status returned as a response, 30-second timeout.
pub struct UreqPost {
    agent: ureq::Agent,
}

impl UreqPost {
    /// A new agent.
    #[must_use]
    pub fn new() -> Self {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(30)))
            .build()
            .into();
        Self { agent }
    }
}

impl Default for UreqPost {
    fn default() -> Self {
        Self::new()
    }
}

/// The kind of a transport failure, without the URL some variants carry.
fn transport_kind(e: &ureq::Error) -> &'static str {
    match e {
        ureq::Error::Timeout(_) => "timeout",
        ureq::Error::HostNotFound => "host not found",
        ureq::Error::ConnectionFailed => "connection failed",
        ureq::Error::Io(_) => "I/O error",
        ureq::Error::BadUri(_) => "invalid URL",
        ureq::Error::Tls(_) | ureq::Error::Rustls(_) | ureq::Error::Pem(_) => "TLS error",
        ureq::Error::BodyExceedsLimit(_) => "response too large",
        _ => "request failed",
    }
}

impl HttpPost for UreqPost {
    fn post_json(&self, url: &str, body: &str) -> Result<(u16, String), CliError> {
        let mut response = self
            .agent
            .post(url)
            .header("Content-Type", "application/json")
            .send(body)
            .map_err(|e| CliError::Transport(transport_kind(&e)))?;
        let status = response.status().as_u16();
        let text = response
            .body_mut()
            .read_to_string()
            .map_err(|e| CliError::Transport(transport_kind(&e)))?;
        Ok((status, text))
    }
}

/// [`Rpc`] over Solana JSON-RPC at `confirmed` commitment.
pub struct JsonRpc<H> {
    http: H,
    url: String,
    polls: u32,
    poll_interval: Duration,
    next_id: u64,
}

impl<H: HttpPost> JsonRpc<H> {
    /// A client for `url` that polls a sent transaction's status up to 60 times, 1 s apart.
    #[must_use]
    pub fn new(http: H, url: impl Into<String>) -> Self {
        Self {
            http,
            url: url.into(),
            polls: 60,
            poll_interval: Duration::from_secs(1),
            next_id: 1,
        }
    }

    /// Changes the confirmation polling budget.
    #[must_use]
    pub fn with_polling(mut self, polls: u32, interval: Duration) -> Self {
        self.polls = polls;
        self.poll_interval = interval;
        self
    }

    /// One JSON-RPC call; returns `result`.
    fn call(&mut self, method: &str, params: Value) -> Result<Value, CliError> {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let body = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let (status, text) = self.http.post_json(&self.url, &body.to_string())?;
        if status != 200 {
            return Err(CliError::HttpStatus(status));
        }
        let mut answer: Value =
            serde_json::from_str(&text).map_err(|_| CliError::Malformed("not JSON"))?;
        if let Some(error) = answer.get("error") {
            return Err(rpc_error(error));
        }
        answer
            .get_mut("result")
            .map(Value::take)
            .ok_or(CliError::Malformed("no result"))
    }
}

/// A JSON-RPC error, with the program logs of a failed preflight appended.
fn rpc_error(error: &Value) -> CliError {
    let code = error.get("code").and_then(Value::as_i64).unwrap_or(0);
    let mut message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("(no message)")
        .to_owned();
    let logs = error
        .get("data")
        .and_then(|d| d.get("logs"))
        .and_then(Value::as_array);
    for line in logs.into_iter().flatten().filter_map(Value::as_str) {
        message.push_str("\n  ");
        message.push_str(line);
    }
    CliError::Rpc { code, message }
}

impl<H: HttpPost> Rpc for JsonRpc<H> {
    fn latest_blockhash(&mut self) -> Result<Hash, CliError> {
        let result = self.call("getLatestBlockhash", json!([{"commitment": "confirmed"}]))?;
        result
            .pointer("/value/blockhash")
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<Hash>().ok())
            .ok_or(CliError::Malformed("no blockhash"))
    }

    fn account(&mut self, address: &Address) -> Result<Option<AccountInfo>, CliError> {
        let result = self.call(
            "getAccountInfo",
            json!([address.to_string(), {"encoding": "base64", "commitment": "confirmed"}]),
        )?;
        let value = result
            .get("value")
            .ok_or(CliError::Malformed("no account value"))?;
        if value.is_null() {
            return Ok(None);
        }
        let owner = value
            .get("owner")
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<Address>().ok())
            .ok_or(CliError::Malformed("no account owner"))?;
        let data = value
            .pointer("/data/0")
            .and_then(Value::as_str)
            .and_then(|s| base64::engine::general_purpose::STANDARD.decode(s).ok())
            .ok_or(CliError::Malformed("no base64 account data"))?;
        Ok(Some(AccountInfo { owner, data }))
    }

    fn send(&mut self, tx: &Transaction) -> Result<(), CliError> {
        let bytes = wincode::serialize(tx).map_err(|_| CliError::Encode("serialization"))?;
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        let result = self.call(
            "sendTransaction",
            json!([encoded, {"encoding": "base64", "preflightCommitment": "confirmed"}]),
        )?;
        let signature = result
            .as_str()
            .ok_or(CliError::Malformed("no signature"))?
            .to_owned();
        for poll in 0..self.polls {
            if poll > 0 {
                std::thread::sleep(self.poll_interval);
            }
            let statuses = self.call("getSignatureStatuses", json!([[signature]]))?;
            let status = statuses
                .pointer("/value/0")
                .ok_or(CliError::Malformed("no signature status"))?;
            if status.is_null() {
                continue;
            }
            if let Some(err) = status.get("err").filter(|e| !e.is_null()) {
                return Err(CliError::TxFailed {
                    signature,
                    err: err.to_string(),
                });
            }
            let level = status.get("confirmationStatus").and_then(Value::as_str);
            if matches!(level, Some("confirmed" | "finalized")) {
                return Ok(());
            }
        }
        Err(CliError::NotConfirmed {
            signature,
            polls: self.polls,
        })
    }
}
