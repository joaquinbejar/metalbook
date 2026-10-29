//! `metalbook-sdk` against the program in LiteSVM:
//! - every builder produces exactly the instruction the test harness restates independently
//!   (accounts, signer and writable flags, data), so the two encodings check each other;
//! - a full lifecycle (config, calendar, series, mint, close, sampling, settle, redeem, claim,
//!   and both closes) where every metalbook instruction comes from the SDK and every account is
//!   read back with its decoders. Only token setup and Pyth price-update fixtures are done by
//!   hand. Run `make program` first.

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
use metalbook_sdk::decode::{self, DecodeError};
use metalbook_sdk::ix::{self, ConfigParams};
use metalbook_sdk::pda::{self, SeriesSpec};
use solana_address::Address;
use solana_signer::Signer;

fn sdk_params(p: &Params) -> ConfigParams {
    ConfigParams {
        min_listing_s: p.min_listing_s,
        strike_tick: p.strike_tick,
        multiplier: p.multiplier,
        cap_bps: p.cap_bps,
        receiver_program: p.receiver,
        feed_id: p.feed_id,
        expo: p.expo,
        max_conf_bps: p.max_conf_bps,
        slot_tolerance_s: p.slot_tolerance_s,
        min_samples: p.min_samples,
        k_next_print: p.k_next_print,
        window_s: p.window_s,
        slot_s: p.slot_s,
        challenge_delay_s: p.challenge_delay_s,
        fallback_lookback_s: p.fallback_lookback_s,
        hard_deadline_s: p.hard_deadline_s,
    }
}

fn spec(a: SeriesArgs) -> SeriesSpec {
    SeriesSpec {
        underlying: a.underlying,
        kind: a.kind,
        strike: a.strike,
        expiry: a.expiry,
        cap: a.cap,
        multiplier: a.multiplier,
    }
}

#[test]
fn test_sdk_instructions_equal_the_independent_encodings() {
    let env = Env::new();
    let (admin, writer, payer, other) = (
        Address::new_unique(),
        Address::new_unique(),
        Address::new_unique(),
        Address::new_unique(),
    );
    let p = Params::default_params();
    let args = SeriesArgs::xau_call(4_000);
    let series = args.series();
    let usdc = env.usdc;
    let pu = Address::new_unique();
    let pairs = vec![
        (
            ix::init_config(&PROGRAM_ID, &admin, &usdc, &sdk_params(&p)),
            env.ix_init_config(&admin, p),
        ),
        (
            ix::set_config(&PROGRAM_ID, &admin, &other, &sdk_params(&p)),
            env.ix_set_config(&admin, &other, p),
        ),
        (
            ix::set_calendar(&PROGRAM_ID, &admin, XAU, &[EXPIRY, EXPIRY_2]),
            env.ix_set_calendar(&admin, XAU, &[EXPIRY, EXPIRY_2]),
        ),
        (
            ix::create_series(&PROGRAM_ID, &payer, &usdc, &spec(args)),
            env.ix_create_series(&payer, args),
        ),
        (
            ix::mint(&PROGRAM_ID, &writer, &series, &usdc, 7),
            env.ix_mint(&writer, args, 7),
        ),
        (
            ix::close(&PROGRAM_ID, &writer, &series, &usdc, 3),
            env.ix_close(&writer, args, 3),
        ),
        (
            ix::sample_settlement(&PROGRAM_ID, &series, &pu),
            env.ix_sample(&series, &pu),
        ),
        (ix::settle(&PROGRAM_ID, &series), env.ix_settle(&series)),
        (
            ix::redeem(&PROGRAM_ID, &writer, &series, &usdc, 2),
            env.ix_redeem(&writer, args, 2),
        ),
        (
            ix::claim_writer(&PROGRAM_ID, &writer, &series, &usdc),
            env.ix_claim(&writer, args),
        ),
        (
            ix::close_series(&PROGRAM_ID, &series, &payer, &usdc),
            env.ix_close_series(args, &payer),
        ),
        (
            ix::close_position(&PROGRAM_ID, &series, &writer),
            env.ix_close_position(&position_address(&series, &writer), &writer),
        ),
    ];
    for (i, (sdk, harness)) in pairs.into_iter().enumerate() {
        assert_eq!(sdk.program_id, harness.program_id, "#{i}");
        assert_eq!(sdk.data, harness.data, "#{i} data");
        assert_eq!(sdk.accounts, harness.accounts, "#{i} accounts");
    }
    // PDA helpers match the harness's.
    assert_eq!(pda::config(&PROGRAM_ID).0, config_address());
    assert_eq!(pda::calendar(&PROGRAM_ID, XAG).0, calendar_address(XAG));
    assert_eq!(pda::series(&PROGRAM_ID, &spec(args)).0, series);
    assert_eq!(pda::long_mint(&PROGRAM_ID, &series).0, args.long_mint());
    assert_eq!(pda::program_data(&PROGRAM_ID).0, program_data_address());
}

#[test]
fn test_full_lifecycle_through_the_sdk() {
    // Program, admin as upgrade authority, a USDC mint and the clock: nothing else set up.
    let mut env = Env::bare();
    let admin = env.admin.insecure_clone();
    let usdc = env.usdc;
    let p = sdk_params(&Params::default_params());
    env.send(
        &[ix::init_config(&PROGRAM_ID, &admin.pubkey(), &usdc, &p)],
        &[&admin],
    )
    .unwrap();
    env.send(
        &[ix::set_calendar(
            &PROGRAM_ID,
            &admin.pubkey(),
            XAU,
            &[EXPIRY],
        )],
        &[&admin],
    )
    .unwrap();
    let cfg = env.account(&pda::config(&PROGRAM_ID).0);
    let cfg = decode::config(&PROGRAM_ID, &cfg.owner, &cfg.data).unwrap();
    assert_eq!(cfg.admin, admin.pubkey().to_bytes());
    let cal = env.account(&pda::calendar(&PROGRAM_ID, XAU).0);
    assert_eq!(
        decode::calendar(&PROGRAM_ID, &cal.owner, &cal.data)
            .unwrap()
            .count,
        1
    );

    // A capped call listed by a payer who keeps a USDC account for the close.
    let payer = env.funded();
    env.token_account(&usdc, &payer.pubkey());
    let s = SeriesSpec {
        underlying: XAU,
        kind: CALL,
        strike: 4_000 * USD,
        expiry: EXPIRY,
        cap: 4_400 * USD,
        multiplier: MULT_XAU,
    };
    env.send(
        &[ix::create_series(&PROGRAM_ID, &payer.pubkey(), &usdc, &s)],
        &[&payer],
    )
    .unwrap();
    let series = pda::series(&PROGRAM_ID, &s).0;
    let long_mint = pda::long_mint(&PROGRAM_ID, &series).0;

    // A writer mints 3, closes 1.
    let w = env.writer(10_000);
    env.token_account(&long_mint, &w.pubkey());
    env.send(
        &[ix::mint(&PROGRAM_ID, &w.pubkey(), &series, &usdc, 3)],
        &[&w],
    )
    .unwrap();
    env.send(
        &[ix::close(&PROGRAM_ID, &w.pubkey(), &series, &usdc, 1)],
        &[&w],
    )
    .unwrap();
    let pos = env.account(&pda::position(&PROGRAM_ID, &series, &w.pubkey()).0);
    assert_eq!(
        decode::position(&PROGRAM_ID, &pos.owner, &pos.data)
            .unwrap()
            .written,
        2
    );

    // Settlement from 30 canonical prints at 4200 USD, sampled and settled through the SDK.
    env.set_time(EXPIRY + 10);
    for i in 0..30 {
        let b = EXPIRY - WINDOW_S + i * SLOT_S;
        let pu = env.price_update(PriceUpdateArgs::xau(4_200_000, b + 1, b - 1));
        env.send(
            &[ix::sample_settlement(&PROGRAM_ID, &series, &pu)],
            &[&admin],
        )
        .unwrap();
    }
    env.send(&[ix::settle(&PROGRAM_ID, &series)], &[&admin])
        .unwrap();
    let st = env.account(&series);
    let st = decode::series(&PROGRAM_ID, &st.owner, &st.data).unwrap();
    assert_eq!(
        (
            st.settle_method,
            st.settlement_price,
            st.payout,
            st.residual
        ),
        (0, 4_200 * USD, 200 * USD, 200 * USD)
    );

    // Redeem, claim, close the position and the series.
    let before = env.token_balance(&pda::ata(&w.pubkey(), &usdc));
    env.send(
        &[ix::redeem(&PROGRAM_ID, &w.pubkey(), &series, &usdc, 2)],
        &[&w],
    )
    .unwrap();
    env.send(
        &[ix::claim_writer(&PROGRAM_ID, &w.pubkey(), &series, &usdc)],
        &[&w],
    )
    .unwrap();
    assert_eq!(
        env.token_balance(&pda::ata(&w.pubkey(), &usdc)) - before,
        2 * 400 * USD
    );
    env.send(
        &[ix::close_position(&PROGRAM_ID, &series, &w.pubkey())],
        &[&admin],
    )
    .unwrap();
    env.send(
        &[ix::close_series(
            &PROGRAM_ID,
            &series,
            &payer.pubkey(),
            &usdc,
        )],
        &[&admin],
    )
    .unwrap();
    assert!(env.svm.get_account(&series).is_none_or(|a| a.lamports == 0));

    // Decoders reject foreign owners and other account types.
    let cfg = env.account(&pda::config(&PROGRAM_ID).0);
    assert_eq!(
        decode::config(&PROGRAM_ID, &TOKEN_PROGRAM, &cfg.data).unwrap_err(),
        DecodeError::WrongOwner
    );
    assert!(matches!(
        decode::series(&PROGRAM_ID, &cfg.owner, &cfg.data),
        Err(DecodeError::Layout(_))
    ));
}
