//! LiteSVM tests of milestone 2: config, calendars, series creation, mint and close. Every
//! account-validation rule of `docs/PLAN.md` 3.3 that applies to these instructions has a
//! negative test, most with a forged account (a copy of a genuine account at another address
//! or under another owner). Run `make program` first.

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
use litesvm_token::CreateMint;
use solana_address::Address;
use solana_keypair::Keypair;
use solana_signer::Signer;

const ATTACKER_PROGRAM: Address = Address::new_from_array([0xaa; 32]);

// ---------------------------------------------------------------------------------------------
// init_config, set_config, set_calendar
// ---------------------------------------------------------------------------------------------

#[test]
fn test_init_config_by_upgrade_authority_succeeds_once() {
    let mut env = Env::bare();
    let admin = env.admin.insecure_clone();
    let ix = env.ix_init_config(&admin.pubkey(), Params::default_params());
    env.send(std::slice::from_ref(&ix), &[&admin]).unwrap();
    let d = env.account(&config_address()).data;
    assert_eq!(d.len(), 280);
    assert_eq!(&d[8..40], admin.pubkey().as_ref());
    assert_eq!(&d[40..72], env.usdc.as_ref());
    assert_eq!(&d[72..104], TOKEN_PROGRAM.as_ref());
    expect_code(env.send(&[ix], &[&admin]), code::ALREADY_INITIALIZED);
}

#[test]
fn test_init_config_by_other_signer_is_rejected() {
    let mut env = Env::bare();
    let intruder = env.funded();
    let ix = env.ix_init_config(&intruder.pubkey(), Params::default_params());
    expect_code(env.send(&[ix], &[&intruder]), code::NOT_AUTHORITY);
}

#[test]
fn test_init_config_unsigned_admin_is_rejected() {
    let mut env = Env::bare();
    let admin = env.admin.insecure_clone();
    let payer = env.funded();
    let mut ix = env.ix_init_config(&admin.pubkey(), Params::default_params());
    ix.accounts[0].is_signer = false;
    expect_code(env.send(&[ix], &[&payer]), code::MISSING_SIGNATURE);
}

#[test]
fn test_init_config_forged_program_data_is_rejected() {
    let mut env = Env::bare();
    let admin = env.admin.insecure_clone();
    // Same bytes, owned by another program.
    let fake = Address::new_unique();
    env.forge(&program_data_address(), fake, Some(ATTACKER_PROGRAM));
    let mut ix = env.ix_init_config(&admin.pubkey(), Params::default_params());
    ix.accounts[2].pubkey = fake;
    expect_code(
        env.send(&[ix.clone()], &[&admin]),
        code::INVALID_ACCOUNT_OWNER,
    );
    // Same bytes and owner, wrong address.
    env.forge(&program_data_address(), fake, None);
    expect_code(env.send(&[ix], &[&admin]), code::INVALID_PDA);
}

#[test]
fn test_init_config_program_data_without_or_with_other_authority_is_rejected() {
    let mut env = Env::bare();
    let admin = env.admin.insecure_clone();
    let pd = program_data_address();
    let ix = env.ix_init_config(&admin.pubkey(), Params::default_params());
    // ProgramData naming a different upgrade authority.
    let mut a = env.account(&pd);
    a.data[13..45].copy_from_slice(Address::new_unique().as_ref());
    env.svm.set_account(pd, a.clone()).unwrap();
    expect_code(
        env.send(std::slice::from_ref(&ix), &[&admin]),
        code::NOT_AUTHORITY,
    );
    // ProgramData with no upgrade authority (immutable program).
    a.data[12] = 0;
    a.data[13..45].copy_from_slice(admin.pubkey().as_ref());
    env.svm.set_account(pd, a).unwrap();
    expect_code(env.send(&[ix], &[&admin]), code::NOT_AUTHORITY);
}

#[test]
fn test_init_config_wrong_config_address_is_rejected() {
    let mut env = Env::bare();
    let admin = env.admin.insecure_clone();
    let mut ix = env.ix_init_config(&admin.pubkey(), Params::default_params());
    ix.accounts[1].pubkey = Address::new_unique();
    expect_code(env.send(&[ix], &[&admin]), code::INVALID_PDA);
}

#[test]
fn test_init_config_forged_usdc_mint_is_rejected() {
    let mut env = Env::bare();
    let admin = env.admin.insecure_clone();
    let fake = Address::new_unique();
    env.forge(&env.usdc.clone(), fake, Some(ATTACKER_PROGRAM));
    let mut ix = env.ix_init_config(&admin.pubkey(), Params::default_params());
    ix.accounts[3].pubkey = fake;
    expect_code(env.send(&[ix], &[&admin]), code::INVALID_MINT);
}

#[test]
fn test_init_config_invalid_params_are_rejected() {
    let mut env = Env::bare();
    let admin = env.admin.insecure_clone();
    let mut bad = Vec::new();
    let mut p = Params::default_params();
    p.strike_tick[1] = 0;
    bad.push(p);
    let mut p = Params::default_params();
    p.multiplier[0] = 0;
    bad.push(p);
    let mut p = Params::default_params();
    p.cap_bps = [0; 4];
    bad.push(p);
    let mut p = Params::default_params();
    p.cap_bps[3] = 60_001;
    bad.push(p);
    for p in bad {
        let ix = env.ix_init_config(&admin.pubkey(), p);
        expect_code(env.send(&[ix], &[&admin]), code::INVALID_CONFIG);
    }
}

#[test]
fn test_set_config_admin_only_and_rotates_admin() {
    let mut env = Env::new();
    let admin = env.admin.insecure_clone();
    let intruder = env.funded();
    let ix = env.ix_set_config(
        &intruder.pubkey(),
        &intruder.pubkey(),
        Params::default_params(),
    );
    expect_code(env.send(&[ix], &[&intruder]), code::NOT_AUTHORITY);

    let new_admin = env.funded();
    let mut p = Params::default_params();
    p.min_listing_s = 60;
    let ix = env.ix_set_config(&admin.pubkey(), &new_admin.pubkey(), p);
    env.send(&[ix], &[&admin]).unwrap();
    let d = env.account(&config_address()).data;
    assert_eq!(&d[8..40], new_admin.pubkey().as_ref());
    assert_eq!(u32::from_le_bytes(d[104..108].try_into().unwrap()), 60);
    let ix = env.ix_set_config(&admin.pubkey(), &admin.pubkey(), p);
    expect_code(env.send(&[ix], &[&admin]), code::NOT_AUTHORITY);
}

#[test]
fn test_set_config_forged_config_is_rejected() {
    let mut env = Env::new();
    let admin = env.admin.insecure_clone();
    let fake = Address::new_unique();
    env.forge(&config_address(), fake, Some(ATTACKER_PROGRAM));
    let mut ix = env.ix_set_config(&admin.pubkey(), &admin.pubkey(), Params::default_params());
    ix.accounts[1].pubkey = fake;
    expect_code(
        env.send(&[ix.clone()], &[&admin]),
        code::INVALID_ACCOUNT_OWNER,
    );
    env.forge(&config_address(), fake, None);
    expect_code(env.send(&[ix], &[&admin]), code::INVALID_PDA);
}

#[test]
fn test_set_calendar_rules() {
    let mut env = Env::new();
    let admin = env.admin.insecure_clone();
    let intruder = env.funded();
    let ix = env.ix_set_calendar(&intruder.pubkey(), XAU, &[EXPIRY]);
    expect_code(env.send(&[ix], &[&intruder]), code::NOT_AUTHORITY);
    let ix = env.ix_set_calendar(&admin.pubkey(), XAU, &[EXPIRY_2, EXPIRY]);
    expect_code(env.send(&[ix], &[&admin]), code::INVALID_INSTRUCTION_DATA);
    let ix = env.ix_set_calendar(&admin.pubkey(), XAU, &[EXPIRY, EXPIRY]);
    expect_code(env.send(&[ix], &[&admin]), code::INVALID_INSTRUCTION_DATA);
    let many: Vec<i64> = (0..65).map(|i| EXPIRY + i).collect();
    let ix = env.ix_set_calendar(&admin.pubkey(), XAU, &many);
    expect_code(env.send(&[ix], &[&admin]), code::INVALID_INSTRUCTION_DATA);
    // The XAG calendar address for the XAU underlying.
    let mut ix = env.ix_set_calendar(&admin.pubkey(), XAU, &[EXPIRY]);
    ix.accounts[2].pubkey = calendar_address(XAG);
    expect_code(env.send(&[ix], &[&admin]), code::INVALID_PDA);
    // A full calendar of 64 is accepted and replaces the list.
    let full: Vec<i64> = (0..64).map(|i| EXPIRY + i * 86_400).collect();
    let ix = env.ix_set_calendar(&admin.pubkey(), XAU, &full);
    env.send(&[ix], &[&admin]).unwrap();
    assert_eq!(env.account(&calendar_address(XAU)).data[4], 64);
}

// ---------------------------------------------------------------------------------------------
// create_series
// ---------------------------------------------------------------------------------------------

#[test]
fn test_create_series_put_and_capped_call_on_both_metals() {
    let mut env = Env::new();
    let xag_call = SeriesArgs {
        underlying: XAG,
        kind: CALL,
        strike: 48 * USD,
        expiry: EXPIRY,
        cap: 60 * USD,
        multiplier: MULT_XAG,
    };
    for args in [
        SeriesArgs::xau_put(4_000),
        SeriesArgs::xau_call(4_000),
        xag_call,
    ] {
        let series = env.create_series(args);
        let d = env.account(&series).data;
        assert_eq!(d.len(), 472);
        assert_eq!(env.account(&series).owner, PROGRAM_ID);
        assert_eq!(d[0], 3, "tag");
        assert_eq!(d[4], 0, "Active");
        assert_eq!(&d[104..136], args.long_mint().as_ref());
        assert_eq!(&d[136..168], ata(&series, &env.usdc).as_ref());
        // Long mint: decimals 0, mint authority the series, no freeze authority.
        let m = env.account(&args.long_mint());
        assert_eq!(m.owner, TOKEN_PROGRAM);
        assert_eq!(m.data[0..4], [1, 0, 0, 0]);
        assert_eq!(&m.data[4..36], series.as_ref());
        assert_eq!(m.data[44], 0, "decimals");
        assert_eq!(m.data[46..50], [0, 0, 0, 0], "no freeze authority");
        // Vault: the series' USDC account, empty.
        assert_eq!(env.token_balance(&ata(&series, &env.usdc)), 0);
    }
}

#[test]
fn test_create_series_twice_is_rejected() {
    let mut env = Env::new();
    let args = SeriesArgs::xau_put(4_000);
    env.create_series(args);
    let payer = env.funded();
    expect_code(
        env.send(&[env.ix_create_series(&payer.pubkey(), args)], &[&payer]),
        code::ALREADY_INITIALIZED,
    );
}

#[test]
fn test_create_series_prefunded_address_still_succeeds() {
    let mut env = Env::new();
    let args = SeriesArgs::xau_put(4_000);
    // Anyone can send lamports to the future PDAs; creation must not be blocked by it.
    // The series gets less than its rent-exempt minimum (top-up path), the long mint more.
    let small = env.svm.minimum_balance_for_rent_exemption(0);
    let large = env.svm.minimum_balance_for_rent_exemption(1_000);
    env.svm.airdrop(&args.series(), small).unwrap();
    env.svm.airdrop(&args.long_mint(), large).unwrap();
    env.create_series(args);
    assert_eq!(env.account(&args.series()).owner, PROGRAM_ID);
}

#[test]
fn test_create_series_with_precreated_vault_succeeds() {
    let mut env = Env::new();
    let args = SeriesArgs::xau_put(4_000);
    // Anyone can create the series' associated USDC account before the listing.
    let usdc = env.usdc;
    let vault = env.token_account(&usdc, &args.series());
    assert_eq!(vault, ata(&args.series(), &usdc));
    env.create_series(args);
    assert_eq!(env.account(&args.series()).owner, PROGRAM_ID);
}

#[test]
fn test_create_series_forged_vault_with_delegate_or_close_authority_is_rejected() {
    // A vault at the right address but with a delegate or close authority set (bytes forged:
    // only the series could set them on a genuine account).
    for (tag_at, key_at) in [(72usize, 76usize), (129, 133)] {
        let mut env = Env::new();
        let args = SeriesArgs::xau_put(4_000);
        let usdc = env.usdc;
        let vault = env.token_account(&usdc, &args.series());
        let mut a = env.account(&vault);
        a.data[tag_at..tag_at + 4].copy_from_slice(&1u32.to_le_bytes());
        a.data[key_at..key_at + 32].copy_from_slice(Address::new_unique().as_ref());
        env.svm.set_account(vault, a).unwrap();
        let payer = env.funded();
        let ix = env.ix_create_series(&payer.pubkey(), args);
        expect_code(env.send(&[ix], &[&payer]), code::INVALID_VAULT);
    }
}

#[test]
fn test_create_series_terms_and_grids_are_checked() {
    let mut env = Env::new();
    let payer = env.funded();
    let cases: Vec<(SeriesArgs, u32)> = vec![
        (
            SeriesArgs {
                expiry: EXPIRY + 1,
                ..SeriesArgs::xau_put(4_000)
            },
            code::EXPIRY_NOT_IN_CALENDAR,
        ),
        // XAG's calendar has only EXPIRY.
        (
            SeriesArgs {
                underlying: XAG,
                multiplier: MULT_XAG,
                expiry: EXPIRY_2,
                ..SeriesArgs::xau_put(48)
            },
            code::EXPIRY_NOT_IN_CALENDAR,
        ),
        (
            SeriesArgs {
                strike: 4_001 * USD,
                ..SeriesArgs::xau_put(4_000)
            },
            code::STRIKE_OFF_GRID,
        ),
        (
            SeriesArgs {
                strike: 0,
                ..SeriesArgs::xau_put(4_000)
            },
            code::INVALID_STRIKE,
        ),
        (
            SeriesArgs {
                cap: 1,
                ..SeriesArgs::xau_put(4_000)
            },
            code::INVALID_CAP,
        ),
        (
            SeriesArgs {
                cap: 4_000 * USD,
                ..SeriesArgs::xau_call(4_000)
            },
            code::INVALID_CAP,
        ),
        (
            SeriesArgs {
                cap: 4_300 * USD,
                ..SeriesArgs::xau_call(4_000)
            },
            code::INVALID_CAP,
        ),
        (
            SeriesArgs {
                multiplier: 2_000,
                ..SeriesArgs::xau_put(4_000)
            },
            code::INVALID_MULTIPLIER,
        ),
        (
            SeriesArgs {
                underlying: 2,
                ..SeriesArgs::xau_put(4_000)
            },
            code::INVALID_INSTRUCTION_DATA,
        ),
        (
            SeriesArgs {
                kind: 2,
                ..SeriesArgs::xau_put(4_000)
            },
            code::INVALID_INSTRUCTION_DATA,
        ),
    ];
    for (args, code) in cases {
        let ix = env.ix_create_series(&payer.pubkey(), args);
        expect_code(env.send(&[ix], &[&payer]), code);
    }
    // Every cap on the grid is accepted.
    for bps in [1_000u64, 2_500, 5_000] {
        let args = SeriesArgs {
            cap: 4_000 * USD * (10_000 + bps) / 10_000,
            ..SeriesArgs::xau_call(4_000)
        };
        env.send(&[env.ix_create_series(&payer.pubkey(), args)], &[&payer])
            .unwrap();
    }
}

#[test]
fn test_create_series_listing_time_is_enforced() {
    let mut env = Env::new();
    let payer = env.funded();
    env.set_time(EXPIRY - i64::from(MIN_LISTING_S) + 1);
    let ix = env.ix_create_series(&payer.pubkey(), SeriesArgs::xau_put(4_000));
    expect_code(env.send(&[ix], &[&payer]), code::LISTING_TOO_LATE);
    env.set_time(EXPIRY - i64::from(MIN_LISTING_S));
    let ix = env.ix_create_series(&payer.pubkey(), SeriesArgs::xau_put(4_000));
    env.send(&[ix], &[&payer]).unwrap();
}

/// Replaces account `index` of a fresh `create_series` instruction and expects `code`.
fn create_with(env: &mut Env, index: usize, replacement: Address, code: u32) {
    let payer = env.funded();
    let mut ix = env.ix_create_series(&payer.pubkey(), SeriesArgs::xau_put(4_000));
    ix.accounts[index].pubkey = replacement;
    expect_code(env.send(&[ix], &[&payer]), code);
}

#[test]
fn test_create_series_forged_config_and_calendar_are_rejected() {
    let mut env = Env::new();
    let fake = Address::new_unique();
    env.forge(&config_address(), fake, Some(ATTACKER_PROGRAM));
    create_with(&mut env, 1, fake, code::INVALID_ACCOUNT_OWNER);
    env.forge(&config_address(), fake, None);
    create_with(&mut env, 1, fake, code::INVALID_PDA);

    let fake = Address::new_unique();
    env.forge(&calendar_address(XAU), fake, Some(ATTACKER_PROGRAM));
    create_with(&mut env, 2, fake, code::INVALID_ACCOUNT_OWNER);
    env.forge(&calendar_address(XAU), fake, None);
    create_with(&mut env, 2, fake, code::INVALID_PDA);
    // Genuine calendar of the other metal.
    create_with(&mut env, 2, calendar_address(XAG), code::INVALID_PDA);
}

#[test]
fn test_create_series_wrong_derived_accounts_are_rejected() {
    let mut env = Env::new();
    create_with(&mut env, 3, Address::new_unique(), code::INVALID_PDA);
    create_with(&mut env, 4, Address::new_unique(), code::INVALID_PDA);
    create_with(&mut env, 5, Address::new_unique(), code::INVALID_VAULT);
}

#[test]
fn test_create_series_wrong_mint_and_programs_are_rejected() {
    let mut env = Env::new();
    let admin = env.admin.insecure_clone();
    let other_mint = CreateMint::new(&mut env.svm, &admin)
        .decimals(6)
        .send()
        .unwrap();
    create_with(&mut env, 6, other_mint, code::INVALID_MINT);
    create_with(&mut env, 7, TOKEN_2022, code::INVALID_PROGRAM);
    create_with(&mut env, 8, TOKEN_PROGRAM, code::INVALID_PROGRAM);
    create_with(&mut env, 9, TOKEN_PROGRAM, code::INVALID_PROGRAM);
}

#[test]
fn test_create_series_unsigned_payer_is_rejected() {
    let mut env = Env::new();
    let payer = env.funded();
    let fee_payer = env.funded();
    let mut ix = env.ix_create_series(&payer.pubkey(), SeriesArgs::xau_put(4_000));
    ix.accounts[0].is_signer = false;
    expect_code(env.send(&[ix], &[&fee_payer]), code::MISSING_SIGNATURE);
}

// ---------------------------------------------------------------------------------------------
// mint and close
// ---------------------------------------------------------------------------------------------

/// A put series and a writer with 100_000 USDC and a long-token account.
fn with_writer(env: &mut Env, args: SeriesArgs) -> Keypair {
    env.create_series(args);
    let w = env.writer(100_000);
    env.long_account(&w.pubkey(), args);
    w
}

#[test]
fn test_mint_and_close_move_collateral_and_long_tokens() {
    let mut env = Env::new();
    for args in [SeriesArgs::xau_put(4_000), SeriesArgs::xau_call(4_000)] {
        let w = with_writer(&mut env, args);
        let series = args.series();
        let vault = ata(&series, &env.usdc);
        let usdc = ata(&w.pubkey(), &env.usdc);
        let longs = ata(&w.pubkey(), &args.long_mint());
        let max = args.max_payout();

        env.send(&[env.ix_mint(&w.pubkey(), args, 3)], &[&w])
            .unwrap();
        env.send(&[env.ix_mint(&w.pubkey(), args, 2)], &[&w])
            .unwrap();
        assert_eq!(env.token_balance(&vault), 5 * max);
        assert_eq!(env.token_balance(&usdc), 100_000 * USD - 5 * max);
        assert_eq!(env.token_balance(&longs), 5);
        assert_eq!(
            read_written(&env, &position_address(&series, &w.pubkey())),
            5
        );
        let v = read_series(&env, &series);
        assert_eq!(
            (
                v.open,
                v.longs_outstanding,
                v.writers_unclaimed,
                v.vault_owed
            ),
            (5, 5, 5, 5 * max)
        );

        env.send(&[env.ix_close(&w.pubkey(), args, 4)], &[&w])
            .unwrap();
        assert_eq!(env.token_balance(&vault), max);
        assert_eq!(env.token_balance(&usdc), 100_000 * USD - max);
        assert_eq!(env.token_balance(&longs), 1);
        assert_eq!(
            read_written(&env, &position_address(&series, &w.pubkey())),
            1
        );
        let v = read_series(&env, &series);
        assert_eq!((v.open, v.vault_owed, v.state), (1, max, 0));
    }
}

#[test]
fn test_mint_xag_uses_multiplier() {
    let mut env = Env::new();
    let args = SeriesArgs {
        underlying: XAG,
        kind: PUT,
        strike: 48 * USD,
        expiry: EXPIRY,
        cap: 0,
        multiplier: MULT_XAG,
    };
    let w = with_writer(&mut env, args);
    env.send(&[env.ix_mint(&w.pubkey(), args, 2)], &[&w])
        .unwrap();
    // 48 USD × 50 oz × 2.
    assert_eq!(
        env.token_balance(&ata(&args.series(), &env.usdc)),
        4_800 * USD
    );
}

#[test]
fn test_mint_and_close_zero_and_unsigned_are_rejected() {
    let mut env = Env::new();
    let args = SeriesArgs::xau_put(4_000);
    let w = with_writer(&mut env, args);
    expect_code(
        env.send(&[env.ix_mint(&w.pubkey(), args, 0)], &[&w]),
        code::ZERO_AMOUNT,
    );
    expect_code(
        env.send(&[env.ix_close(&w.pubkey(), args, 0)], &[&w]),
        code::ZERO_AMOUNT,
    );
    let fee_payer = env.funded();
    for tag in [4u8, 5] {
        let mut ix = env.ix_position(tag, &w.pubkey(), args, 1);
        ix.accounts[0].is_signer = false;
        expect_code(env.send(&[ix], &[&fee_payer]), code::MISSING_SIGNATURE);
    }
}

#[test]
fn test_mint_and_close_after_expiry_are_rejected() {
    let mut env = Env::new();
    let args = SeriesArgs::xau_put(4_000);
    let w = with_writer(&mut env, args);
    env.send(&[env.ix_mint(&w.pubkey(), args, 1)], &[&w])
        .unwrap();
    env.set_time(EXPIRY - 1);
    env.send(&[env.ix_mint(&w.pubkey(), args, 1)], &[&w])
        .unwrap();
    env.set_time(EXPIRY);
    expect_code(
        env.send(&[env.ix_mint(&w.pubkey(), args, 1)], &[&w]),
        code::SERIES_EXPIRED,
    );
    expect_code(
        env.send(&[env.ix_close(&w.pubkey(), args, 1)], &[&w]),
        code::SERIES_EXPIRED,
    );
}

#[test]
fn test_mint_forged_series_is_rejected() {
    let mut env = Env::new();
    let args = SeriesArgs::xau_put(4_000);
    let w = with_writer(&mut env, args);
    let fake = Address::new_unique();
    env.forge(&args.series(), fake, Some(ATTACKER_PROGRAM));
    for tag in [4u8, 5] {
        let mut ix = env.ix_position(tag, &w.pubkey(), args, 1);
        ix.accounts[1].pubkey = fake;
        expect_code(env.send(&[ix], &[&w]), code::INVALID_ACCOUNT_OWNER);
    }
    env.forge(&args.series(), fake, None);
    for tag in [4u8, 5] {
        let mut ix = env.ix_position(tag, &w.pubkey(), args, 1);
        ix.accounts[1].pubkey = fake;
        expect_code(env.send(&[ix], &[&w]), code::INVALID_PDA);
    }
    // A config account passed as a series: owned by the program, wrong layout.
    let mut ix = env.ix_mint(&w.pubkey(), args, 1);
    ix.accounts[1].pubkey = config_address();
    expect_code(env.send(&[ix], &[&w]), code::INVALID_ACCOUNT_DATA);
}

/// Replaces account `index` of a fresh `mint` and `close` and expects `code` from both.
fn position_with(
    env: &mut Env,
    w: &Keypair,
    args: SeriesArgs,
    index: usize,
    replacement: Address,
    code: u32,
) {
    for tag in [4u8, 5] {
        let mut ix = env.ix_position(tag, &w.pubkey(), args, 1);
        ix.accounts[index].pubkey = replacement;
        expect_code(env.send(&[ix], &[w]), code);
    }
}

#[test]
fn test_mint_and_close_wrong_vault_mints_and_program_are_rejected() {
    let mut env = Env::new();
    let args = SeriesArgs::xau_put(4_000);
    let w = with_writer(&mut env, args);
    env.send(&[env.ix_mint(&w.pubkey(), args, 2)], &[&w])
        .unwrap();
    // Another series' genuine vault.
    let other = SeriesArgs::xau_put(3_000);
    let other_series = env.create_series(other);
    let usdc = env.usdc;
    position_with(
        &mut env,
        &w,
        args,
        4,
        ata(&other_series, &usdc),
        code::INVALID_VAULT,
    );
    let usdc = env.usdc;
    position_with(
        &mut env,
        &w,
        args,
        4,
        ata(&w.pubkey(), &usdc),
        code::INVALID_VAULT,
    );
    // Another mint as USDC, another series' long mint as long mint.
    let admin = env.admin.insecure_clone();
    let fake_usdc = CreateMint::new(&mut env.svm, &admin)
        .decimals(6)
        .send()
        .unwrap();
    position_with(&mut env, &w, args, 5, fake_usdc, code::INVALID_MINT);
    position_with(&mut env, &w, args, 6, other.long_mint(), code::INVALID_MINT);
    position_with(&mut env, &w, args, 8, TOKEN_2022, code::INVALID_PROGRAM);
    let mut ix = env.ix_mint(&w.pubkey(), args, 1);
    ix.accounts[9].pubkey = TOKEN_PROGRAM;
    expect_code(env.send(&[ix], &[&w]), code::INVALID_PROGRAM);
}

#[test]
fn test_mint_and_close_wrong_token_accounts_are_rejected() {
    let mut env = Env::new();
    let args = SeriesArgs::xau_put(4_000);
    let w = with_writer(&mut env, args);
    env.send(&[env.ix_mint(&w.pubkey(), args, 2)], &[&w])
        .unwrap();
    let other = env.writer(1_000);
    env.long_account(&other.pubkey(), args);
    let usdc = env.usdc;
    // USDC account owned by someone else, or of another mint.
    position_with(
        &mut env,
        &w,
        args,
        3,
        ata(&other.pubkey(), &usdc),
        code::INVALID_TOKEN_ACCOUNT,
    );
    position_with(
        &mut env,
        &w,
        args,
        3,
        ata(&w.pubkey(), &args.long_mint()),
        code::INVALID_TOKEN_ACCOUNT,
    );
    // Long account owned by someone else (close would burn from it), or of another mint.
    position_with(
        &mut env,
        &w,
        args,
        7,
        ata(&other.pubkey(), &args.long_mint()),
        code::INVALID_TOKEN_ACCOUNT,
    );
    position_with(
        &mut env,
        &w,
        args,
        7,
        ata(&w.pubkey(), &usdc),
        code::INVALID_TOKEN_ACCOUNT,
    );
    // A forged token account: the writer's own USDC account bytes under another owner.
    let fake = Address::new_unique();
    env.forge(&ata(&w.pubkey(), &usdc), fake, Some(ATTACKER_PROGRAM));
    position_with(&mut env, &w, args, 3, fake, code::INVALID_ACCOUNT_OWNER);
}

#[test]
fn test_mint_and_close_other_writers_position_is_rejected() {
    let mut env = Env::new();
    let args = SeriesArgs::xau_put(4_000);
    let w = with_writer(&mut env, args);
    let other = env.writer(100_000);
    env.long_account(&other.pubkey(), args);
    env.send(&[env.ix_mint(&other.pubkey(), args, 2)], &[&other])
        .unwrap();
    let theirs = position_address(&args.series(), &other.pubkey());
    // Existing position of another writer, for both instructions.
    position_with(&mut env, &w, args, 2, theirs, code::INVALID_PDA);
    // A new, wrong address for a first mint.
    let mut ix = env.ix_mint(&w.pubkey(), args, 1);
    ix.accounts[2].pubkey = Address::new_unique();
    expect_code(env.send(&[ix], &[&w]), code::INVALID_PDA);
    // A forged position: their bytes at another address.
    let fake = Address::new_unique();
    env.forge(&theirs, fake, None);
    position_with(&mut env, &w, args, 2, fake, code::INVALID_PDA);
    env.forge(&theirs, fake, Some(ATTACKER_PROGRAM));
    let mut ix = env.ix_close(&w.pubkey(), args, 1);
    ix.accounts[2].pubkey = fake;
    expect_code(env.send(&[ix], &[&w]), code::INVALID_ACCOUNT_OWNER);
}

#[test]
fn test_close_cannot_exceed_own_written() {
    let mut env = Env::new();
    let args = SeriesArgs::xau_put(4_000);
    let a = with_writer(&mut env, args);
    let b = env.writer(100_000);
    let b_longs = env.long_account(&b.pubkey(), args);
    env.send(&[env.ix_mint(&a.pubkey(), args, 1)], &[&a])
        .unwrap();
    env.send(&[env.ix_mint(&b.pubkey(), args, 2)], &[&b])
        .unwrap();
    // A buys B's long tokens: A holds 3 but wrote 1.
    let a_longs = ata(&a.pubkey(), &args.long_mint());
    litesvm_token::Transfer::new(&mut env.svm, &b, &args.long_mint(), &a_longs, 2)
        .source(&b_longs)
        .owner(&b)
        .send()
        .unwrap();
    assert_eq!(env.token_balance(&a_longs), 3);
    expect_code(
        env.send(&[env.ix_close(&a.pubkey(), args, 2)], &[&a]),
        code::INSUFFICIENT_WRITTEN,
    );
    env.send(&[env.ix_close(&a.pubkey(), args, 1)], &[&a])
        .unwrap();
    // A writer who never wrote has no position.
    let c = env.writer(10);
    env.long_account(&c.pubkey(), args);
    expect_code(
        env.send(&[env.ix_close(&c.pubkey(), args, 1)], &[&c]),
        code::INSUFFICIENT_WRITTEN,
    );
}

#[test]
fn test_mint_without_enough_usdc_fails_without_state_change() {
    let mut env = Env::new();
    let args = SeriesArgs::xau_put(4_000);
    let w = with_writer(&mut env, args);
    let before = read_series(&env, &args.series());
    // 100_000 USDC covers 25 contracts of 4_000; 26 fails in the token program.
    assert!(env
        .send(&[env.ix_mint(&w.pubkey(), args, 26)], &[&w])
        .is_err());
    assert_eq!(read_series(&env, &args.series()), before);
    env.send(&[env.ix_mint(&w.pubkey(), args, 25)], &[&w])
        .unwrap();
}

#[test]
fn test_read_only_accounts_are_rejected() {
    let mut env = Env::new();
    let args = SeriesArgs::xau_put(4_000);
    let w = with_writer(&mut env, args);
    for index in [1usize, 2, 3, 4, 6, 7] {
        let mut ix = env.ix_mint(&w.pubkey(), args, 1);
        ix.accounts[index].is_writable = false;
        expect_code(env.send(&[ix], &[&w]), code::ACCOUNT_NOT_WRITABLE);
    }
}
