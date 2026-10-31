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

use metalbook_keeper::{Chain, HermesSource, HttpGet, KeeperError, PrintMsg};
use metalbook_sdk::{ix, Address};
use pyth_post::{close_if_open, post, AccumulatorUpdate, PostError, Transport};
use solana_keypair::Keypair;

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
        self.close_orphans();
        let bytes = self.updates.update(feed, print)?;
        let update = AccumulatorUpdate::parse(&bytes).map_err(|e| chain_error(&e))?;
        let (merkle, message) = update
            .find(feed)
            .ok_or(KeeperError::Update("feed not in the update"))?;
        // Only the print the keeper chose may be posted.
        let signed = PrintMsg {
            price: message.price,
            conf: message.conf,
            expo: message.exponent,
            publish_time: message.publish_time,
            prev_publish_time: message.prev_publish_time,
        };
        if signed != *print {
            return Err(KeeperError::Update(
                "signed message differs from the chosen print",
            ));
        }
        let program_id = self.program_id;
        let series = *series;
        let sent = post(
            &mut *self.transport.borrow_mut(),
            &self.payer,
            &update,
            merkle,
            0,
            |price_update| vec![ix::sample_settlement(&program_id, &series, price_update)],
        );
        match sent {
            Ok(_) => Ok(()),
            Err(e) => {
                match &e {
                    PostError::Orphaned { encoded_vaa, .. } => {
                        self.pending.push(Pending::Open(*encoded_vaa));
                    }
                    PostError::Uncertain {
                        encoded_vaa,
                        valid_until,
                        ..
                    } => self.pending.push(Pending::InFlight {
                        address: *encoded_vaa,
                        valid_until: *valid_until,
                    }),
                    _ => {}
                }
                Err(chain_error(&e))
            }
        }
    }

    fn settle(&mut self, series: &Address) -> Result<(), KeeperError> {
        self.transport
            .borrow_mut()
            .send(&[ix::settle(&self.program_id, series)], &self.payer, None)
            .map(|_| ())
            .map_err(|e| chain_error(&e))
    }
}
