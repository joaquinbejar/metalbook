//! Milestone 7b-2 and 7c in LiteSVM against the real programs, through `pyth_post` (the shared
//! posting crate that replaced `metalbook_keeper::receiver`): a guardian-signed Hermes update
//! for XAU/USD (recorded 2026-10-30, `fixtures/pyth/hermes_xau_update.json`) is verified by the
//! Wormhole core bridge, posted as a `Full` `PriceUpdateV2` by the Pyth receiver and sampled by
//! metalbook in the same transaction, which then returns both temporary accounts' rent; the
//! keeper drives it through `metalbook_cli::keeper_chain::RpcChain`. Kept as regression tests
//! after the migration, with the fault-injection cases (sends that land and then fail, or land
//! late).
//!
//! The receiver and Wormhole binaries are the devnet deployments (`make pyth-programs`, pinned
//! by sha256), with the receiver's config and guardian set 1 recorded from devnet. Run
//! `make program pyth-programs` first.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::integer_division,
    clippy::panic_in_result_fn,
    clippy::disallowed_macros,
    missing_docs
)]

mod common;

use std::path::PathBuf;

use base64::Engine;
use common::*;
use metalbook_cli::keeper_chain::{RpcChain, UpdateSource};
use metalbook_keeper::{Chain, Keeper, KeeperError, PrintMsg, SeriesView, TapeSource, TickReport};
use metalbook_sdk::ix;
use pyth_post::ix::{PYTH_RECEIVER, WORMHOLE_PROGRAM};
use pyth_post::{post, AccumulatorUpdate, PostError, Transport};
use serde_json::Value;
use solana_account::Account;
use solana_address::Address;
use solana_clock::Clock;
use solana_instruction::Instruction;
use solana_keypair::Keypair;
use solana_signer::Signer;
use solana_transaction::Transaction;

/// Encoded VAA account discriminator, `sha256("account:EncodedVaa")[..8]`.
const ENCODED_VAA: [u8; 8] = [226, 101, 163, 4, 133, 160, 84, 245];

fn fixture(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/pyth")
        .join(name);
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn b64(v: &Value) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(v.as_str().unwrap())
        .unwrap()
}

fn load_program(env: &mut Env, id: Address, file: &str) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../target/deploy")
        .join(file);
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("{}: {e}; run `make pyth-programs` first", path.display()));
    env.svm.add_program(id, &bytes).unwrap();
}

fn load_account(env: &mut Env, name: &str) {
    let f = fixture(name);
    let address: Address = f["address"].as_str().unwrap().parse().unwrap();
    let account = Account {
        lamports: f["lamports"].as_u64().unwrap(),
        data: b64(&f["data_base64"]),
        owner: f["owner"].as_str().unwrap().parse().unwrap(),
        executable: false,
        rent_epoch: 0,
    };
    env.svm.set_account(address, account).unwrap();
}

struct Recorded {
    update: Vec<u8>,
    price: i64,
    publish_time: i64,
    prev_publish_time: i64,
}

fn recorded() -> Recorded {
    let f = fixture("hermes_xau_update.json");
    assert_eq!(f["feed_id"].as_str().unwrap(), hex(&FEED_XAU));
    assert_eq!(f["expo"].as_i64().unwrap(), i64::from(EXPO_XAU));
    Recorded {
        update: b64(&f["binary_base64"]),
        price: f["price"].as_i64().unwrap(),
        publish_time: f["publish_time"].as_i64().unwrap(),
        prev_publish_time: f["prev_publish_time"].as_i64().unwrap(),
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// The real receiver and Wormhole with their devnet state, metalbook configured, and an XAU put
/// whose expiry is one hour after the recorded print (so the print is its fallback candidate).
fn setup(r: &Recorded) -> (Env, Address) {
    let mut env = Env::bare();
    load_program(&mut env, PYTH_RECEIVER, "pyth_receiver.so");
    load_program(&mut env, WORMHOLE_PROGRAM, "wormhole_core_bridge.so");
    load_account(&mut env, "receiver_config.json");
    load_account(&mut env, "guardian_set_1.json");
    // The fee treasury exists on devnet as a plain system account.
    env.svm
        .airdrop(&pyth_post::ix::treasury(0), 1_000_000_000)
        .unwrap();
    assert_eq!(
        pyth_post::ix::receiver_config().to_string(),
        fixture("receiver_config.json")["address"]
    );
    assert_eq!(
        pyth_post::ix::guardian_set(1).to_string(),
        fixture("guardian_set_1.json")["address"]
    );

    let expiry = r.publish_time + 3_600;
    env.set_time(r.publish_time - 7_200);
    let admin = env.admin.insecure_clone();
    env.send(
        &[env.ix_init_config(&admin.pubkey(), Params::default_params())],
        &[&admin],
    )
    .unwrap();
    env.send(
        &[env.ix_set_calendar(&admin.pubkey(), XAU, &[expiry])],
        &[&admin],
    )
    .unwrap();
    let args = SeriesArgs {
        expiry,
        ..SeriesArgs::xau_put(4_000)
    };
    let series = env.create_series(args);
    env.set_time(r.publish_time + 30);
    (env, series)
}

/// [`Transport`] over an owned [`Env`]. A send's blockhash is valid for 150 slots.
struct Svm {
    env: Env,
    last_expiry: Option<u64>,
    /// Expire the blockhash before each send (identical transactions would collide); off when
    /// held transactions must stay valid.
    fresh_blockhash: bool,
}

impl Svm {
    fn new(env: Env) -> Self {
        Self {
            env,
            last_expiry: None,
            fresh_blockhash: true,
        }
    }

    fn signed(
        &mut self,
        instructions: &[Instruction],
        payer: &Keypair,
        extra: Option<&Keypair>,
    ) -> Transaction {
        if self.fresh_blockhash {
            self.env.svm.expire_blockhash();
        }
        let mut signers = vec![payer];
        signers.extend(extra);
        let slot = self.env.svm.get_sysvar::<Clock>().slot;
        self.last_expiry = Some(slot + 150);
        Transaction::new_signed_with_payer(
            instructions,
            Some(&payer.pubkey()),
            &signers,
            self.env.svm.latest_blockhash(),
        )
    }

    fn deliver(&mut self, tx: &Transaction) -> Result<String, PostError> {
        self.env
            .svm
            .send_transaction(tx.clone())
            .map(|_| tx.signatures[0].to_string())
            .map_err(|e| PostError::Chain(format!("{:?} {:?}", e.err, e.meta.logs)))
    }
}

impl Transport for Svm {
    fn now(&self) -> Result<i64, PostError> {
        Ok(self.env.svm.get_sysvar::<Clock>().unix_timestamp)
    }

    fn account(&self, account: &Address) -> Result<Option<(Address, Vec<u8>)>, PostError> {
        Ok(self.env.svm.get_account(account).map(|a| (a.owner, a.data)))
    }

    fn block_height(&self) -> Result<u64, PostError> {
        Ok(self.env.svm.get_sysvar::<Clock>().slot)
    }

    fn last_send_expiry(&self) -> Option<u64> {
        self.last_expiry
    }

    fn program_accounts(
        &self,
        program: &Address,
        offset: usize,
        bytes: &[u8],
    ) -> Result<Vec<Address>, PostError> {
        use solana_account::ReadableAccount;
        Ok(self
            .env
            .svm
            .accounts_db()
            .inner
            .iter()
            .filter(|(_, a)| a.owner() == program && a.lamports() > 0)
            .filter(|(_, a)| a.data().get(offset..offset + bytes.len()) == Some(bytes))
            .map(|(k, _)| *k)
            .collect())
    }

    fn rent_exempt(&self, len: usize) -> Result<u64, PostError> {
        Ok(self.env.svm.minimum_balance_for_rent_exemption(len))
    }

    fn send(
        &mut self,
        instructions: &[Instruction],
        payer: &Keypair,
        extra: Option<&Keypair>,
    ) -> Result<String, PostError> {
        let tx = self.signed(instructions, payer, extra);
        self.deliver(&tx)
    }
}

/// [`Svm`] whose sends at the given indices land and then report an error, like an RPC whose
/// confirmation fails after the transaction was processed (codex, #26).
struct LandsThenFails {
    svm: Svm,
    sends: usize,
    faulty: Vec<usize>,
}

/// [`Svm`] whose sends at the given indices report an error without landing yet; the held
/// transactions land on [`LandsLate::deliver_held`] (a confirmation that fails while the
/// transaction is still in flight, codex #26).
struct LandsLate {
    svm: Svm,
    sends: usize,
    late: Vec<usize>,
    held: Vec<Transaction>,
}

impl LandsLate {
    fn deliver_held(&mut self) {
        for tx in std::mem::take(&mut self.held) {
            self.svm.deliver(&tx).unwrap();
        }
    }
}

macro_rules! delegate_reads {
    () => {
        fn now(&self) -> Result<i64, PostError> {
            self.svm.now()
        }
        fn account(&self, account: &Address) -> Result<Option<(Address, Vec<u8>)>, PostError> {
            self.svm.account(account)
        }
        fn block_height(&self) -> Result<u64, PostError> {
            self.svm.block_height()
        }
        fn last_send_expiry(&self) -> Option<u64> {
            self.svm.last_send_expiry()
        }
        fn program_accounts(
            &self,
            program: &Address,
            offset: usize,
            bytes: &[u8],
        ) -> Result<Vec<Address>, PostError> {
            self.svm.program_accounts(program, offset, bytes)
        }
        fn rent_exempt(&self, len: usize) -> Result<u64, PostError> {
            self.svm.rent_exempt(len)
        }
    };
}

impl Transport for LandsThenFails {
    delegate_reads!();
    fn send(
        &mut self,
        instructions: &[Instruction],
        payer: &Keypair,
        extra: Option<&Keypair>,
    ) -> Result<String, PostError> {
        let index = self.sends;
        self.sends += 1;
        let result = self.svm.send(instructions, payer, extra);
        if self.faulty.contains(&index) {
            result?;
            return Err(PostError::Chain("confirmation timed out".into()));
        }
        result
    }
}

impl Transport for LandsLate {
    delegate_reads!();
    fn send(
        &mut self,
        instructions: &[Instruction],
        payer: &Keypair,
        extra: Option<&Keypair>,
    ) -> Result<String, PostError> {
        let index = self.sends;
        self.sends += 1;
        let tx = self.svm.signed(instructions, payer, extra);
        if self.late.contains(&index) {
            self.held.push(tx);
            return Err(PostError::Chain("confirmation timed out".into()));
        }
        self.svm.deliver(&tx)
    }
}

/// Serves the recorded binary update for its own print only, as Hermes would.
struct Recording(Vec<u8>, PrintMsg);

impl UpdateSource for Recording {
    fn update(&self, feed: &[u8; 32], print: &PrintMsg) -> Result<Vec<u8>, KeeperError> {
        assert_eq!(feed, &FEED_XAU);
        if print.publish_time == self.1.publish_time {
            Ok(self.0.clone())
        } else {
            Err(KeeperError::Source("no update".into()))
        }
    }
}

fn recorded_print(r: &Recorded) -> PrintMsg {
    let update = AccumulatorUpdate::parse(&r.update).unwrap();
    let (_, m) = update.find(&FEED_XAU).unwrap();
    let print = PrintMsg {
        price: m.price,
        conf: m.conf,
        expo: m.exponent,
        publish_time: m.publish_time,
        prev_publish_time: m.prev_publish_time,
    };
    assert_eq!(print.price, r.price);
    assert_eq!(print.publish_time, r.publish_time);
    assert_eq!(print.prev_publish_time, r.prev_publish_time);
    print
}

/// Live encoded VAA accounts in the VM.
fn encoded_vaas(env: &Env) -> usize {
    use solana_account::ReadableAccount;
    env.svm
        .accounts_db()
        .inner
        .iter()
        .filter(|(_, a)| a.owner() == &WORMHOLE_PROGRAM && a.lamports() > 0)
        .filter(|(_, a)| a.data().get(..8) == Some(&ENCODED_VAA[..]))
        .count()
}

fn fallback_time(env: &Env, series: &Address) -> i64 {
    SeriesView::decode(&env.account(series).data)
        .unwrap()
        .candidates
        .parts()
        .fallback_time
}

/// The three transactions of a post of this 292-byte VAA: create, init and write the first
/// chunk (2 signatures); write the rest and verify (1); post, sample and close (2).
const POST_SIGNATURES: u64 = 5;

#[test]
fn test_a_recorded_hermes_update_is_verified_posted_and_sampled() {
    let r = recorded();
    let update = AccumulatorUpdate::parse(&r.update).unwrap();
    assert_eq!(update.guardian_set_index().unwrap(), 1);
    assert!(update.find(&FEED_XAG).is_none());
    let (merkle, _) = update.find(&FEED_XAU).unwrap();

    let (mut env, series) = setup(&r);
    let payer = env.funded();
    let before = env.svm.get_balance(&payer.pubkey()).unwrap();
    let mut svm = Svm::new(env);
    let signatures = post(&mut svm, &payer, &update, merkle, 0, |pu| {
        vec![ix::sample_settlement(&PROGRAM_ID, &series, pu)]
    })
    .unwrap();
    assert_eq!(signatures.len(), 3);
    let env = &svm.env;
    // metalbook stored the print as the series' fallback candidate.
    let parts = SeriesView::decode(&env.account(&series).data)
        .unwrap()
        .candidates
        .parts();
    assert_eq!(parts.fallback_time, r.publish_time);
    assert_eq!(parts.fallback_price, u64::try_from(r.price).unwrap());
    // Both temporary accounts are gone and their rent is back: only signatures were paid.
    assert_eq!(encoded_vaas(env), 0);
    assert_eq!(
        before - env.svm.get_balance(&payer.pubkey()).unwrap(),
        POST_SIGNATURES * 5_000
    );
}

#[test]
fn test_tampered_updates_are_rejected_by_wormhole_or_the_receiver() {
    let r = recorded();
    let update = AccumulatorUpdate::parse(&r.update).unwrap();

    // A changed guardian signature: Wormhole's verification fails, nothing reaches the series,
    // and the encoded VAA is closed.
    let (mut env, series) = setup(&r);
    let payer = env.funded();
    let mut bad_sig = update.clone();
    bad_sig.vaa[10] ^= 1;
    let (merkle, _) = bad_sig.find(&FEED_XAU).unwrap();
    let mut svm = Svm::new(env);
    let e = post(&mut svm, &payer, &bad_sig, merkle, 0, |pu| {
        vec![ix::sample_settlement(&PROGRAM_ID, &series, pu)]
    })
    .unwrap_err();
    assert!(
        !matches!(e, PostError::Orphaned { .. } | PostError::Uncertain { .. }),
        "{e}"
    );
    assert_eq!(fallback_time(&svm.env, &series), 0);
    assert_eq!(encoded_vaas(&svm.env), 0);

    // A changed price in the message: the VAA verifies but the Merkle proof does not
    // (the receiver's InvalidPriceUpdate, 6002).
    let (mut env, series) = setup(&r);
    let payer = env.funded();
    let mut bad_price = update.clone();
    bad_price.updates[0].message[33 + 7] ^= 1;
    let merkle = bad_price.updates[0].clone();
    let mut svm = Svm::new(env);
    let e = post(&mut svm, &payer, &bad_price, &merkle, 0, |pu| {
        vec![ix::sample_settlement(&PROGRAM_ID, &series, pu)]
    })
    .unwrap_err();
    assert!(
        e.to_string().contains("6002") || e.to_string().contains("InvalidPriceUpdate"),
        "{e}"
    );
    assert_eq!(fallback_time(&svm.env, &series), 0);
    assert_eq!(encoded_vaas(&svm.env), 0);
}

fn chain<T: Transport>(
    t: T,
    r: &Recorded,
    print: PrintMsg,
    payer: &Keypair,
) -> RpcChain<T, Recording> {
    RpcChain::new(
        t,
        Recording(r.update.clone(), print),
        PROGRAM_ID,
        payer.insecure_clone(),
    )
}

#[test]
fn test_the_rpc_chain_posts_through_the_receiver_for_the_keeper() {
    let r = recorded();
    let print = recorded_print(&r);
    let (mut env, series) = setup(&r);
    // Past expiry (one hour after the print): the keeper seeks the fallback from expiry on.
    env.set_time(r.publish_time + 3_610);
    let payer = env.funded();
    let before = env.svm.get_balance(&payer.pubkey()).unwrap();
    let mut chain = chain(Svm::new(env), &r, print, &payer);
    let tape = TapeSource::new(FEED_XAU, vec![print]);
    let report = Keeper.tick(&mut chain, &tape, &series).unwrap();
    assert!(report.fallback, "{report:?}");
    let env = &chain.transport().env;
    assert_eq!(fallback_time(env, &series), r.publish_time);
    assert_eq!(
        before - env.svm.get_balance(&payer.pubkey()).unwrap(),
        POST_SIGNATURES * 5_000
    );
}

#[test]
fn test_the_rpc_chain_refuses_an_update_that_is_not_the_chosen_print() {
    let r = recorded();
    let signed = recorded_print(&r);
    let (mut env, series) = setup(&r);
    env.set_time(r.publish_time + 3_610);
    let payer = env.funded();
    let before = env.svm.get_balance(&payer.pubkey()).unwrap();
    // The keeper chose a print with another price at the same time; the update source still
    // returns the signed one, which must not be posted in its place.
    let chosen = PrintMsg {
        price: signed.price + 1,
        ..signed
    };
    let mut chain = chain(Svm::new(env), &r, chosen, &payer);
    let tape = TapeSource::new(FEED_XAU, vec![chosen]);
    let err = Keeper.tick(&mut chain, &tape, &series).unwrap_err();
    assert!(err.to_string().contains("differs"), "{err}");
    let env = &chain.transport().env;
    assert_eq!(
        env.svm.get_balance(&payer.pubkey()).unwrap(),
        before,
        "nothing was sent"
    );
    assert_eq!(fallback_time(env, &series), 0);
}

#[test]
fn test_the_keeper_posts_no_fallback_before_expiry() {
    let r = recorded();
    let print = recorded_print(&r);
    let (mut env, series) = setup(&r); // the clock is 30 s after the print, before expiry
    let payer = env.funded();
    let before = env.svm.get_balance(&payer.pubkey()).unwrap();
    let mut chain = chain(Svm::new(env), &r, print, &payer);
    let tape = TapeSource::new(FEED_XAU, vec![print]);
    assert_eq!(
        Keeper.tick(&mut chain, &tape, &series).unwrap(),
        TickReport::default()
    );
    assert_eq!(
        chain
            .transport()
            .env
            .svm
            .get_balance(&payer.pubkey())
            .unwrap(),
        before
    );
}

#[test]
fn test_a_failed_post_closes_its_verified_encoded_vaa() {
    let r = recorded();
    let print = recorded_print(&r);
    let (mut env, _series) = setup(&r);
    let payer = env.funded();
    let before = env.svm.get_balance(&payer.pubkey()).unwrap();
    let mut chain = chain(Svm::new(env), &r, print, &payer);
    // Not a series: the VAA verifies, the last transaction fails in sample_settlement.
    assert!(chain.post(&config_address(), &FEED_XAU, &print).is_err());
    assert!(chain.orphans().is_empty());
    let env = &chain.transport().env;
    assert_eq!(encoded_vaas(env), 0);
    // The post's 5 signatures and the close's 1; no rent lost.
    assert_eq!(
        before - env.svm.get_balance(&payer.pubkey()).unwrap(),
        (POST_SIGNATURES + 1) * 5_000
    );
}

fn faulty_chain(
    r: &Recorded,
    print: PrintMsg,
    faulty: Vec<usize>,
    after_expiry: bool,
) -> (RpcChain<LandsThenFails, Recording>, Address, Keypair, u64) {
    let (mut env, series) = setup(r);
    if after_expiry {
        env.set_time(r.publish_time + 3_610);
    }
    let payer = env.funded();
    let before = env.svm.get_balance(&payer.pubkey()).unwrap();
    let t = LandsThenFails {
        svm: Svm::new(env),
        sends: 0,
        faulty,
    };
    (chain(t, r, print, &payer), series, payer, before)
}

#[test]
fn test_a_create_that_lands_but_reports_an_error_is_closed() {
    let r = recorded();
    let print = recorded_print(&r);
    // Send 0 (create, init, write) lands, then fails: the encoded VAA must not stay behind.
    let (mut chain, series, payer, before) = faulty_chain(&r, print, vec![0], false);
    assert!(chain.post(&series, &FEED_XAU, &print).is_err());
    assert!(chain.orphans().is_empty());
    let env = &chain.transport().svm.env;
    assert_eq!(encoded_vaas(env), 0);
    // The first transaction (2 signatures) and the close (1).
    assert_eq!(
        before - env.svm.get_balance(&payer.pubkey()).unwrap(),
        3 * 5_000
    );
    assert_eq!(fallback_time(env, &series), 0);
}

#[test]
fn test_a_post_that_lands_but_reports_an_error_leaves_no_orphan() {
    let r = recorded();
    let print = recorded_print(&r);
    // Send 2 (post, sample, closes) lands, then fails: the print is on the series and the
    // transaction itself closed the encoded VAA, so nothing is queued.
    let (mut chain, series, payer, before) = faulty_chain(&r, print, vec![2], false);
    assert!(chain.post(&series, &FEED_XAU, &print).is_err());
    assert!(chain.orphans().is_empty());
    let env = &chain.transport().svm.env;
    assert_eq!(encoded_vaas(env), 0);
    assert_eq!(
        before - env.svm.get_balance(&payer.pubkey()).unwrap(),
        POST_SIGNATURES * 5_000
    );
    assert_eq!(fallback_time(env, &series), r.publish_time);
}

#[test]
fn test_a_close_that_lands_but_reports_an_error_is_not_retried_forever() {
    let r = recorded();
    let print = recorded_print(&r);
    // The last transaction fails for real (not a series), then send 3 (the close) lands and
    // fails: the account is gone, so nothing stays queued and nothing is resent.
    let (mut chain, _series, _payer, _before) = faulty_chain(&r, print, vec![3], false);
    assert!(chain.post(&config_address(), &FEED_XAU, &print).is_err());
    assert!(chain.orphans().is_empty());
    let sends = chain.transport().sends;
    chain.close_orphans();
    assert_eq!(chain.transport().sends, sends, "no further close was sent");
    assert_eq!(encoded_vaas(&chain.transport().svm.env), 0);
}

fn late_chain(
    r: &Recorded,
    print: PrintMsg,
) -> (RpcChain<LandsLate, Recording>, Address, Keypair, u64) {
    let (mut env, series) = setup(r);
    let payer = env.funded();
    let before = env.svm.get_balance(&payer.pubkey()).unwrap();
    let mut svm = Svm::new(env);
    // Held transactions must keep a valid blockhash until they are delivered.
    svm.fresh_blockhash = false;
    let t = LandsLate {
        svm,
        sends: 0,
        late: vec![0],
        held: Vec::new(),
    };
    (chain(t, r, print, &payer), series, payer, before)
}

#[test]
fn test_a_create_that_lands_late_is_kept_then_closed() {
    let r = recorded();
    let print = recorded_print(&r);
    let (mut chain, series, payer, before) = late_chain(&r, print);
    // The first transaction is still in flight when its confirmation fails: pyth_post reports
    // it as uncertain, and the chain keeps the address.
    let e = chain.post(&series, &FEED_XAU, &print).unwrap_err();
    assert!(e.to_string().contains("may still be created"), "{e}");
    assert_eq!(chain.orphans().len(), 1);
    chain.close_orphans();
    assert_eq!(
        chain.orphans().len(),
        1,
        "still absent and within its blockhash: kept"
    );
    // It lands: the next cleanup closes it.
    chain.transport().deliver_held();
    assert_eq!(encoded_vaas(&chain.transport().svm.env), 1);
    chain.close_orphans();
    assert!(chain.orphans().is_empty());
    let env = &chain.transport().svm.env;
    assert_eq!(encoded_vaas(env), 0);
    // The late transaction (2 signatures) and the close (1): no rent lost.
    assert_eq!(
        before - env.svm.get_balance(&payer.pubkey()).unwrap(),
        3 * 5_000
    );
}

#[test]
fn test_an_address_that_never_lands_is_dropped_once_its_blockhash_expires() {
    let r = recorded();
    let print = recorded_print(&r);
    let (mut chain, series, _payer, _before) = late_chain(&r, print);
    assert!(chain.post(&series, &FEED_XAU, &print).is_err());
    assert_eq!(chain.orphans().len(), 1);
    // Past the blockhash's last valid height, the creation can no longer land: dropped.
    let slot = chain.transport().svm.env.svm.get_sysvar::<Clock>().slot;
    chain.transport().svm.env.svm.warp_to_slot(slot + 1_000);
    chain.close_orphans();
    assert!(chain.orphans().is_empty());
}
