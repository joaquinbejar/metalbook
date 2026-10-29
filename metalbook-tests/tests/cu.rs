//! Measures the compute units of every instruction in LiteSVM.
//!
//! - The measurement runs twice in fresh VMs and must match exactly (CU are deterministic).
//! - With `METALBOOK_BENCH_OUT=<path>` the table is written there (`make bench` uses
//!   `docs/benchmarks.md`); it is always printed.
//!
//! Run `make program` first.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::integer_division,
    clippy::disallowed_macros,
    missing_docs
)]

mod common;

use std::fmt::Write as _;

use common::*;
use solana_signer::Signer;

/// One row per measured call: instruction, case, compute units.
fn measure() -> Vec<(&'static str, &'static str, u64)> {
    let mut env = Env::bare();
    let admin = env.admin.insecure_clone();
    let mut rows = Vec::new();

    let ix = env.ix_init_config(&admin.pubkey(), Params::default_params());
    rows.push((
        "init_config",
        "creates the config",
        env.send(&[ix], &[&admin]).unwrap(),
    ));
    let ix = env.ix_set_config(&admin.pubkey(), &admin.pubkey(), Params::default_params());
    rows.push((
        "set_config",
        "same admin",
        env.send(&[ix], &[&admin]).unwrap(),
    ));
    let ix = env.ix_set_calendar(&admin.pubkey(), XAU, &[EXPIRY, EXPIRY_2]);
    rows.push((
        "set_calendar",
        "creates the calendar, 2 expiries",
        env.send(&[ix], &[&admin]).unwrap(),
    ));
    let full: Vec<i64> = (0..64).map(|i| EXPIRY + i * 86_400).collect();
    let ix = env.ix_set_calendar(&admin.pubkey(), XAU, &full);
    rows.push((
        "set_calendar",
        "replaces the list, 64 expiries",
        env.send(&[ix], &[&admin]).unwrap(),
    ));

    let payer = env.funded();
    let put = SeriesArgs::xau_put(4_000);
    let ix = env.ix_create_series(&payer.pubkey(), put);
    rows.push(("create_series", "put", env.send(&[ix], &[&payer]).unwrap()));
    let call = SeriesArgs::xau_call(4_000);
    let ix = env.ix_create_series(&payer.pubkey(), call);
    rows.push((
        "create_series",
        "capped call",
        env.send(&[ix], &[&payer]).unwrap(),
    ));

    let w = env.writer(100_000);
    env.long_account(&w.pubkey(), put);
    let ix = env.ix_mint(&w.pubkey(), put, 3);
    rows.push((
        "mint",
        "first mint, creates the position",
        env.send(&[ix], &[&w]).unwrap(),
    ));
    let ix = env.ix_mint(&w.pubkey(), put, 2);
    rows.push(("mint", "existing position", env.send(&[ix], &[&w]).unwrap()));
    let ix = env.ix_close(&w.pubkey(), put, 4);
    rows.push(("close", "partial", env.send(&[ix], &[&w]).unwrap()));
    let ix = env.ix_close(&w.pubkey(), put, 1);
    rows.push(("close", "to zero", env.send(&[ix], &[&w]).unwrap()));

    // Settlement of the call series: 30 window slots, then a chain link and a fallback.
    let series = call.series();
    let mint_w = env.writer(100_000);
    env.long_account(&mint_w.pubkey(), call);
    env.send(&[env.ix_mint(&mint_w.pubkey(), call, 1)], &[&mint_w])
        .unwrap();
    env.set_time(EXPIRY + 10);
    let start = EXPIRY - WINDOW_S;
    let pu = env.price_update(PriceUpdateArgs::xau(4_100_000, start + 1, start - 1));
    let ix = env.ix_sample(&series, &pu);
    rows.push((
        "sample_settlement",
        "first print: slot 0 and fallback, expires the series",
        env.send(&[ix], &[&admin]).unwrap(),
    ));
    for i in 1..29 {
        let b = start + i * SLOT_S;
        let pu = env.price_update(PriceUpdateArgs::xau(4_100_000, b + 1, b - 1));
        env.send(&[env.ix_sample(&series, &pu)], &[&admin]).unwrap();
    }
    let b = start + 29 * SLOT_S;
    let pu = env.price_update(PriceUpdateArgs::xau(4_100_000, b + 1, b - 1));
    let ix = env.ix_sample(&series, &pu);
    rows.push((
        "sample_settlement",
        "window slot and fallback",
        env.send(&[ix], &[&admin]).unwrap(),
    ));
    let pu = env.price_update(PriceUpdateArgs::xau(4_100_000, EXPIRY + 1, b + 1));
    let ix = env.ix_sample(&series, &pu);
    rows.push((
        "sample_settlement",
        "next-print link",
        env.send(&[ix], &[&admin]).unwrap(),
    ));
    let ix = env.ix_settle(&series);
    rows.push((
        "settle",
        "full window (30 slots)",
        env.send(&[ix], &[&admin]).unwrap(),
    ));

    let ix = env.ix_redeem(&mint_w.pubkey(), call, 1);
    rows.push((
        "redeem",
        "1 long token, in the money",
        env.send(&[ix], &[&mint_w]).unwrap(),
    ));
    let ix = env.ix_claim(&mint_w.pubkey(), call);
    rows.push((
        "claim_writer",
        "residual of 1 contract",
        env.send(&[ix], &[&mint_w]).unwrap(),
    ));

    let position = position_address(&series, &mint_w.pubkey());
    let ix = env.ix_close_position(&position, &mint_w.pubkey());
    rows.push((
        "close_position",
        "claimed position, rent to the writer",
        env.send(&[ix], &[&admin]).unwrap(),
    ));
    let usdc = env.usdc;
    env.token_account(&usdc, &payer.pubkey());
    let ix = env.ix_close_series(call, &payer.pubkey());
    rows.push((
        "close_series",
        "vault and series closed, rent to the payer",
        env.send(&[ix], &[&admin]).unwrap(),
    ));

    // Void: the put series, nothing posted, after the hard deadline plus D.
    env.set_time(EXPIRY + HARD_S + D);
    let ix = env.ix_settle(&put.series());
    rows.push(("settle", "void", env.send(&[ix], &[&admin]).unwrap()));
    rows
}

#[test]
fn test_cu_table_is_deterministic_and_written() {
    let first = measure();
    let second = measure();
    assert_eq!(first, second, "CU differ between two fresh runs");

    let mut out = String::new();
    writeln!(out, "# Compute units").unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "Measured by `make bench` (`metalbook-tests/tests/cu.rs`) in LiteSVM, sBPF v3, including the"
    )
    .unwrap();
    writeln!(
        out,
        "CPIs each instruction makes (system, token, associated token). Deterministic: two fresh"
    )
    .unwrap();
    writeln!(out, "runs must agree exactly.").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "| Instruction | Case | CU |").unwrap();
    writeln!(out, "| --- | --- | ---: |").unwrap();
    for (ix, case, cu) in &first {
        writeln!(out, "| `{ix}` | {case} | {cu} |").unwrap();
    }
    print!("{out}");
    if let Ok(path) = std::env::var("METALBOOK_BENCH_OUT") {
        std::fs::write(&path, &out).unwrap();
    }
}
