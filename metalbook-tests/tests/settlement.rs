//! LiteSVM tests of milestone 3: `sample_settlement` and `settle` (`docs/PLAN.md` 4.2, 4.3).
//! Prints are synthetic `PriceUpdateV2` accounts (no Hermes key yet), plus one real XAU/USD
//! account captured from mainnet by varswap (`fixtures/xau_usd.*`). Run `make program`
//! first.

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

use common::*;
use solana_address::Address;
use solana_signer::Signer;

const WS: i64 = EXPIRY - WINDOW_S;
const HARD: i64 = EXPIRY + HARD_S;

/// Settle methods, restated.
const WINDOW: u8 = 0;
const NEXT_PRINT: u8 = 1;
const PARTIAL_NEXT_PRINT: u8 = 2;
const PARTIAL_WINDOW: u8 = 3;
const FALLBACK: u8 = 4;
const VOID: u8 = 5;

/// States, restated.
const ACTIVE: u8 = 0;
const EXPIRED: u8 = 1;
const SETTLED: u8 = 2;
const VOIDED: u8 = 3;

fn boundary(i: i64) -> i64 {
    WS + i * SLOT_S
}

/// A series with one writer holding 2 contracts.
fn setup(args: SeriesArgs) -> (Env, Address) {
    let mut env = Env::new();
    let series = env.create_series(args);
    let w = env.writer(100_000);
    env.long_account(&w.pubkey(), args);
    env.send(&[env.ix_mint(&w.pubkey(), args, 2)], &[&w])
        .unwrap();
    (env, series)
}

/// Posts the canonical print of slot `i` at `price` (XAU, exponent -3).
fn post_slot(env: &mut Env, series: &Address, i: i64, price: i64) -> Sent {
    env.post(series, price, boundary(i) + 1, boundary(i) - 1)
}

#[test]
fn test_full_window_settles_at_expiry_with_mean_price() {
    let (mut env, series) = setup(SeriesArgs::xau_put(4_000));
    // Live sampling during the window: the series stays Active.
    for i in 0..30 {
        env.set_time(boundary(i) + 2);
        post_slot(&mut env, &series, i, 3_900_000 + i).unwrap();
    }
    let v = read_settlement(&env, &series);
    assert_eq!((v.state, v.filled_slots), (ACTIVE, u32::MAX >> 2));
    expect_code(env.settle(&series), code::SERIES_NOT_EXPIRED);
    env.set_time(EXPIRY);
    env.settle(&series).unwrap();
    let v = read_settlement(&env, &series);
    // Mean of 3_900_000 + 0..29 at 10^-3 = 3900.0145 USD = 3_900_014_500 atoms.
    assert_eq!(
        (v.state, v.method, v.price, v.samples),
        (SETTLED, WINDOW, 3_900_014_500, 30)
    );
    // Put K = 4000, 1 oz: payout 99.9855 USD rounded down to the atom.
    assert_eq!(v.payout, 99_985_500);
    assert_eq!(v.payout + v.residual, 4_000 * USD);
    expect_code(env.settle(&series), code::SERIES_ALREADY_SETTLED);
    expect_code(
        post_slot(&mut env, &series, 0, 1),
        code::SERIES_ALREADY_SETTLED,
    );
}

#[test]
fn test_min_samples_window_waits_for_the_challenge_delay() {
    let (mut env, series) = setup(SeriesArgs::xau_put(4_000));
    env.set_time(EXPIRY + 100);
    for i in 0..20 {
        post_slot(&mut env, &series, i, 4_000_000).unwrap();
    }
    // The first print after expiry moved the series to Expired.
    assert_eq!(read_settlement(&env, &series).state, EXPIRED);
    env.set_time(EXPIRY + 100 + D - 1);
    expect_code(env.settle(&series), code::SETTLEMENT_NOT_READY);
    env.set_time(EXPIRY + 100 + D);
    env.settle(&series).unwrap();
    let v = read_settlement(&env, &series);
    assert_eq!((v.method, v.price, v.samples), (WINDOW, 4_000_000_000, 20));
}

#[test]
fn test_sampling_and_settling_in_one_transaction_fails_for_a_subset() {
    let (mut env, series) = setup(SeriesArgs::xau_put(4_000));
    let admin = env.admin.insecure_clone();
    env.set_time(EXPIRY + 3_600);
    for i in 0..20 {
        post_slot(&mut env, &series, i, 4_000_000).unwrap();
    }
    // D later the 20-slot window is eligible on its own (checked by simulation only)...
    env.set_time(EXPIRY + 3_600 + D);
    let tx = solana_transaction::Transaction::new_signed_with_payer(
        &[env.ix_settle(&series)],
        Some(&admin.pubkey()),
        &[&admin],
        env.svm.latest_blockhash(),
    );
    assert!(env.svm.simulate_transaction(tx).is_ok());
    // ...but adding a slot and settling in the same transaction restarts D: a pusher cannot
    // pick a subset and settle on it at once.
    let pu = env.price_update(PriceUpdateArgs::xau(
        4_400_000,
        boundary(20) + 1,
        boundary(20) - 1,
    ));
    let ixs = [env.ix_sample(&series, &pu), env.ix_settle(&series)];
    expect_code(env.send(&ixs, &[&admin]), code::SETTLEMENT_NOT_READY);
    // Completing the window in the same transaction is allowed: there is no subset to pick.
    for i in 20..29 {
        post_slot(&mut env, &series, i, 4_000_000).unwrap();
    }
    let pu = env.price_update(PriceUpdateArgs::xau(
        4_100_000,
        boundary(29) + 1,
        boundary(29) - 1,
    ));
    let ixs = [env.ix_sample(&series, &pu), env.ix_settle(&series)];
    env.send(&ixs, &[&admin]).unwrap();
    let v = read_settlement(&env, &series);
    assert_eq!((v.method, v.samples), (WINDOW, 30));
}

#[test]
fn test_slot_rules_canonical_tolerance_and_gap() {
    let (mut env, series) = setup(SeriesArgs::xau_put(4_000));
    env.set_time(EXPIRY + 10);
    let b = boundary(5);
    // Not crossing the boundary, or too late after it: no slot (each still becomes the
    // fallback candidate, so the posts succeed; check the slot bitmap).
    env.post(&series, 4_000_000, b + 3, b).unwrap();
    env.post(&series, 4_000_000, b + TOLERANCE_S + 1, b - 1)
        .unwrap();
    assert_eq!(read_settlement(&env, &series).filled_slots, 0);
    // Two valid prints for the same slot cannot both be canonical: the canonical one fills it,
    // a second identical post is rejected.
    env.post(&series, 4_000_000, b + TOLERANCE_S, b - 1)
        .unwrap();
    assert_eq!(read_settlement(&env, &series).filled_slots, 1 << 5);
    expect_code(
        env.post(&series, 4_000_000, b + TOLERANCE_S, b - 1),
        code::SAMPLE_OUTSIDE_WINDOW,
    );
    // A print after a 20-minute gap fills at most one slot.
    let late = boundary(28) + 3;
    env.post(&series, 4_100_000, late, late - 1_200).unwrap();
    assert_eq!(
        read_settlement(&env, &series).filled_slots,
        (1 << 5) | (1 << 28)
    );
}

#[test]
fn test_slot_canonical_print_wins_in_both_orders() {
    for canonical_first in [true, false] {
        let (mut env, series) = setup(SeriesArgs::xau_put(4_000));
        env.set_time(EXPIRY + 10);
        let b = boundary(7);
        // The canonical print, and a same-second later aggregate (prev == publish) with
        // another price: only the canonical one can fill slot 7.
        let canonical = (4_000_000, b + 2, b - 1);
        let repeat = (4_200_000, b + 2, b + 2);
        let order = if canonical_first {
            [canonical, repeat]
        } else {
            [repeat, canonical]
        };
        for (px, publish, prev) in order {
            let _ = env.post(&series, px, publish, prev);
        }
        let mut sum = [0u8; 16];
        sum.copy_from_slice(&env.account(&series).data[312..328]);
        assert_eq!(
            u128::from_le_bytes(sum),
            4_000_000,
            "canonical_first = {canonical_first}"
        );
    }
}

#[test]
fn test_next_print_chain_settles_and_rejects_non_advancing_links() {
    let (mut env, series) = setup(SeriesArgs::xau_put(4_000));
    env.set_time(EXPIRY + 60);
    env.post(&series, 4_010_000, EXPIRY + 1, EXPIRY - 2)
        .unwrap();
    // A non-advancing repeat of the first link.
    expect_code(
        env.post(&series, 4_500_000, EXPIRY + 1, EXPIRY + 1),
        code::SAMPLE_OUTSIDE_WINDOW,
    );
    // Same publish time, two messages: only the one chained on the last link.
    expect_code(
        env.post(&series, 4_500_000, EXPIRY + 2, EXPIRY + 2),
        code::SAMPLE_OUTSIDE_WINDOW,
    );
    env.post(&series, 4_020_000, EXPIRY + 2, EXPIRY + 1)
        .unwrap();
    for (j, t) in [(3, EXPIRY + 4), (4, EXPIRY + 5), (5, EXPIRY + 7)] {
        let prev = [EXPIRY + 2, EXPIRY + 4, EXPIRY + 5][j - 3];
        env.post(&series, 4_000_000 + j as i64 * 10_000, t, prev)
            .unwrap();
    }
    assert_eq!(read_settlement(&env, &series).next_len, 5);
    // A complete chain waits D after window_end so the window can be backfilled first.
    expect_code(env.settle(&series), code::SETTLEMENT_NOT_READY);
    env.set_time(EXPIRY + D);
    env.settle(&series).unwrap();
    let v = read_settlement(&env, &series);
    // Mean of 4010, 4020, 4030, 4040, 4050.
    assert_eq!(
        (v.method, v.price, v.samples),
        (NEXT_PRINT, 4_030_000_000, 5)
    );
}

#[test]
fn test_pending_window_blocks_the_next_print_chain() {
    let (mut env, series) = setup(SeriesArgs::xau_put(4_000));
    env.set_time(EXPIRY + 10);
    let mut prev = EXPIRY - 1;
    for t in [EXPIRY + 1, EXPIRY + 2, EXPIRY + 3, EXPIRY + 4, EXPIRY + 5] {
        env.post(&series, 4_500_000, t, prev).unwrap();
        prev = t;
    }
    // Backfill 20 window slots just before the chain would become eligible.
    env.set_time(EXPIRY + D - 1);
    for i in 0..20 {
        post_slot(&mut env, &series, i, 4_000_000).unwrap();
    }
    env.set_time(EXPIRY + D);
    expect_code(env.settle(&series), code::SETTLEMENT_NOT_READY);
    env.set_time(EXPIRY + 2 * D - 1);
    env.settle(&series).unwrap();
    assert_eq!(read_settlement(&env, &series).method, WINDOW);
}

#[test]
fn test_hard_deadline_steps_partial_chain_partial_window_fallback_void() {
    // Partial chain beats a partial window and the fallback.
    let (mut env, series) = setup(SeriesArgs::xau_put(4_000));
    env.set_time(EXPIRY + 10);
    post_slot(&mut env, &series, 0, 3_000_000).unwrap();
    env.post(&series, 4_100_000, EXPIRY + 1, EXPIRY - 3)
        .unwrap();
    env.set_time(HARD - 1);
    expect_code(env.settle(&series), code::SETTLEMENT_NOT_READY);
    env.set_time(HARD + D);
    env.settle(&series).unwrap();
    let v = read_settlement(&env, &series);
    assert_eq!(
        (v.method, v.price, v.samples),
        (PARTIAL_NEXT_PRINT, 4_100_000_000, 1)
    );

    // Partial window.
    let (mut env, series) = setup(SeriesArgs::xau_put(4_000));
    env.set_time(EXPIRY + 10);
    post_slot(&mut env, &series, 3, 3_000_000).unwrap();
    env.set_time(HARD + D);
    env.settle(&series).unwrap();
    assert_eq!(read_settlement(&env, &series).method, PARTIAL_WINDOW);

    // Fallback only: a print an hour before the window, posted after the deadline, waits D.
    let (mut env, series) = setup(SeriesArgs::xau_put(4_000));
    env.set_time(HARD + 100);
    env.post(&series, 3_950_000, WS - 3_600, WS - 3_601)
        .unwrap();
    env.set_time(HARD + 100 + D - 1);
    expect_code(env.settle(&series), code::SETTLEMENT_NOT_READY);
    env.set_time(HARD + 100 + D);
    env.settle(&series).unwrap();
    let v = read_settlement(&env, &series);
    assert_eq!((v.method, v.price), (FALLBACK, 3_950_000_000));

    // Nothing at all: void from the hard deadline plus D; writers get all collateral back.
    let (mut env, series) = setup(SeriesArgs::xau_put(4_000));
    env.set_time(HARD + D - 1);
    expect_code(env.settle(&series), code::SETTLEMENT_NOT_READY);
    env.set_time(HARD + D);
    env.settle(&series).unwrap();
    let v = read_settlement(&env, &series);
    assert_eq!(
        (v.state, v.method, v.price, v.payout, v.residual),
        (VOIDED, VOID, 0, 0, 4_000 * USD)
    );
    expect_code(env.settle(&series), code::SERIES_ALREADY_SETTLED);
}

#[test]
fn test_oracle_checks_reject_forged_and_unsuitable_updates() {
    let (mut env, series) = setup(SeriesArgs::xau_put(4_000));
    env.set_time(EXPIRY + 10);
    let b = boundary(2);
    let good = PriceUpdateArgs::xau(4_000_000, b + 1, b - 1);
    let cases: Vec<(PriceUpdateArgs, u32)> = vec![
        (
            PriceUpdateArgs {
                owner: Address::new_unique(),
                ..good
            },
            code::ORACLE_OWNER_MISMATCH,
        ),
        (
            PriceUpdateArgs {
                partial: Some(5),
                ..good
            },
            code::ORACLE_NOT_FULLY_VERIFIED,
        ),
        (
            PriceUpdateArgs {
                feed_id: FEED_XAG,
                ..good
            },
            code::ORACLE_FEED_MISMATCH,
        ),
        (
            PriceUpdateArgs {
                conf: 40_001,
                ..good
            },
            code::ORACLE_CONFIDENCE_TOO_WIDE,
        ),
        (
            PriceUpdateArgs {
                exponent: -5,
                ..good
            },
            code::ORACLE_EXPONENT_MISMATCH,
        ),
        (
            PriceUpdateArgs {
                price: -4_000_000,
                ..good
            },
            code::INVALID_PRINT,
        ),
        (PriceUpdateArgs { price: 0, ..good }, code::INVALID_PRINT),
        (PriceUpdateArgs { prev: 0, ..good }, code::INVALID_PRINT),
        (
            PriceUpdateArgs {
                discriminator: [0; 8],
                ..good
            },
            code::INVALID_ACCOUNT_DATA,
        ),
    ];
    let admin = env.admin.insecure_clone();
    for (args, code) in cases {
        let pu = env.price_update(args);
        expect_code(env.send(&[env.ix_sample(&series, &pu)], &[&admin]), code);
    }
    // Truncated data.
    let pu = env.price_update(good);
    let mut a = env.account(&pu);
    a.data.truncate(100);
    env.svm.set_account(pu, a).unwrap();
    expect_code(
        env.send(&[env.ix_sample(&series, &pu)], &[&admin]),
        code::INVALID_ACCOUNT_DATA,
    );
    // The genuine update works.
    let pu = env.price_update(good);
    env.send(&[env.ix_sample(&series, &pu)], &[&admin]).unwrap();
    // A forged series (genuine bytes, attacker owner) is rejected before the update is read.
    let fake = Address::new_unique();
    env.forge(&series, fake, Some(Address::new_from_array([0xaa; 32])));
    let pu = env.price_update(PriceUpdateArgs::xau(
        4_000_000,
        boundary(3) + 1,
        boundary(3) - 1,
    ));
    expect_code(
        env.send(&[env.ix_sample(&fake, &pu)], &[&admin]),
        code::INVALID_ACCOUNT_OWNER,
    );
    expect_code(
        env.send(&[env.ix_settle(&fake)], &[&admin]),
        code::INVALID_ACCOUNT_OWNER,
    );
}

#[test]
fn test_live_series_keep_their_oracle_snapshot() {
    let (mut env, series) = setup(SeriesArgs::xau_put(4_000));
    // The admin tightens min_samples to 30 and shortens D after the listing.
    let admin = env.admin.insecure_clone();
    let mut p = Params::default_params();
    p.min_samples = 30;
    p.challenge_delay_s = 60;
    p.feed_id[0] = [9; 32];
    env.send(
        &[env.ix_set_config(&admin.pubkey(), &admin.pubkey(), p)],
        &[&admin],
    )
    .unwrap();
    env.set_time(EXPIRY + 10);
    for i in 0..20 {
        post_slot(&mut env, &series, i, 4_000_000).unwrap();
    }
    // The series still uses min_samples = 20, D = 600 and the XAU feed id.
    env.set_time(EXPIRY + 10 + 60);
    expect_code(env.settle(&series), code::SETTLEMENT_NOT_READY);
    env.set_time(EXPIRY + 10 + D);
    env.settle(&series).unwrap();
    assert_eq!(read_settlement(&env, &series).method, WINDOW);
}

#[test]
fn test_mainnet_xau_update_fills_a_slot() {
    // varswap's capture of a real PriceUpdateV2 (Metal.XAU/USD, Full, exponent -3).
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let data = std::fs::read(dir.join("xau_usd.data.bin")).unwrap();
    let publish = i64::from_le_bytes(data[93..101].try_into().unwrap());
    let prev = i64::from_le_bytes(data[101..109].try_into().unwrap());
    assert_eq!((publish, prev), (1_793_289_597, 1_793_289_596));

    let mut env = Env::new();
    let admin = env.admin.insecure_clone();
    // Expiry 60 s after the print: the print is the canonical print of the last slot.
    let expiry = publish + 60;
    env.set_time(publish - 7_200);
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
    let pu = Address::new_unique();
    let lamports = env.svm.minimum_balance_for_rent_exemption(data.len());
    env.svm
        .set_account(
            pu,
            solana_account::Account {
                lamports,
                data,
                owner: PYTH_RECEIVER,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.send(&[env.ix_sample(&series, &pu)], &[&admin]).unwrap();
    assert_eq!(read_settlement(&env, &series).filled_slots, 1 << 29);
}

#[test]
fn test_writer_position_owner_is_checked() {
    let (mut env, _series) = setup(SeriesArgs::xau_put(4_000));
    let args = SeriesArgs::xau_put(4_000);
    let w = env.writer(100_000);
    env.long_account(&w.pubkey(), args);
    env.send(&[env.ix_mint(&w.pubkey(), args, 1)], &[&w])
        .unwrap();
    // The genuine position address, its genuine bytes, another owner.
    let position = position_address(&args.series(), &w.pubkey());
    env.forge(
        &position,
        position,
        Some(Address::new_from_array([0xaa; 32])),
    );
    expect_code(
        env.send(&[env.ix_mint(&w.pubkey(), args, 1)], &[&w]),
        code::INVALID_ACCOUNT_OWNER,
    );
    expect_code(
        env.send(&[env.ix_close(&w.pubkey(), args, 1)], &[&w]),
        code::INVALID_ACCOUNT_OWNER,
    );
}
