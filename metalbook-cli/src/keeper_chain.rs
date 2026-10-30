//! The keeper's [`Chain`] over an [`Rpc`] (milestone 7c): reads accounts and the clock, posts
//! a print through Wormhole and the Pyth receiver followed by `sample_settlement`
//! (`metalbook_keeper::receiver`), and settles. The binary update of a print comes from an
//! [`UpdateSource`] (Hermes in production, a recording in tests), and its signed message must be
//! exactly the print the keeper chose before anything is sent.

use std::cell::RefCell;

use metalbook_keeper::receiver::{
    self, close_encoded_vaa, compute_unit_limit, post_instructions, AccumulatorUpdate, PostAccounts,
};
use metalbook_keeper::{Chain, HermesSource, HttpGet, KeeperError, PrintMsg};
use metalbook_sdk::{ix, Address};
use solana_keypair::Keypair;
use solana_signer::Signer;

use crate::{submit_signed, CliError, Rpc};

/// The Clock sysvar; `unix_timestamp` is at bytes 32..40.
const CLOCK: Address = Address::from_str_const("SysvarC1ock11111111111111111111111111111111");

/// Compute limit of each posting transaction (the receiver CLI asks 600,000; about 85,000 and
/// 38,000 are used, `docs/PLAN.md` M7b-2).
const POST_CU: u32 = 600_000;

/// Where the binary update of a print comes from.
pub trait UpdateSource {
    /// The Hermes binary update whose message for `feed` is published at `print.publish_time`.
    ///
    /// # Errors
    ///
    /// `KeeperError::Source` if it cannot be fetched.
    fn update(&self, feed: &[u8; 32], print: &PrintMsg) -> Result<Vec<u8>, KeeperError>;
}

/// By reference, so the keeper's price source and the chain share one cache (Hermes' timestamp
/// endpoint is rate limited, and posting a print needs exactly the update the keeper found).
impl<H: HttpGet> UpdateSource for &HermesSource<H> {
    fn update(&self, feed: &[u8; 32], print: &PrintMsg) -> Result<Vec<u8>, KeeperError> {
        let found = self
            .first_at_or_after(feed, print.publish_time)?
            .ok_or_else(|| KeeperError::Source("Hermes has no update at that time".into()))?;
        if found.print != *print {
            return Err(KeeperError::Source(
                "Hermes' update at that time is not the chosen print".into(),
            ));
        }
        Ok(found.update_data)
    }
}

fn chain_error(e: CliError) -> KeeperError {
    // `CliError`'s RPC messages are already redacted (`rpc::client_error`).
    KeeperError::Chain(e.to_string())
}

/// The keeper's chain for one metalbook deployment.
pub struct RpcChain<R, U> {
    rpc: RefCell<R>,
    updates: U,
    program_id: Address,
    payer: Keypair,
    /// Encoded VAAs of this payer that may be open: retried before every post (codex, #25:
    /// only the write authority can return their rent).
    orphans: Vec<Orphan>,
    /// How long an address never seen to exist is kept: its creating transaction may still
    /// land until its blockhash expires (codex, #26).
    landing_window: std::time::Duration,
}

/// An encoded VAA that may be open.
#[derive(Debug, Clone, Copy)]
struct Orphan {
    address: Address,
    /// When its creation was sent (or when it was found).
    since: std::time::Instant,
    /// Whether it was ever known to exist: once it is absent again, it was closed.
    seen: bool,
}

/// Default [`RpcChain::with_landing_window`]: a blockhash is valid for 150 blocks, about a
/// minute, so twice that leaves margin. This is a heuristic, not a proof that the transaction
/// expired (block times vary); the keeper binary's periodic orphan scan is the backstop for
/// anything that lands later still (codex, #29).
const LANDING_WINDOW: std::time::Duration = std::time::Duration::from_secs(120);

impl<R: Rpc, U: UpdateSource> RpcChain<R, U> {
    /// A chain over `rpc` for `program_id`, paying with `payer` (also the write authority of
    /// the temporary accounts, so their rent returns to it).
    pub fn new(rpc: R, updates: U, program_id: Address, payer: Keypair) -> Self {
        Self {
            rpc: RefCell::new(rpc),
            updates,
            program_id,
            payer,
            orphans: Vec::new(),
            landing_window: LANDING_WINDOW,
        }
    }

    /// Replaces how long an address never seen to exist is kept before it is dropped (default
    /// 120 seconds, past a blockhash's validity).
    #[must_use]
    pub fn with_landing_window(mut self, window: std::time::Duration) -> Self {
        self.landing_window = window;
        self
    }

    /// Adds encoded VAAs to close (for example found at startup,
    /// [`crate::rpc::Cluster::encoded_vaas_of`]).
    pub fn adopt_orphans(&mut self, orphans: impl IntoIterator<Item = Address>) {
        for address in orphans {
            if self.orphans.iter().all(|o| o.address != address) {
                self.orphans.push(Orphan {
                    address,
                    since: std::time::Instant::now(),
                    seen: true,
                });
            }
        }
    }

    /// The encoded VAAs still waiting to be closed.
    #[must_use]
    pub fn orphans(&self) -> Vec<Address> {
        self.orphans.iter().map(|o| o.address).collect()
    }

    /// Closes every known orphan that exists. The account is re-read rather than a send's
    /// result trusted, since a transaction can land while its confirmation fails (codex, #26):
    /// an orphan is dropped after a successful close, when it is absent after having existed,
    /// or when it never appeared within the landing window (its creation can no longer land);
    /// otherwise it is kept.
    pub fn close_orphans(&mut self) {
        let pending = std::mem::take(&mut self.orphans);
        for mut o in pending {
            match self.exists(&o.address) {
                Some(false) if o.seen || o.since.elapsed() >= self.landing_window => continue,
                Some(true) => o.seen = true,
                _ => {
                    self.orphans.push(o);
                    continue;
                }
            }
            let closed = self
                .send(&[close_encoded_vaa(&self.payer.pubkey(), &o.address)], &[])
                .is_ok();
            if !closed && self.exists(&o.address) != Some(false) {
                self.orphans.push(o);
            }
        }
    }

    /// Whether `account` exists; `None` if it cannot be read.
    fn exists(&self, account: &Address) -> Option<bool> {
        self.rpc
            .borrow_mut()
            .account(account)
            .ok()
            .map(|a| a.is_some())
    }

    /// The RPC, for reads outside the keeper.
    pub fn rpc(&self) -> std::cell::RefMut<'_, R> {
        self.rpc.borrow_mut()
    }

    fn send(
        &self,
        instructions: &[solana_instruction::Instruction],
        others: &[&Keypair],
    ) -> Result<String, KeeperError> {
        submit_signed(
            &mut *self.rpc.borrow_mut(),
            instructions,
            &self.payer,
            others,
        )
        .map_err(chain_error)
    }
}

impl<R: Rpc, U: UpdateSource> Chain for RpcChain<R, U> {
    type Address = Address;

    fn now(&self) -> Result<i64, KeeperError> {
        let clock = self
            .rpc
            .borrow_mut()
            .account(&CLOCK)
            .map_err(chain_error)?
            .ok_or_else(|| KeeperError::Chain("no Clock sysvar".into()))?;
        clock
            .data
            .get(32..40)
            .and_then(|b| b.try_into().ok())
            .map(i64::from_le_bytes)
            .ok_or_else(|| KeeperError::Chain("Clock sysvar too short".into()))
    }

    fn account_data(&self, account: &Address) -> Result<Option<Vec<u8>>, KeeperError> {
        self.rpc
            .borrow_mut()
            .account(account)
            .map(|a| a.map(|a| a.data))
            .map_err(chain_error)
    }

    fn post(
        &mut self,
        series: &Address,
        feed: &[u8; 32],
        print: &PrintMsg,
    ) -> Result<(), KeeperError> {
        self.close_orphans();
        let bytes = self.updates.update(feed, print)?;
        let update = AccumulatorUpdate::parse(&bytes)?;
        // Only the print the keeper chose may be posted.
        if update.for_feed(feed)?.print()? != *print {
            return Err(KeeperError::Update(
                "signed message differs from the chosen print",
            ));
        }
        let encoded_vaa = Keypair::new();
        let price_update = Keypair::new();
        let a = PostAccounts {
            payer: self.payer.pubkey(),
            encoded_vaa: encoded_vaa.pubkey(),
            price_update: price_update.pubkey(),
            encoded_vaa_rent: receiver::rent_exempt(receiver::encoded_vaa_size(&update)?)?,
        };
        let sample = ix::sample_settlement(&self.program_id, series, &price_update.pubkey());
        let p = post_instructions(&update, feed, &a, sample)?;
        let mut verify = vec![compute_unit_limit(POST_CU)];
        verify.extend(p.verify);
        let mut post = vec![compute_unit_limit(POST_CU)];
        post.extend(p.post_and_sample);
        // Recorded before anything is sent: either transaction may land even when its
        // confirmation fails, and only the account itself tells (codex, #26). Transaction 2
        // closes it; a failure re-reads it through `close_orphans`.
        self.orphans.push(Orphan {
            address: a.encoded_vaa,
            since: std::time::Instant::now(),
            seen: false,
        });
        let sent = self.send(&verify, &[&encoded_vaa]).and_then(|_| {
            // Confirmed: the account exists, so once it is absent it was closed.
            if let Some(o) = self.orphans.iter_mut().find(|o| o.address == a.encoded_vaa) {
                o.seen = true;
            }
            self.send(&post, &[&price_update])
        });
        match sent {
            Ok(_) => {
                self.orphans.retain(|o| o.address != a.encoded_vaa);
                Ok(())
            }
            Err(e) => {
                // Return the encoded VAA's rent now if it exists, or keep it for the next post;
                // the post's error is the one that matters.
                self.close_orphans();
                Err(e)
            }
        }
    }

    fn settle(&mut self, series: &Address) -> Result<(), KeeperError> {
        self.send(&[ix::settle(&self.program_id, series)], &[])
            .map(|_| ())
    }
}
