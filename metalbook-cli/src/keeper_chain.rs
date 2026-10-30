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
    /// Encoded VAAs of this payer left open by a failed post whose close also failed; retried
    /// before every post (codex, #25: only the write authority can return their rent).
    orphans: Vec<Address>,
}

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
        }
    }

    /// Adds encoded VAAs to close (for example found at startup,
    /// [`crate::rpc::Cluster::encoded_vaas_of`]).
    pub fn adopt_orphans(&mut self, orphans: impl IntoIterator<Item = Address>) {
        for o in orphans {
            if !self.orphans.contains(&o) {
                self.orphans.push(o);
            }
        }
    }

    /// The encoded VAAs still waiting to be closed.
    #[must_use]
    pub fn orphans(&self) -> &[Address] {
        &self.orphans
    }

    /// Closes every known orphan that still exists. The account is re-read rather than the
    /// send's result trusted: a transaction can land while its confirmation fails (codex, #26),
    /// so an orphan is dropped once it is gone and kept while it exists or cannot be read.
    pub fn close_orphans(&mut self) {
        let pending = std::mem::take(&mut self.orphans);
        for o in pending {
            if self.exists(&o) == Some(false) {
                continue;
            }
            let closed = self
                .send(&[close_encoded_vaa(&self.payer.pubkey(), &o)], &[])
                .is_ok();
            if !closed && self.exists(&o) != Some(false) {
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
        self.orphans.push(a.encoded_vaa);
        let sent = self
            .send(&verify, &[&encoded_vaa])
            .and_then(|_| self.send(&post, &[&price_update]));
        match sent {
            Ok(_) => {
                self.orphans.retain(|o| *o != a.encoded_vaa);
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
