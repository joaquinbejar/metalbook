//! The keeper's [`Chain`] over a `pyth_post::Transport` (milestone 7c): reads accounts and the
//! clock, posts a print through Wormhole and the Pyth receiver with `pyth_post::post` followed
//! by `sample_settlement`, and settles. The binary update of a print comes from an
//! [`UpdateSource`] (Hermes in production, a recording in tests), and its signed message must
//! be exactly the print the keeper chose before anything is sent.
//!
//! `pyth_post` closes a post's encoded VAA after any failed send, re-reading the chain rather
//! than trusting the send's result (a transaction can land while its confirmation fails). What
//! it cannot close yet it reports: [`PostError::Orphaned`] (still open) and
//! [`PostError::Uncertain`] (its creation may still land until a block height). The chain keeps
//! both and retries them before every post, until they are closed or can no longer appear.

use std::cell::RefCell;

use metalbook_keeper::{BatchError, Chain, HermesSource, HttpGet, KeeperError, PrintMsg};
use metalbook_sdk::{ix, Address};
use pyth_post::{
    close_if_open, post, post_plan, AccumulatorUpdate, PostAccounts, PostError, Transport,
};
use solana_instruction::Instruction;
use solana_keypair::Keypair;

use crate::metrics::legacy_tx_size;

/// The legacy transaction size limit, bytes (signatures included).
pub const MAX_TX_BYTES: u64 = 1_232;

/// The most series one batched post carries, whatever the size allows: `sample_settlement`
/// costs 2,621 CU (`docs/benchmarks.md`) and the post's last transaction runs under the
/// default limit of 200,000 CU per instruction, so compute never binds before this.
pub const MAX_BATCH: usize = 32;

/// Blocks a transaction can still land after its blockhash was fetched (150) plus a margin,
/// used when the transport cannot tell a failed send's last valid height.
const BLOCKHASH_LIFETIME_MARGIN: u64 = 300;

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

fn chain_error(e: &PostError) -> KeeperError {
    // `pyth_post` errors carry redacted text only (no URL, no key).
    KeeperError::Chain(e.to_string())
}

/// An encoded VAA of this payer that may be open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    /// Exists, or cannot be read: close it.
    Open(Address),
    /// Its creating transaction may still land until block height `valid_until`.
    InFlight { address: Address, valid_until: u64 },
}

impl Pending {
    fn address(&self) -> Address {
        match *self {
            Self::Open(a) | Self::InFlight { address: a, .. } => a,
        }
    }
}

/// The keeper's chain for one metalbook deployment.
pub struct RpcChain<T, U> {
    transport: RefCell<T>,
    updates: U,
    program_id: Address,
    payer: Keypair,
    pending: Vec<Pending>,
    /// Last valid block heights of posts whose outcome is unknown: one of their transactions
    /// may still land. No new post is sent until every one has expired (a payer-wide gate),
    /// so a candidate is never posted twice while an earlier post of it may still land.
    in_flight: Vec<u64>,
}

/// One `sample_settlement` per series, all reading `price_update`.
fn samples(program_id: &Address, series: &[Address], price_update: &Address) -> Vec<Instruction> {
    series
        .iter()
        .map(|s| ix::sample_settlement(program_id, s, price_update))
        .collect()
}

/// Serialized size of the last transaction of a post of `update` that samples `n` series
/// (`post_update`, the samples, `reclaim_rent`, `close_encoded_vaa`), signatures included. Fresh
/// addresses stand in for the series and the temporary accounts: their values do not change
/// the size.
///
/// # Errors
///
/// `KeeperError` if the post cannot be planned.
pub fn last_post_tx_size(
    program_id: &Address,
    payer: &Address,
    update: &AccumulatorUpdate,
    merkle: &pyth_post::MerkleUpdate,
    n: usize,
) -> Result<u64, KeeperError> {
    let stand_in = |i: usize| {
        let mut a = [0xA5_u8; 32];
        let le = u64::try_from(i).unwrap_or(u64::MAX).to_le_bytes();
        for (dst, src) in a.iter_mut().zip(le) {
            *dst = src;
        }
        Address::new_from_array(a)
    };
    let encoded_vaa = stand_in(usize::MAX);
    let price_update = stand_in(usize::MAX.saturating_sub(1));
    let series: Vec<Address> = (0..n).map(stand_in).collect();
    let steps = post_plan(
        &PostAccounts {
            payer,
            encoded_vaa: &encoded_vaa,
            price_update: &price_update,
            encoded_vaa_rent: 0,
            treasury_id: 0,
        },
        update,
        merkle,
        samples(program_id, &series, &price_update),
    )
    .map_err(|e| chain_error(&e))?;
    let last = steps.last().ok_or(KeeperError::Update("empty post plan"))?;
    Ok(legacy_tx_size(&last.instructions, payer))
}

impl<T: Transport, U: UpdateSource> RpcChain<T, U> {
    /// A chain over `transport` for `program_id`, paying with `payer` (also the write authority
    /// of the temporary accounts, so their rent returns to it). One poster per payer
    /// (`pyth_post::PayerLock` across processes; this chain is not shared across threads):
    /// another poster's orphan scan would see this one's in-flight accounts.
    pub fn new(transport: T, updates: U, program_id: Address, payer: Keypair) -> Self {
        Self {
            transport: RefCell::new(transport),
            updates,
            program_id,
            payer,
            pending: Vec::new(),
            in_flight: Vec::new(),
        }
    }

    /// The transport, for reads outside the keeper.
    pub fn transport(&self) -> std::cell::RefMut<'_, T> {
        self.transport.borrow_mut()
    }

    /// The payer's address.
    #[must_use]
    pub fn payer(&self) -> Address {
        solana_signer::Signer::pubkey(&self.payer)
    }

    /// Adds encoded VAAs to close (for example reported by `pyth_post::LeftoverScanner`).
    pub fn adopt_orphans(&mut self, orphans: impl IntoIterator<Item = Address>) {
        for address in orphans {
            if self.pending.iter().all(|p| p.address() != address) {
                self.pending.push(Pending::Open(address));
            }
        }
    }

    /// The encoded VAAs still waiting to be closed or to expire.
    #[must_use]
    pub fn orphans(&self) -> Vec<Address> {
        self.pending.iter().map(Pending::address).collect()
    }

    /// Refuses to post while an earlier post's outcome is unknown: every block height in
    /// `in_flight` (and every in-flight encoded VAA) must be in the past. An unreadable block
    /// height refuses too.
    fn posting_gate(&mut self) -> Result<(), BatchError> {
        let mut until: Vec<u64> = self.in_flight.clone();
        until.extend(self.pending.iter().filter_map(|p| match *p {
            Pending::InFlight { valid_until, .. } => Some(valid_until),
            Pending::Open(_) => None,
        }));
        if until.is_empty() {
            return Ok(());
        }
        let height = self
            .transport
            .borrow()
            .block_height()
            .map_err(|e| BatchError::Uncertain(chain_error(&e)))?;
        self.in_flight.retain(|v| height <= *v);
        match until.into_iter().filter(|v| height <= *v).max() {
            None => Ok(()),
            Some(last) => Err(BatchError::Uncertain(KeeperError::Chain(format!(
                "an earlier post may still land until block {last} (now {height}): not posting"
            )))),
        }
    }

    /// The last block height at which a transaction of a failed post can still land: the
    /// transport's last send expiry, or (if it cannot tell) a full blockhash lifetime and a
    /// margin from now.
    fn failed_post_horizon(&self) -> u64 {
        let t = self.transport.borrow();
        t.last_send_expiry().unwrap_or_else(|| {
            t.block_height()
                .map_or(u64::MAX, |h| h.saturating_add(BLOCKHASH_LIFETIME_MARGIN))
        })
    }

    /// Sends `settles` as one transaction.
    fn send_settles(&mut self, settles: &[Instruction]) -> Result<(), KeeperError> {
        self.transport
            .borrow_mut()
            .send(settles, &self.payer, None)
            .map(|_| ())
            .map_err(|e| chain_error(&e))
    }

    /// Closes what can be closed: an open orphan with `close_if_open`; an in-flight one once it
    /// appears, or drops it once its creation can no longer land.
    pub fn close_orphans(&mut self) {
        let pending = std::mem::take(&mut self.pending);
        for p in pending {
            let keep = match p {
                Pending::Open(a) => {
                    close_if_open(&mut *self.transport.borrow_mut(), &self.payer, &a)
                        .map(Pending::Open)
                }
                Pending::InFlight {
                    address,
                    valid_until,
                } => {
                    let appeared = self.transport.borrow().account(&address);
                    match appeared {
                        // It landed: close it like any open orphan.
                        Ok(Some(_)) => {
                            close_if_open(&mut *self.transport.borrow_mut(), &self.payer, &address)
                                .map(Pending::Open)
                        }
                        // Not there: it can still appear until its blockhash expires.
                        Ok(None) => {
                            let expired = self
                                .transport
                                .borrow()
                                .block_height()
                                .is_ok_and(|h| h > valid_until);
                            (!expired).then_some(p)
                        }
                        // Unknown: keep it.
                        Err(_) => Some(p),
                    }
                }
            };
            self.pending.extend(keep);
        }
    }
}

impl<T: Transport, U: UpdateSource> Chain for RpcChain<T, U> {
    type Address = Address;

    fn now(&self) -> Result<i64, KeeperError> {
        self.transport.borrow().now().map_err(|e| chain_error(&e))
    }

    fn account_data(&self, account: &Address) -> Result<Option<Vec<u8>>, KeeperError> {
        self.transport
            .borrow()
            .account(account)
            .map(|a| a.map(|(_, data)| data))
            .map_err(|e| chain_error(&e))
    }

    fn post(
        &mut self,
        series: &Address,
        feed: &[u8; 32],
        print: &PrintMsg,
    ) -> Result<(), KeeperError> {
        self.post_batch(std::slice::from_ref(series), feed, print)
            .map_err(|e| match e {
                BatchError::Uncertain(e) | BatchError::Failed(e) => e,
            })
    }

    fn batch_capacity(&self, feed: &[u8; 32], print: &PrintMsg) -> Result<usize, KeeperError> {
        let bytes = self.updates.update(feed, print)?;
        let update = AccumulatorUpdate::parse(&bytes).map_err(|e| chain_error(&e))?;
        let (merkle, _) = update
            .find(feed)
            .ok_or(KeeperError::Update("feed not in the update"))?;
        let payer = self.payer();
        let size = |n: usize| -> Result<u64, KeeperError> {
            last_post_tx_size(&self.program_id, &payer, &update, merkle, n)
        };
        let mut n = 1;
        while n < MAX_BATCH && size(n.saturating_add(1))? <= MAX_TX_BYTES {
            n = n.saturating_add(1);
        }
        Ok(n)
    }

    fn post_batch(
        &mut self,
        series: &[Address],
        feed: &[u8; 32],
        print: &PrintMsg,
    ) -> Result<(), BatchError> {
        self.close_orphans();
        self.posting_gate()?;
        let bytes = self
            .updates
            .update(feed, print)
            .map_err(BatchError::Failed)?;
        let update =
            AccumulatorUpdate::parse(&bytes).map_err(|e| BatchError::Failed(chain_error(&e)))?;
        let (merkle, message) =
            update
                .find(feed)
                .ok_or(BatchError::Failed(KeeperError::Update(
                    "feed not in the update",
                )))?;
        // Only the print the keeper chose may be posted.
        let signed = PrintMsg {
            price: message.price,
            conf: message.conf,
            expo: message.exponent,
            publish_time: message.publish_time,
            prev_publish_time: message.prev_publish_time,
        };
        if signed != *print {
            return Err(BatchError::Failed(KeeperError::Update(
                "signed message differs from the chosen print",
            )));
        }
        let program_id = self.program_id;
        let sent = post(
            &mut *self.transport.borrow_mut(),
            &self.payer,
            &update,
            merkle,
            0,
            |price_update| samples(&program_id, series, price_update),
        );
        // Any failure after sending began is uncertain: pyth-post re-reads the encoded VAA,
        // but a later transaction (the samples) whose confirmation was lost can still land
        // until its blockhash expires, and an RPC error cannot be told from a rejection. Only
        // planning errors (nothing was sent) are failures.
        match sent {
            Ok(_) => Ok(()),
            Err(e) => Err(match &e {
                PostError::Update(_) => BatchError::Failed(chain_error(&e)),
                PostError::Uncertain {
                    encoded_vaa,
                    valid_until,
                    ..
                } => {
                    self.pending.push(Pending::InFlight {
                        address: *encoded_vaa,
                        valid_until: *valid_until,
                    });
                    self.in_flight.push(*valid_until);
                    BatchError::Uncertain(chain_error(&e))
                }
                PostError::Orphaned { encoded_vaa, .. } => {
                    self.pending.push(Pending::Open(*encoded_vaa));
                    let horizon = self.failed_post_horizon();
                    self.in_flight.push(horizon);
                    BatchError::Uncertain(chain_error(&e))
                }
                _ => {
                    let horizon = self.failed_post_horizon();
                    self.in_flight.push(horizon);
                    BatchError::Uncertain(chain_error(&e))
                }
            }),
        }
    }

    fn settle_batch(&mut self, series: &[Address]) -> Result<(), KeeperError> {
        let payer = self.payer();
        let mut chunk: Vec<Instruction> = Vec::new();
        for s in series {
            let next = ix::settle(&self.program_id, s);
            let mut candidate = chunk.clone();
            candidate.push(next.clone());
            if !chunk.is_empty() && legacy_tx_size(&candidate, &payer) > MAX_TX_BYTES {
                self.send_settles(&chunk)?;
                chunk = vec![next];
            } else {
                chunk = candidate;
            }
        }
        if !chunk.is_empty() {
            self.send_settles(&chunk)?;
        }
        Ok(())
    }

    fn settle(&mut self, series: &Address) -> Result<(), KeeperError> {
        self.transport
            .borrow_mut()
            .send(&[ix::settle(&self.program_id, series)], &self.payer, None)
            .map(|_| ())
            .map_err(|e| chain_error(&e))
    }
}
