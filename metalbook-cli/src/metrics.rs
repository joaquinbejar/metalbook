//! Per-pass counters for the keeper (`docs/PLAN-improvements.md`, P3 baseline and P1
//! measurement): transactions sent and their serialized size, through [`Metered`] around the
//! chain transport, and Hermes requests by status, through [`CountingHttp`] around the HTTP
//! client. Counters only: no URL, key, signature or body is kept. Also the Hermes pacing floor
//! ([`hermes_interval`]).

use std::cell::Cell;
use std::time::Duration;

use metalbook_keeper::{HttpGet, HttpResponse, KeeperError};
use pyth_post::{PostError, Transport};
use solana_address::Address;
use solana_instruction::Instruction;
use solana_keypair::Keypair;
use solana_signer::Signer;
use solana_transaction::Transaction;

use crate::CliError;

/// The shortest Hermes interval a keeper accepts, in seconds: 5 requests a minute, the primary
/// keeper's share of metalbook's 7 (`docs/PLAN-improvements.md`, P2; the key is shared with
/// varswap, which gets the other 3).
pub const MIN_HERMES_INTERVAL_S: u64 = 12;

/// The Hermes interval of `seconds`, refusing anything under [`MIN_HERMES_INTERVAL_S`].
///
/// # Errors
///
/// [`CliError::Usage`] below the floor.
pub fn hermes_interval(seconds: u64) -> Result<Duration, CliError> {
    if seconds < MIN_HERMES_INTERVAL_S {
        return Err(CliError::Usage(format!(
            "--hermes-interval-s {seconds} is under the {MIN_HERMES_INTERVAL_S} s floor \
             (metalbook's Hermes budget is 7 requests a minute across its processes)"
        )));
    }
    Ok(Duration::from_secs(seconds))
}

/// metalbook's shared Hermes limiter for the key in `config` (`docs/PLAN-phase2.md`, R1 step
/// 3a): [`metalbook_keeper::DEFAULT_REQUESTS_PER_MINUTE`] a minute across every metalbook process
/// on this host, in pyth-post's lock directory (`$PYTH_POST_LOCK_DIR`, default
/// `~/.local/state/pyth-post`). The files are named after a hash of the key, never the key.
///
/// # Errors
///
/// [`CliError::Usage`] when the directory cannot be found or created.
pub fn hermes_limiter(
    config: &metalbook_keeper::HermesConfig,
) -> Result<metalbook_keeper::HermesLimiter, CliError> {
    let dir = pyth_post::default_lock_dir()
        .map_err(|e| CliError::Usage(format!("Hermes limiter directory: {e}")))?;
    metalbook_keeper::HermesLimiter::for_key(
        &dir,
        config.api_key(),
        metalbook_keeper::DEFAULT_REQUESTS_PER_MINUTE,
    )
    .map_err(|e| CliError::Usage(format!("Hermes limiter: {e}")))
}

/// Send attempts through a [`Metered`] transport since its last [`Metered::take`]. An
/// attempt is a call to `Transport::send`: a failed one may never have reached the cluster (the
/// blockhash fetch failed) or may have landed anyway (the confirmation failed), so `bytes`
/// measures what was attempted, not what went over the wire.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TxCounts {
    /// Attempts that returned a signature (confirmed).
    pub ok: u64,
    /// Attempts that returned an error.
    pub failed: u64,
    /// Serialized size of every attempted transaction, signatures included, in bytes.
    pub bytes: u64,
    /// The largest attempted transaction since the last take, in bytes.
    pub max_bytes: u64,
}

impl TxCounts {
    /// Every attempt.
    #[must_use]
    pub fn attempts(&self) -> u64 {
        self.ok.saturating_add(self.failed)
    }
}

/// Serialized size of a legacy transaction of `instructions` paid by `payer`: the short-vector
/// signature count, 64 bytes per required signature, then the message.
#[must_use]
pub fn legacy_tx_size(instructions: &[Instruction], payer: &Address) -> u64 {
    let tx = Transaction::new_with_payer(instructions, Some(payer));
    let signatures = u64::from(tx.message.header.num_required_signatures);
    let message = u64::try_from(tx.message_data().len()).unwrap_or(u64::MAX);
    // Fewer than 128 signatures: the short-vector length takes one byte.
    signatures
        .saturating_mul(64)
        .saturating_add(message)
        .saturating_add(1)
}

/// A [`Transport`] that counts what it sends.
#[derive(Debug)]
pub struct Metered<T> {
    inner: T,
    counts: TxCounts,
}

impl<T> Metered<T> {
    /// Wraps `inner` with zeroed counters.
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            counts: TxCounts::default(),
        }
    }

    /// The wrapped transport.
    #[must_use]
    pub fn inner(&self) -> &T {
        &self.inner
    }

    /// The counters since the last take (or creation), without resetting them.
    #[must_use]
    pub fn counts(&self) -> TxCounts {
        self.counts
    }

    /// The counters since the last take (or creation), resetting them: call it at every pass
    /// boundary, so each pass's maximum is its own.
    pub fn take(&mut self) -> TxCounts {
        std::mem::take(&mut self.counts)
    }
}

impl<T: Transport> Transport for Metered<T> {
    fn now(&self) -> Result<i64, PostError> {
        self.inner.now()
    }

    fn account(&self, account: &Address) -> Result<Option<(Address, Vec<u8>)>, PostError> {
        self.inner.account(account)
    }

    fn block_height(&self) -> Result<u64, PostError> {
        self.inner.block_height()
    }

    fn last_send_expiry(&self) -> Option<u64> {
        self.inner.last_send_expiry()
    }

    fn program_accounts(
        &self,
        program: &Address,
        offset: usize,
        bytes: &[u8],
    ) -> Result<Vec<Address>, PostError> {
        self.inner.program_accounts(program, offset, bytes)
    }

    fn rent_exempt(&self, len: usize) -> Result<u64, PostError> {
        self.inner.rent_exempt(len)
    }

    fn send(
        &mut self,
        instructions: &[Instruction],
        payer: &Keypair,
        extra: Option<&Keypair>,
    ) -> Result<String, PostError> {
        let size = legacy_tx_size(instructions, &payer.pubkey());
        self.counts.bytes = self.counts.bytes.saturating_add(size);
        self.counts.max_bytes = self.counts.max_bytes.max(size);
        let sent = self.inner.send(instructions, payer, extra);
        if sent.is_ok() {
            self.counts.ok = self.counts.ok.saturating_add(1);
        } else {
            self.counts.failed = self.counts.failed.saturating_add(1);
        }
        sent
    }
}

/// Hermes requests through a [`CountingHttp`], by response class.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HttpCounts {
    /// 200 responses.
    pub ok: u64,
    /// 404 responses (no update at that time yet).
    pub not_found: u64,
    /// 429 responses (rate limited).
    pub rate_limited: u64,
    /// Any other status, or a transport failure.
    pub other: u64,
}

impl HttpCounts {
    /// Every request sent.
    #[must_use]
    pub fn total(&self) -> u64 {
        self.ok
            .saturating_add(self.not_found)
            .saturating_add(self.rate_limited)
            .saturating_add(self.other)
    }

    /// `self - earlier`, saturating: the counts of one pass.
    #[must_use]
    pub fn since(&self, earlier: &Self) -> Self {
        Self {
            ok: self.ok.saturating_sub(earlier.ok),
            not_found: self.not_found.saturating_sub(earlier.not_found),
            rate_limited: self.rate_limited.saturating_sub(earlier.rate_limited),
            other: self.other.saturating_sub(earlier.other),
        }
    }
}

/// An [`HttpGet`] that counts requests by response class. Every Hermes request of
/// `HermesSource` goes through its one `HttpGet` (timestamp queries, 404s and `/latest`
/// alike), so this counts exactly what the key spends.
#[derive(Debug)]
pub struct CountingHttp<H> {
    inner: H,
    counts: Cell<HttpCounts>,
}

impl<H> CountingHttp<H> {
    /// Wraps `inner` with zeroed counters.
    pub fn new(inner: H) -> Self {
        Self {
            inner,
            counts: Cell::new(HttpCounts::default()),
        }
    }

    /// The counters since creation.
    #[must_use]
    pub fn counts(&self) -> HttpCounts {
        self.counts.get()
    }
}

impl<H: HttpGet> HttpGet for CountingHttp<H> {
    fn get(&self, url: &str, headers: &[(String, String)]) -> Result<HttpResponse, KeeperError> {
        let response = self.inner.get(url, headers);
        let mut c = self.counts.get();
        match response.as_ref().map(|r| r.status) {
            Ok(200) => c.ok = c.ok.saturating_add(1),
            Ok(404) => c.not_found = c.not_found.saturating_add(1),
            Ok(429) => c.rate_limited = c.rate_limited.saturating_add(1),
            _ => c.other = c.other.saturating_add(1),
        }
        self.counts.set(c);
        response
    }
}

impl<H: HttpGet> HttpGet for &CountingHttp<H> {
    fn get(&self, url: &str, headers: &[(String, String)]) -> Result<HttpResponse, KeeperError> {
        (*self).get(url, headers)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::disallowed_macros)]
mod tests {
    use super::*;
    use solana_instruction::AccountMeta;

    #[test]
    fn test_hermes_interval_floor() {
        assert!(hermes_interval(11).is_err());
        assert!(hermes_interval(0).is_err());
        assert_eq!(hermes_interval(12).unwrap(), Duration::from_secs(12));
        assert_eq!(hermes_interval(30).unwrap(), Duration::from_secs(30));
    }

    #[test]
    fn test_legacy_tx_size_by_hand() {
        let payer = Address::new_from_array([1; 32]);
        let program = Address::new_from_array([2; 32]);
        let account = Address::new_from_array([3; 32]);
        let ix = Instruction::new_with_bytes(program, &[7], vec![AccountMeta::new(account, false)]);
        // Message: header 3, 3 keys (1 + 96), blockhash 32, 1 instruction (1 + program index
        // 1, account count 1, account index 1, data length 1, data 1) = 138. Plus the
        // signature count (1) and one signature (64).
        assert_eq!(legacy_tx_size(std::slice::from_ref(&ix), &payer), 203);
        // A second signer adds a key and a signature.
        let signer = Address::new_from_array([4; 32]);
        let ix2 = Instruction::new_with_bytes(program, &[7], vec![AccountMeta::new(signer, true)]);
        assert_eq!(legacy_tx_size(&[ix2], &payer), 203 + 64);
    }

    /// A transport whose sends succeed, or fail when `fail` is set; nothing else is used.
    struct Stub {
        fail: bool,
    }

    impl Transport for Stub {
        fn now(&self) -> Result<i64, PostError> {
            Ok(0)
        }
        fn account(&self, _: &Address) -> Result<Option<(Address, Vec<u8>)>, PostError> {
            Ok(None)
        }
        fn block_height(&self) -> Result<u64, PostError> {
            Ok(0)
        }
        fn last_send_expiry(&self) -> Option<u64> {
            None
        }
        fn program_accounts(
            &self,
            _: &Address,
            _: usize,
            _: &[u8],
        ) -> Result<Vec<Address>, PostError> {
            Ok(Vec::new())
        }
        fn rent_exempt(&self, _: usize) -> Result<u64, PostError> {
            Ok(0)
        }
        fn send(
            &mut self,
            _: &[Instruction],
            _: &Keypair,
            _: Option<&Keypair>,
        ) -> Result<String, PostError> {
            if self.fail {
                Err(PostError::Chain("stub".into()))
            } else {
                Ok("sig".into())
            }
        }
    }

    #[test]
    fn test_each_pass_reports_its_own_maximum() {
        let payer = Keypair::new();
        let program = Address::new_from_array([2; 32]);
        let large = Instruction::new_with_bytes(program, &[0; 500], vec![]);
        let small = Instruction::new_with_bytes(program, &[0; 1], vec![]);
        let mut m = Metered::new(Stub { fail: false });
        // Pass 1: one large transaction.
        m.send(std::slice::from_ref(&large), &payer, None).unwrap();
        let p1 = m.take();
        assert_eq!(p1.max_bytes, legacy_tx_size(&[large], &payer.pubkey()));
        // Pass 2: one small transaction; its maximum is its own, not pass 1's.
        m.send(std::slice::from_ref(&small), &payer, None).unwrap();
        let p2 = m.take();
        let small_size = legacy_tx_size(&[small], &payer.pubkey());
        assert_eq!(
            (p2.ok, p2.failed, p2.bytes, p2.max_bytes),
            (1, 0, small_size, small_size)
        );
        assert!(p2.max_bytes < p1.max_bytes);
        // Pass 3: nothing.
        assert_eq!(m.take(), TxCounts::default());
    }

    #[test]
    fn test_failed_attempts_are_counted_as_attempts() {
        let payer = Keypair::new();
        let ix = Instruction::new_with_bytes(Address::new_from_array([2; 32]), &[1], vec![]);
        let mut m = Metered::new(Stub { fail: true });
        assert!(m.send(std::slice::from_ref(&ix), &payer, None).is_err());
        let c = m.take();
        assert_eq!((c.ok, c.failed, c.attempts()), (0, 1, 1));
        assert_eq!(c.bytes, legacy_tx_size(&[ix], &payer.pubkey()));
    }

    struct Fixed(u16);

    impl HttpGet for Fixed {
        fn get(&self, _: &str, _: &[(String, String)]) -> Result<HttpResponse, KeeperError> {
            if self.0 == 0 {
                return Err(KeeperError::Source("transport".into()));
            }
            Ok(HttpResponse {
                status: self.0,
                body: String::new(),
            })
        }
    }

    #[test]
    fn test_counting_http_classifies_every_response() {
        for (status, want) in [
            (
                200,
                HttpCounts {
                    ok: 1,
                    ..HttpCounts::default()
                },
            ),
            (
                404,
                HttpCounts {
                    not_found: 1,
                    ..HttpCounts::default()
                },
            ),
            (
                429,
                HttpCounts {
                    rate_limited: 1,
                    ..HttpCounts::default()
                },
            ),
            (
                500,
                HttpCounts {
                    other: 1,
                    ..HttpCounts::default()
                },
            ),
            (
                0,
                HttpCounts {
                    other: 1,
                    ..HttpCounts::default()
                },
            ),
        ] {
            let http = CountingHttp::new(Fixed(status));
            let by_ref = &http;
            let _ = HttpGet::get(&by_ref, "u", &[]);
            assert_eq!(http.counts(), want, "status {status}");
            assert_eq!(http.counts().total(), 1);
        }
        let http = CountingHttp::new(Fixed(200));
        let before = http.counts();
        let _ = http.get("u", &[]);
        let _ = http.get("u", &[]);
        assert_eq!(http.counts().since(&before).ok, 2);
    }
}
