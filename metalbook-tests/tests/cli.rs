//! `metalbook-cli` against the program in LiteSVM: the admin CLI's commands (the same functions
//! `metalbook-admin` calls) run through an in-process [`Rpc`], from `init_config` with the
//! committed devnet parameters file to a listed series, and every read decodes what the program
//! wrote. Run `make program` first.

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
use metalbook_cli::params::{parse_usd, ParamsFile};
use metalbook_cli::{
    calendar_json, config_json, create_series_ix, describe, init_config_ix, read_calendar,
    read_config, read_series, series_json, set_calendar_ix, set_config_ix, submit, AccountInfo,
    CliError, Rpc, SeriesArgs,
};
use metalbook_sdk::pda::{self, SeriesSpec};
use solana_address::Address;
use solana_signer::Signer;
use solana_transaction::Transaction;

/// [`Rpc`] over the LiteSVM of an [`Env`].
struct Svm<'a>(&'a mut Env);

impl Rpc for Svm<'_> {
    fn latest_blockhash(&mut self) -> Result<solana_hash::Hash, CliError> {
        // A fresh blockhash per transaction, so identical transactions do not collide.
        self.0.svm.expire_blockhash();
        Ok(self.0.svm.latest_blockhash())
    }

    fn account(&mut self, address: &Address) -> Result<Option<AccountInfo>, CliError> {
        Ok(self.0.svm.get_account(address).map(|a| AccountInfo {
            owner: a.owner,
            data: a.data,
        }))
    }

    fn send(&mut self, tx: &Transaction) -> Result<(), CliError> {
        self.0
            .svm
            .send_transaction(tx.clone())
            .map(|_| ())
            .map_err(|e| CliError::Rpc(format!("{:?} {:?}", e.err, e.meta.logs)))
    }
}

/// `MetalbookError` codes (`metalbook-program/src/error.rs`).
const NOT_AUTHORITY: u32 = 108;
const EXPIRY_NOT_IN_CALENDAR: u32 = 110;
const STRIKE_OFF_GRID: u32 = 111;

fn devnet_params() -> ParamsFile {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../metalbook-cli/devnet-params.json"
    );
    ParamsFile::from_json(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn custom_code(result: Result<String, CliError>, code: u32) {
    match result {
        Err(CliError::Rpc(err)) => {
            assert!(err.contains(&format!("Custom({code})")), "{err}");
        }
        other => panic!("expected custom error {code}, got {other:?}"),
    }
}

#[test]
fn the_admin_cli_lists_a_series_from_scratch() {
    let mut env = Env::bare();
    let admin = env.admin.insecure_clone();
    let payer = env.funded();
    let file = devnet_params();
    let params = file.to_params().unwrap();
    let mut rpc = Svm(&mut env);

    // Nothing exists yet.
    assert!(matches!(
        read_config(&mut rpc, &PROGRAM_ID),
        Err(CliError::AccountNotFound(_))
    ));

    // init_config: only the upgrade authority; `payer` is rejected by the program.
    let ix = init_config_ix(&PROGRAM_ID, &payer.pubkey(), &USDC_MINT, &params);
    custom_code(submit(&mut rpc, &[ix], &payer), NOT_AUTHORITY);
    let ix = init_config_ix(&PROGRAM_ID, &admin.pubkey(), &USDC_MINT, &params);
    submit(&mut rpc, &[ix], &admin).unwrap();

    // show-config prints the parameters file back, so it can be edited and resent.
    let config = read_config(&mut rpc, &PROGRAM_ID).unwrap();
    let shown = config_json(&PROGRAM_ID, &config);
    assert_eq!(shown["params"], serde_json::to_value(&file).unwrap());
    assert_eq!(shown["admin"], admin.pubkey().to_string());
    assert_eq!(shown["usdc_mint"], USDC_MINT.to_string());
    assert_eq!(shown["address"], pda::config(&PROGRAM_ID).0.to_string());
    let round_trip: ParamsFile = serde_json::from_value(shown["params"].clone()).unwrap();
    assert_eq!(round_trip.to_params().unwrap(), params);

    // set_calendar: the admin only.
    let expiries = [EXPIRY, EXPIRY_2];
    let ix = set_calendar_ix(&PROGRAM_ID, &payer.pubkey(), XAU, &expiries).unwrap();
    custom_code(submit(&mut rpc, &[ix], &payer), NOT_AUTHORITY);
    let ix = set_calendar_ix(&PROGRAM_ID, &admin.pubkey(), XAU, &expiries).unwrap();
    submit(&mut rpc, &[ix], &admin).unwrap();
    let calendar = read_calendar(&mut rpc, &PROGRAM_ID, XAU).unwrap();
    let shown = calendar_json(&PROGRAM_ID, &calendar);
    assert_eq!(shown["underlying"], "xau");
    assert_eq!(shown["expiries"], serde_json::json!(expiries));

    // create_series by any payer, multiplier taken from the config.
    let args = SeriesArgs {
        underlying: XAU,
        kind: 1,
        strike: parse_usd("2400").unwrap(),
        expiry: EXPIRY,
        cap: 0,
        multiplier: None,
    };
    let (ix, series) = create_series_ix(&mut rpc, &PROGRAM_ID, &payer.pubkey(), &args).unwrap();
    let dump = describe(&ix);
    assert!(
        dump.starts_with(&format!("program {PROGRAM_ID}\n")),
        "{dump}"
    );
    assert!(dump.contains(&format!("{series} writable")), "{dump}");
    submit(&mut rpc, &[ix], &payer).unwrap();
    let spec = SeriesSpec {
        underlying: XAU,
        kind: 1,
        strike: 2_400_000_000,
        expiry: EXPIRY,
        cap: 0,
        multiplier: 1_000,
    };
    assert_eq!(series, pda::series(&PROGRAM_ID, &spec).0);
    let shown = series_json(
        &series,
        &read_series(&mut rpc, &PROGRAM_ID, &series).unwrap(),
    );
    assert_eq!(shown["kind"], "put");
    assert_eq!(shown["underlying"], "xau");
    assert_eq!(shown["strike_usd"], "2400");
    assert_eq!(shown["cap_usd"], "0");
    assert_eq!(shown["expiry"], EXPIRY);
    assert_eq!(shown["multiplier_milli_oz"], 1_000);
    assert_eq!(shown["state"], "active");
    assert_eq!(shown["open"], 0);
    assert_eq!(
        shown["long_mint"],
        pda::long_mint(&PROGRAM_ID, &series).0.to_string()
    );
    assert_eq!(shown["vault"], pda::ata(&series, &USDC_MINT).to_string());

    // A strike off the 5 USD tick, or an expiry outside the calendar, is the program's to
    // reject; the CLI passes the error through with its code.
    let off_tick = SeriesArgs {
        strike: parse_usd("2401").unwrap(),
        ..args
    };
    let (ix, _) = create_series_ix(&mut rpc, &PROGRAM_ID, &payer.pubkey(), &off_tick).unwrap();
    custom_code(submit(&mut rpc, &[ix], &payer), STRIKE_OFF_GRID);
    let off_calendar = SeriesArgs {
        expiry: EXPIRY + 60,
        ..args
    };
    let (ix, _) = create_series_ix(&mut rpc, &PROGRAM_ID, &payer.pubkey(), &off_calendar).unwrap();
    custom_code(submit(&mut rpc, &[ix], &payer), EXPIRY_NOT_IN_CALENDAR);

    // Reading the wrong kind of account is a decode error, a missing one is not found.
    assert!(matches!(
        read_series(&mut rpc, &PROGRAM_ID, &pda::config(&PROGRAM_ID).0),
        Err(CliError::Decode { what: "series", .. })
    ));
    assert!(matches!(
        read_series(&mut rpc, &PROGRAM_ID, &Address::new_from_array([3; 32])),
        Err(CliError::AccountNotFound(_))
    ));

    // set_config hands admin over; the old admin then loses set_calendar.
    let new_admin = payer.pubkey();
    let ix = set_config_ix(&PROGRAM_ID, &admin.pubkey(), Some(&new_admin), &params);
    submit(&mut rpc, &[ix], &admin).unwrap();
    let config = read_config(&mut rpc, &PROGRAM_ID).unwrap();
    assert_eq!(
        config_json(&PROGRAM_ID, &config)["admin"],
        new_admin.to_string()
    );
    let ix = set_calendar_ix(&PROGRAM_ID, &admin.pubkey(), XAG, &[EXPIRY]).unwrap();
    custom_code(submit(&mut rpc, &[ix], &admin), NOT_AUTHORITY);
    let ix = set_calendar_ix(&PROGRAM_ID, &new_admin, XAG, &[EXPIRY]).unwrap();
    submit(&mut rpc, &[ix], &payer).unwrap();
    assert_eq!(read_calendar(&mut rpc, &PROGRAM_ID, XAG).unwrap().count, 1);
}

#[test]
fn the_admin_cli_mints_for_a_writer_creating_its_long_account() {
    let mut env = Env::new();
    let args = common::SeriesArgs::xau_call(4_000);
    let series = env.create_series(args);
    // Three contracts of a 4000/4400 call at 1 oz hold 3 × 400 USD.
    let writer = env.writer(1_200);
    let mut rpc = Svm(&mut env);
    let ixs = metalbook_cli::mint_ixs(&mut rpc, &PROGRAM_ID, &writer.pubkey(), &series, 2).unwrap();
    assert_eq!(ixs.len(), 2, "create the long-token account, then mint");
    submit(&mut rpc, &ixs, &writer).unwrap();
    // Again: the account creation is idempotent.
    let ixs = metalbook_cli::mint_ixs(&mut rpc, &PROGRAM_ID, &writer.pubkey(), &series, 1).unwrap();
    submit(&mut rpc, &ixs, &writer).unwrap();
    let shown = series_json(
        &series,
        &read_series(&mut rpc, &PROGRAM_ID, &series).unwrap(),
    );
    assert_eq!(shown["open"], 3);
    assert_eq!(shown["longs_outstanding"], 3);
    let long = pda::ata(&writer.pubkey(), &pda::long_mint(&PROGRAM_ID, &series).0);
    assert_eq!(env.token_balance(&long), 3);
}

#[test]
fn the_admin_cli_redeems_and_claims_after_settlement() {
    let mut env = Env::new();
    let args = common::SeriesArgs::xau_put(4_000);
    let series = env.create_series(args);
    let writer = env.writer(8_000); // two contracts of a 4000 put at 1 oz
    let mut rpc = Svm(&mut env);
    let ixs = metalbook_cli::mint_ixs(&mut rpc, &PROGRAM_ID, &writer.pubkey(), &series, 2).unwrap();
    submit(&mut rpc, &ixs, &writer).unwrap();
    // Not settled yet: the program refuses both.
    let ix = metalbook_cli::redeem_ix(&mut rpc, &PROGRAM_ID, &writer.pubkey(), &series, 1).unwrap();
    assert!(submit(&mut rpc, &[ix], &writer).is_err());
    // Settles at 3600: each long token pays 400 USD, each written contract leaves 3600.
    env.settle_with_window(args, &[3_600_000; 30]);
    let usdc = pda::ata(&writer.pubkey(), &USDC_MINT);
    let mut rpc = Svm(&mut env);
    let ix = metalbook_cli::redeem_ix(&mut rpc, &PROGRAM_ID, &writer.pubkey(), &series, 2).unwrap();
    submit(&mut rpc, &[ix], &writer).unwrap();
    assert_eq!(env.token_balance(&usdc), 800 * 1_000_000);
    let mut rpc = Svm(&mut env);
    let ix =
        metalbook_cli::claim_writer_ix(&mut rpc, &PROGRAM_ID, &writer.pubkey(), &series).unwrap();
    submit(&mut rpc, &[ix], &writer).unwrap();
    // Everything deposited is back with the writer, who was both sides.
    assert_eq!(env.token_balance(&usdc), 8_000 * 1_000_000);
    let shown = series_json(
        &series,
        &read_series(&mut Svm(&mut env), &PROGRAM_ID, &series).unwrap(),
    );
    assert_eq!(shown["state"], "settled");
    assert_eq!(shown["vault_owed_atoms"], 0);
    assert_eq!(shown["settle_method"], "window");
    assert_eq!(shown["settlement_price_usd"], "3600");
    assert_eq!(shown["settlement_samples"], 30);
    let c = &shown["candidates"];
    assert_eq!(c["window_slots"], 30);
    assert_eq!(c["window_slots_total"], 30);
    assert_eq!(c["min_samples"], MIN_SAMPLES);
    assert_eq!(c["next_print_links"], 0);
    // The first window print is also the fallback candidate (within the lookback, before expiry).
    assert!(c["fallback_time"].as_i64().unwrap() <= args.expiry);
    assert_eq!(c["window_end"], args.expiry);
    assert_eq!(c["hard_deadline"], args.expiry + HARD_S);
}

#[test]
fn the_admin_cli_voids_an_unsampled_series_and_closes_it() {
    // The Nov 6 drill (docs/PLAN-improvements.md, P3): a series nobody sampled voids from
    // `hard_deadline + D`, then `close-series` returns its rent to the payer recorded at
    // `create_series`, creating that payer's USDC account first.
    let mut env = Env::new();
    let args = common::SeriesArgs::xau_put(4_000);
    let series = env.create_series(args);
    let rent_payer = {
        let s = read_series(&mut Svm(&mut env), &PROGRAM_ID, &series).unwrap();
        Address::new_from_array(s.rent_payer)
    };
    let caller = env.funded();
    let settle = metalbook_cli::settle_ix(&PROGRAM_ID, &series);
    // One second early: the void is not ready yet.
    env.set_time(EXPIRY + HARD_S + D - 1);
    assert!(submit(&mut Svm(&mut env), std::slice::from_ref(&settle), &caller).is_err());
    env.set_time(EXPIRY + HARD_S + D);
    submit(&mut Svm(&mut env), &[settle], &caller).unwrap();
    let shown = series_json(
        &series,
        &read_series(&mut Svm(&mut env), &PROGRAM_ID, &series).unwrap(),
    );
    assert_eq!(shown["state"], "voided");

    let rent = env.account(&series).lamports + env.account(&pda::ata(&series, &USDC_MINT)).lamports;
    let before = env.account(&rent_payer).lamports;
    let payer_usdc = pda::ata(&rent_payer, &USDC_MINT);
    assert!(env.svm.get_account(&payer_usdc).is_none());
    let mut rpc = Svm(&mut env);
    let ixs =
        metalbook_cli::close_series_ixs(&mut rpc, &PROGRAM_ID, &caller.pubkey(), &series).unwrap();
    submit(&mut rpc, &ixs, &caller).unwrap();
    // Everything went to the rent payer; the caller paid the USDC account and the fee.
    assert_eq!(env.account(&rent_payer).lamports, before + rent);
    assert!(env.svm.get_account(&series).is_none_or(|a| a.lamports == 0));
    assert_eq!(env.token_balance(&payer_usdc), 0);
    // A second close finds no series.
    let mut rpc = Svm(&mut env);
    assert!(
        metalbook_cli::close_series_ixs(&mut rpc, &PROGRAM_ID, &caller.pubkey(), &series).is_err()
    );
}

#[test]
fn the_admin_cli_week_listing_is_accepted_by_the_program_and_idempotent() {
    // `list-week` plans from the on-chain config only; every series it plans must pass the
    // program's grid, cap and calendar checks, and a second run finds them all listed.
    let mut env = Env::new();
    let payer = env.funded();
    let config = read_config(&mut Svm(&mut env), &PROGRAM_ID).unwrap();
    let spot = metalbook_cli::listing::spot_atoms(4_012_345, -3).unwrap();
    let plan = metalbook_cli::listing::week_series(
        0,
        EXPIRY,
        spot,
        config.strike_tick[0],
        metalbook_cli::listing::LISTING_BAND_BPS,
        config.cap_bps[0],
    )
    .unwrap();
    assert!(!plan.is_empty());
    for args in &plan {
        let mut rpc = Svm(&mut env);
        let (ix, series) = create_series_ix(&mut rpc, &PROGRAM_ID, &payer.pubkey(), args).unwrap();
        assert!(rpc.account(&series).unwrap().is_none());
        submit(&mut rpc, &[ix], &payer).unwrap();
        assert!(rpc.account(&series).unwrap().is_some());
    }
    // Second run: everything exists, so `list-week` would send nothing.
    for args in &plan {
        let mut rpc = Svm(&mut env);
        let (_, series) = create_series_ix(&mut rpc, &PROGRAM_ID, &payer.pubkey(), args).unwrap();
        assert!(rpc.account(&series).unwrap().is_some());
    }
}

#[test]
fn the_admin_cli_reclaims_only_series_that_owe_nothing() {
    let mut env = Env::new();
    let args = common::SeriesArgs::xau_put(4_000);
    let series = env.create_series(args);
    let writer = env.writer(4_000);
    let mut rpc = Svm(&mut env);
    let ixs = metalbook_cli::mint_ixs(&mut rpc, &PROGRAM_ID, &writer.pubkey(), &series, 1).unwrap();
    submit(&mut rpc, &ixs, &writer).unwrap();
    let reclaimable = |env: &mut Env| {
        metalbook_cli::listing::reclaimable(
            &read_series(&mut Svm(env), &PROGRAM_ID, &series).unwrap(),
        )
    };
    // Active, then settled with a long and a writer claim outstanding: not closable.
    assert!(!reclaimable(&mut env));
    env.settle_with_window(args, &[3_600_000; 30]);
    assert!(!reclaimable(&mut env));
    let mut rpc = Svm(&mut env);
    let ix = metalbook_cli::redeem_ix(&mut rpc, &PROGRAM_ID, &writer.pubkey(), &series, 1).unwrap();
    submit(&mut rpc, &[ix], &writer).unwrap();
    assert!(!reclaimable(&mut env));
    let mut rpc = Svm(&mut env);
    let ix =
        metalbook_cli::claim_writer_ix(&mut rpc, &PROGRAM_ID, &writer.pubkey(), &series).unwrap();
    submit(&mut rpc, &[ix], &writer).unwrap();
    // Everything paid: closable, and the close succeeds.
    assert!(reclaimable(&mut env));
    let caller = env.funded();
    let mut rpc = Svm(&mut env);
    let ixs =
        metalbook_cli::close_series_ixs(&mut rpc, &PROGRAM_ID, &caller.pubkey(), &series).unwrap();
    submit(&mut rpc, &ixs, &caller).unwrap();
}
