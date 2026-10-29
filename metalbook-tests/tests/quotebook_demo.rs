//! Milestone 5 demo in LiteSVM: a writer mints metalbook long tokens, quotes them on quotebook
//! at a price from `metalbook-maker`, a taker buys through quotebook (signed quote, ed25519
//! precompile and settle, all built by quotebook-sdk), the series settles from Pyth prints, and
//! the taker redeems. The vanilla pricer is optfix's Black-Scholes-Merton
//! (`metalbook_maker::OptfixPricer`). Run `make program quotebook-program` first.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::integer_division,
    clippy::float_arithmetic,
    clippy::panic_in_result_fn,
    clippy::disallowed_macros,
    clippy::result_large_err,
    missing_docs
)]

mod common;

use std::path::PathBuf;

use common::*;
use metalbook_core::{
    OptionKind, OuncesPerContract, SeriesKey, SeriesTerms, StrikeUsd, Underlying,
    UnixTimestamp as MbTime,
};
use metalbook_maker::{mark, quote_atoms, quote_terms, Listing, MarketInputs, OptfixPricer};
use quotebook_sdk::core::instruction::CreateSeriesArgs;
use quotebook_sdk::core::{BaseLots, Quote, SeriesId, Side};
use quotebook_sdk::{deployment_id_from_genesis_hash, ix, pda, SettleAccounts};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use solana_address::Address;
use solana_signer::Signer;

const QUOTEBOOK: Address = Address::new_from_array([0x51; 32]);

#[test]
fn test_quote_and_settle_a_long_token_through_quotebook() {
    // metalbook: a put series and a writer (the maker) with 10 long tokens.
    let mut env = Env::new();
    let args = SeriesArgs::xau_put(4_000);
    let series = env.create_series(args);
    let maker = env.writer(100_000);
    let maker_long = env.long_account(&maker.pubkey(), args);
    env.send(&[env.ix_mint(&maker.pubkey(), args, 10)], &[&maker])
        .unwrap();
    let taker = env.writer(10_000);
    let taker_long = env.long_account(&taker.pubkey(), args);
    let usdc = env.usdc;
    let (maker_usdc, taker_usdc) = (ata(&maker.pubkey(), &usdc), ata(&taker.pubkey(), &usdc));

    // quotebook: deployed with the metalbook admin as upgrade authority.
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/deploy/quotebook_program.so");
    let so = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("{}: {e}; run `make quotebook-program`", path.display()));
    env.svm.add_program(QUOTEBOOK, &so).unwrap();
    let admin = env.admin.insecure_clone();
    let pd = pda::program_data(&QUOTEBOOK).0;
    let mut acc = env.account(&pd);
    acc.data[12] = 1;
    acc.data[13..45].copy_from_slice(admin.pubkey().as_ref());
    env.svm.set_account(pd, acc).unwrap();

    // Everything quotebook goes through quotebook-sdk: config, the long-token/USDC series
    // (one long token per base lot, a 0.01 USD tick), the maker and its delegate allowance.
    let deployment = deployment_id_from_genesis_hash(&[7; 32]);
    env.send(
        &[ix::init_config(
            &QUOTEBOOK,
            &admin.pubkey(),
            &admin.pubkey(),
            deployment,
        )],
        &[&admin],
    )
    .unwrap();
    let qb_args = CreateSeriesArgs {
        base_lot_size: 1,
        quote_lot_size: 10_000,
        max_quote_ttl: 60,
        min_size: 1,
    };
    let long_mint = args.long_mint();
    env.send(
        &[ix::create_series(
            &QUOTEBOOK,
            &admin.pubkey(),
            &admin.pubkey(),
            &long_mint,
            &usdc,
            0,
            &qb_args,
        )],
        &[&admin],
    )
    .unwrap();
    let (qb_series, _) = pda::series(&QUOTEBOOK, &long_mint, &usdc, 0);
    let payer = env.funded();
    env.send(
        &[ix::init_maker(&QUOTEBOOK, &maker.pubkey(), &payer.pubkey())],
        &[&payer, &maker],
    )
    .unwrap();
    env.send(
        &[ix::approve_delegate(
            &QUOTEBOOK,
            &TOKEN_PROGRAM,
            &maker_long,
            &maker.pubkey(),
            10,
        )],
        &[&maker],
    )
    .unwrap();

    // The maker marks the put and asks for 2 long tokens.
    let key = SeriesKey::try_new(
        Underlying::Xau,
        OptionKind::Put,
        StrikeUsd::new(args.strike),
        MbTime::new(args.expiry),
    )
    .unwrap();
    let terms = SeriesTerms::try_new(
        key,
        OuncesPerContract::try_new(args.multiplier).unwrap(),
        StrikeUsd::new(0),
    )
    .unwrap();
    let market = MarketInputs {
        spot: dec!(3950),
        vol: dec!(0.18),
        rate: dec!(0.04),
        carry: Decimal::ZERO,
        now: NOW,
        market_open: true,
    };
    let m = mark(&OptfixPricer, &terms, &market, WINDOW_S as u32).unwrap();
    let q = quote_atoms(&m, dec!(0.50), qb_args.quote_lot_size).unwrap();
    assert!(q.bid < q.ask && q.ask <= 4_000 * USD, "{q:?}");
    let listing = Listing {
        program_id: QUOTEBOOK,
        deployment,
        series: SeriesId::new(qb_series),
        quote_lot_size_atoms: qb_args.quote_lot_size,
    };
    let quote_terms = quote_terms(
        &listing,
        maker.pubkey(),
        taker.pubkey(),
        Side::Ask,
        q.ask,
        2,
        NOW,
        NOW + 30,
        1,
    )
    .unwrap();
    let quote = Quote::new(quote_terms).unwrap();
    let signature: [u8; 64] = maker
        .sign_message(&quotebook_sdk::quote_message(&quote))
        .as_ref()
        .try_into()
        .unwrap();

    // The taker buys 2 long tokens through quotebook.
    let maker_usdc0 = env.token_balance(&maker_usdc);
    let taker_usdc0 = env.token_balance(&taker_usdc);
    let accounts = SettleAccounts {
        taker: taker.pubkey(),
        base_mint: long_mint,
        quote_mint: usdc,
        base_token_program: TOKEN_PROGRAM,
        quote_token_program: TOKEN_PROGRAM,
        maker_base: maker_long,
        maker_quote: maker_usdc,
        taker_base: taker_long,
        taker_quote: taker_usdc,
    };
    let pair = quotebook_sdk::settle_instructions(
        &QUOTEBOOK,
        &quote,
        &signature,
        &accounts,
        BaseLots::new(2),
        0,
    );
    env.send(&pair, &[&taker]).unwrap();
    assert_eq!(env.token_balance(&taker_long), 2);
    assert_eq!(env.token_balance(&maker_long), 8);
    assert_eq!(env.token_balance(&maker_usdc) - maker_usdc0, 2 * q.ask);
    assert_eq!(taker_usdc0 - env.token_balance(&taker_usdc), 2 * q.ask);

    // The series settles in the money at 3900 and the taker redeems: 100 USD per token.
    env.settle_with_window(args, &[3_900_000; 30]);
    let before = env.token_balance(&taker_usdc);
    env.send(&[env.ix_redeem(&taker.pubkey(), args, 2)], &[&taker])
        .unwrap();
    assert_eq!(env.token_balance(&taker_usdc) - before, 2 * 100 * USD);
    // The maker redeems its 8 and claims; the vault empties.
    env.send(&[env.ix_redeem(&maker.pubkey(), args, 8)], &[&maker])
        .unwrap();
    env.send(&[env.ix_claim(&maker.pubkey(), args)], &[&maker])
        .unwrap();
    assert_eq!(env.token_balance(&ata(&series, &usdc)), 0);
}
