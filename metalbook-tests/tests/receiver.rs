//! Milestone 7b-2 in LiteSVM against the real programs: a guardian-signed Hermes update for
//! XAU/USD (recorded 2026-10-30, `fixtures/pyth/hermes_xau_update.json`) is written to an
//! encoded VAA and verified by the Wormhole core bridge, posted as a `Full` `PriceUpdateV2` by
//! the Pyth receiver, and sampled by metalbook in the same transaction, which then returns both
//! temporary accounts' rent. Every instruction comes from `metalbook_keeper::receiver`.
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
use metalbook_keeper::receiver::{
    self, compute_unit_limit, post_instructions, AccumulatorUpdate, PostAccounts, RECEIVER,
    WORMHOLE,
};
use metalbook_keeper::SeriesView;
use metalbook_sdk::ix;
use serde_json::Value;
use solana_account::Account;
use solana_address::Address;
use solana_keypair::Keypair;
use solana_signer::Signer;

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
    load_program(&mut env, RECEIVER, "pyth_receiver.so");
    load_program(&mut env, WORMHOLE, "wormhole_core_bridge.so");
    load_account(&mut env, "receiver_config.json");
    load_account(&mut env, "guardian_set_1.json");
    // The fee treasury exists on devnet as a plain system account.
    env.svm
        .airdrop(&receiver::treasury(), 1_000_000_000)
        .unwrap();
    assert_eq!(
        receiver::receiver_config().to_string(),
        fixture("receiver_config.json")["address"]
    );
    assert_eq!(
        receiver::guardian_set(1).to_string(),
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

/// The two transactions of one post, signed with fresh temporary keypairs.
fn post(
    env: &mut Env,
    payer: &Keypair,
    update: &AccumulatorUpdate,
    series: &Address,
) -> (Sent, Sent, Address, Address) {
    let encoded_vaa = Keypair::new();
    let price_update = Keypair::new();
    let size = receiver::encoded_vaa_size(update).unwrap();
    let a = PostAccounts {
        payer: payer.pubkey(),
        encoded_vaa: encoded_vaa.pubkey(),
        price_update: price_update.pubkey(),
        encoded_vaa_rent: env.svm.minimum_balance_for_rent_exemption(size),
    };
    let sample = ix::sample_settlement(&PROGRAM_ID, series, &price_update.pubkey());
    let p = post_instructions(update, &FEED_XAU, &a, sample).unwrap();
    let mut verify = vec![compute_unit_limit(600_000)];
    verify.extend(p.verify);
    let first = env.send(&verify, &[payer, &encoded_vaa]);
    let mut second = vec![compute_unit_limit(600_000)];
    second.extend(p.post_and_sample);
    let second = env.send(&second, &[payer, &price_update]);
    (first, second, encoded_vaa.pubkey(), price_update.pubkey())
}

#[test]
fn test_a_recorded_hermes_update_is_verified_posted_and_sampled() {
    let r = recorded();
    let update = AccumulatorUpdate::parse(&r.update).unwrap();
    assert_eq!(update.guardian_set_index().unwrap(), 1);
    assert!(update.for_feed(&FEED_XAU).is_ok());
    assert!(update.for_feed(&FEED_XAG).is_err());

    let (mut env, series) = setup(&r);
    let payer = env.funded();
    let before = env.svm.get_balance(&payer.pubkey()).unwrap();
    let (first, second, encoded_vaa, price_update) = post(&mut env, &payer, &update, &series);
    let first_cu = first.unwrap_or_else(|(e, logs)| panic!("verify: {e}\n{}", logs.join("\n")));
    let second_cu =
        second.unwrap_or_else(|(e, logs)| panic!("post and sample: {e}\n{}", logs.join("\n")));
    println!("CU: verify {first_cu}, post + sample + closes {second_cu}");

    // metalbook stored the print as the series' fallback candidate.
    let view = SeriesView::decode(&env.account(&series).data).unwrap();
    let parts = view.candidates.parts();
    assert_eq!(parts.fallback_time, r.publish_time);
    assert_eq!(parts.fallback_price, u64::try_from(r.price).unwrap());
    assert!(r.prev_publish_time < r.publish_time);

    // Both temporary accounts are gone and their rent is back: the payer only paid the
    // signature fees (2 + 2 signatures at 5,000 lamports).
    assert!(env
        .svm
        .get_account(&encoded_vaa)
        .is_none_or(|a| a.lamports == 0));
    assert!(env
        .svm
        .get_account(&price_update)
        .is_none_or(|a| a.lamports == 0));
    let after = env.svm.get_balance(&payer.pubkey()).unwrap();
    assert_eq!(before - after, 4 * 5_000);
}

#[test]
fn test_tampered_updates_are_rejected_by_wormhole_or_the_receiver() {
    let r = recorded();
    let update = AccumulatorUpdate::parse(&r.update).unwrap();

    // A changed guardian signature: Wormhole's verification fails, nothing is posted.
    let (mut env, series) = setup(&r);
    let payer = env.funded();
    let mut bad_sig = update.clone();
    bad_sig.vaa[10] ^= 1;
    let (first, second, _, _) = post(&mut env, &payer, &bad_sig, &series);
    let (e, logs) = first.unwrap_err();
    // `verify_encoded_vaa_v1` is instruction 4 (budget, create, init, write, verify).
    assert!(
        e.contains("InstructionError(4,"),
        "{e}: {}",
        logs.join("\n")
    );
    assert!(second.is_err());
    let view = SeriesView::decode(&env.account(&series).data).unwrap();
    assert_eq!(view.candidates.parts().fallback_time, 0);

    // A changed price in the message: the VAA verifies but the Merkle proof does not.
    let (mut env, series) = setup(&r);
    let payer = env.funded();
    let mut bad_price = update.clone();
    let m = &mut bad_price.updates[0].message;
    m[33 + 7] ^= 1; // low byte of the big-endian price after the type byte and feed id
    let (first, second, _, _) = post(&mut env, &payer, &bad_price, &series);
    first.unwrap();
    let (e, logs) = second.unwrap_err();
    // `post_update` is instruction 1 (after the compute budget): the receiver's
    // `InvalidPriceUpdate` (6002), the Merkle proof check.
    assert!(e.contains("InstructionError(1, Custom(6002))"), "{e}");
    assert!(logs.iter().any(|l| l.contains("InvalidPriceUpdate")));
    let view = SeriesView::decode(&env.account(&series).data).unwrap();
    assert_eq!(view.candidates.parts().fallback_time, 0);
}
