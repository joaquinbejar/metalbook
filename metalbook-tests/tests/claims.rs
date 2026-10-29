//! LiteSVM tests of milestone 4: `redeem` and `claim_writer`. End to end for both kinds, both
//! metals, in, at and out of the money (and a call above its cap): the vault ends at exactly
//! zero, every holder gets `n × payout`, every writer `written × (max_payout − payout)`, and
//! with a multiplier that is not a whole number of ounces the writer keeps the rounding dust.
//! Every account rule has a negative test, most with a forged account. Run `make program`
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
use litesvm_token::Transfer;
use solana_address::Address;
use solana_keypair::Keypair;
use solana_signer::Signer;

const ATTACKER_PROGRAM: Address = Address::new_from_array([0xaa; 32]);

/// Settlement price in atoms from the 30 raw slot mantissas: mean, rounded to nearest (ties
/// up), with the underlying's exponent. Restated from `docs/PLAN.md`.
fn settlement_atoms(underlying: u8, prices: &[i64; 30]) -> u64 {
    let sum: i128 = prices.iter().map(|&p| i128::from(p)).sum();
    let (num, den) = if underlying == XAU {
        (sum * 1_000, 30) // 10^(6 - 3)
    } else {
        (sum * 10, 30) // 10^(6 - 5)
    };
    let q = num / den;
    (if 2 * (num % den) >= den { q + 1 } else { q }) as u64
}

/// Payout per contract (rounded down) and maximum payout (rounded up), restated.
fn expected(args: SeriesArgs, mult: u32, s: u64) -> (u64, u64) {
    let (payoff, bound) = if args.kind == PUT {
        (args.strike.saturating_sub(s), args.strike)
    } else {
        (
            s.saturating_sub(args.strike).min(args.cap - args.strike),
            args.cap - args.strike,
        )
    };
    let m = u128::from(mult);
    let payout = (u128::from(payoff) * m / 1_000) as u64;
    let max = (u128::from(bound) * m).div_ceil(1_000) as u64;
    (payout, max)
}

struct Actors {
    a: Keypair,
    b: Keypair,
    h1: Keypair,
    h2: Keypair,
}

/// Lists `args`, has writer A write 5 and close 1 and writer B write 3, and moves 2 of A's
/// long tokens to holder H1 and 3 of B's to H2 (A keeps 2, B none).
fn open_positions(env: &mut Env, args: SeriesArgs) -> Actors {
    env.create_series(args);
    let a = env.writer(10_000_000);
    let b = env.writer(10_000_000);
    let h1 = env.writer(0);
    let h2 = env.writer(0);
    for k in [&a, &b, &h1, &h2] {
        env.long_account(&k.pubkey(), args);
    }
    env.send(&[env.ix_mint(&a.pubkey(), args, 5)], &[&a])
        .unwrap();
    env.send(&[env.ix_close(&a.pubkey(), args, 1)], &[&a])
        .unwrap();
    env.send(&[env.ix_mint(&b.pubkey(), args, 3)], &[&b])
        .unwrap();
    let mint = args.long_mint();
    for (from, to, n) in [(&a, &h1, 2u64), (&b, &h2, 3)] {
        let dest = ata(&to.pubkey(), &mint);
        Transfer::new(&mut env.svm, from, &mint, &dest, n)
            .source(&ata(&from.pubkey(), &mint))
            .owner(from)
            .send()
            .unwrap();
    }
    Actors { a, b, h1, h2 }
}

/// Full lifecycle with settlement prices `prices`; claims before or after the redeems.
fn lifecycle(env: &mut Env, args: SeriesArgs, mult: u32, prices: [i64; 30], claims_first: bool) {
    let series = args.series();
    let usdc = env.usdc;
    let vault = ata(&series, &usdc);
    let x = open_positions(env, args);
    let balance = |env: &Env, k: &Keypair| env.token_balance(&ata(&k.pubkey(), &usdc));
    let before: Vec<u64> = [&x.a, &x.b, &x.h1, &x.h2]
        .iter()
        .map(|k| balance(env, k))
        .collect();
    let deposited = env.token_balance(&vault);

    env.settle_with_window(args, &prices);
    let (payout, max) = expected(args, mult, settlement_atoms(args.underlying, &prices));
    let residual = max - payout;
    let v = read_settlement(env, &series);
    assert_eq!((v.payout, v.residual), (payout, residual), "{args:?}");
    assert_eq!(deposited, 7 * max);

    let redeems = [(&x.h2, 1u64), (&x.h2, 2), (&x.h1, 2), (&x.a, 2)];
    let claims = [&x.a, &x.b];
    let run_redeems = |env: &mut Env| {
        for (k, n) in redeems {
            env.send(&[env.ix_redeem(&k.pubkey(), args, n)], &[k])
                .unwrap();
        }
    };
    if !claims_first {
        run_redeems(env);
    }
    for k in claims {
        env.send(&[env.ix_claim(&k.pubkey(), args)], &[k]).unwrap();
    }
    if claims_first {
        run_redeems(env);
    }

    // The vault is empty and the accounting is closed.
    assert_eq!(env.token_balance(&vault), 0, "{args:?}");
    let s = read_series(env, &series);
    assert_eq!(
        (s.longs_outstanding, s.writers_unclaimed, s.vault_owed),
        (0, 0, 0)
    );
    // Everyone got exactly their share.
    let after: Vec<u64> = [&x.a, &x.b, &x.h1, &x.h2]
        .iter()
        .map(|k| balance(env, k))
        .collect();
    assert_eq!(after[0] - before[0], 2 * payout + 4 * residual, "writer A");
    assert_eq!(after[1] - before[1], 3 * residual, "writer B");
    assert_eq!(after[2] - before[2], 2 * payout, "holder H1");
    assert_eq!(after[3] - before[3], 3 * payout, "holder H2");
    assert_eq!(
        after.iter().sum::<u64>() - before.iter().sum::<u64>(),
        deposited
    );
    // Long tokens are burned.
    for k in [&x.a, &x.b, &x.h1, &x.h2] {
        assert_eq!(env.token_balance(&ata(&k.pubkey(), &args.long_mint())), 0);
    }
}

/// Raw mantissa of `usd_milli / 1000` USD for the underlying's exponent.
fn raw(underlying: u8, usd_milli: i64) -> i64 {
    if underlying == XAU {
        usd_milli
    } else {
        usd_milli * 100
    }
}

#[test]
fn test_end_to_end_both_kinds_both_metals_itm_atm_otm() {
    let xau_put = SeriesArgs::xau_put(4_000);
    let xau_call = SeriesArgs::xau_call(4_000);
    let xag_put = SeriesArgs {
        underlying: XAG,
        multiplier: MULT_XAG,
        ..SeriesArgs::xau_put(48)
    };
    let xag_call = SeriesArgs {
        underlying: XAG,
        multiplier: MULT_XAG,
        ..SeriesArgs::xau_call(48)
    };
    // (series, multiplier, settlement in milli-USD per ounce, case)
    let cases = [
        (xau_put, MULT_XAU, 3_600_000, "put ITM"),
        (xau_put, MULT_XAU, 4_000_000, "put ATM"),
        (xau_put, MULT_XAU, 4_400_000, "put OTM"),
        (xau_call, MULT_XAU, 4_200_000, "call ITM"),
        (xau_call, MULT_XAU, 4_800_000, "call above cap"),
        (xau_call, MULT_XAU, 4_000_000, "call ATM"),
        (xau_call, MULT_XAU, 3_600_000, "call OTM"),
        (xag_put, MULT_XAG, 43_200, "put ITM"),
        (xag_put, MULT_XAG, 48_000, "put ATM"),
        (xag_put, MULT_XAG, 52_800, "put OTM"),
        (xag_call, MULT_XAG, 50_400, "call ITM"),
        (xag_call, MULT_XAG, 57_600, "call above cap"),
        (xag_call, MULT_XAG, 48_000, "call ATM"),
        (xag_call, MULT_XAG, 43_200, "call OTM"),
    ];
    for (i, (args, mult, milli, case)) in cases.into_iter().enumerate() {
        let mut env = Env::new();
        let prices = [raw(args.underlying, milli); 30];
        lifecycle(&mut env, args, mult, prices, i % 2 == 0);
        let v = read_settlement(&env, &args.series());
        match case {
            "put ATM" | "put OTM" | "call ATM" | "call OTM" => assert_eq!(v.payout, 0, "{case}"),
            "call above cap" => assert_eq!(v.residual, 0, "{case}"),
            _ => assert!(v.payout > 0 && v.residual > 0, "{case}"),
        }
    }
}

#[test]
fn test_rounding_dust_goes_to_the_writer() {
    // 1.333 oz per contract: collateral and payout are not whole atoms.
    let mut p = Params::default_params();
    p.multiplier[0] = 1_333;
    let mut env = Env::with_params(p);
    let args = SeriesArgs {
        multiplier: 1_333,
        ..SeriesArgs::xau_put(4_000)
    };
    // A mean of 3995.000033... USD: 29 slots at 3995.000 and one at 3995.001.
    let mut prices = [3_995_000i64; 30];
    prices[7] = 3_995_001;
    let s = settlement_atoms(XAU, &prices);
    assert_eq!(s, 3_995_000_033);
    let (payout, max) = expected(args, 1_333, s);
    // Exact payout 6_664_956.011 atoms, paid 6_664_956: the 0.011 stays with the writer.
    assert_eq!((payout, max), (6_664_956, 5_332_000_000));
    lifecycle(&mut env, args, 1_333, prices, false);
}

#[test]
fn test_void_returns_all_collateral_to_writers_and_longs_get_nothing() {
    let mut env = Env::new();
    let args = SeriesArgs::xau_put(4_000);
    let x = open_positions(&mut env, args);
    let series = args.series();
    env.set_time(EXPIRY + HARD_S + D);
    env.settle(&series).unwrap();
    expect_code(
        env.send(&[env.ix_redeem(&x.h1.pubkey(), args, 1)], &[&x.h1]),
        code::SERIES_NOT_SETTLED,
    );
    let usdc = env.usdc;
    let a0 = env.token_balance(&ata(&x.a.pubkey(), &usdc));
    let b0 = env.token_balance(&ata(&x.b.pubkey(), &usdc));
    env.send(&[env.ix_claim(&x.a.pubkey(), args)], &[&x.a])
        .unwrap();
    env.send(&[env.ix_claim(&x.b.pubkey(), args)], &[&x.b])
        .unwrap();
    assert_eq!(
        env.token_balance(&ata(&x.a.pubkey(), &usdc)) - a0,
        4 * 4_000 * USD
    );
    assert_eq!(
        env.token_balance(&ata(&x.b.pubkey(), &usdc)) - b0,
        3 * 4_000 * USD
    );
    assert_eq!(env.token_balance(&ata(&series, &usdc)), 0);
}

/// A settled put series (S = 3600) with the standard positions.
fn settled() -> (Env, SeriesArgs, Actors) {
    let mut env = Env::new();
    let args = SeriesArgs::xau_put(4_000);
    let x = open_positions(&mut env, args);
    env.settle_with_window(args, &[3_600_000; 30]);
    (env, args, x)
}

#[test]
fn test_redeem_and_claim_before_settlement_are_rejected() {
    let mut env = Env::new();
    let args = SeriesArgs::xau_put(4_000);
    let x = open_positions(&mut env, args);
    expect_code(
        env.send(&[env.ix_redeem(&x.h1.pubkey(), args, 1)], &[&x.h1]),
        code::SERIES_NOT_SETTLED,
    );
    expect_code(
        env.send(&[env.ix_claim(&x.a.pubkey(), args)], &[&x.a]),
        code::SERIES_NOT_SETTLED,
    );
    // Expired but not settled.
    env.set_time(EXPIRY + 1);
    expect_code(
        env.send(&[env.ix_redeem(&x.h1.pubkey(), args, 1)], &[&x.h1]),
        code::SERIES_NOT_SETTLED,
    );
    expect_code(
        env.send(&[env.ix_claim(&x.a.pubkey(), args)], &[&x.a]),
        code::SERIES_NOT_SETTLED,
    );
}

#[test]
fn test_claim_once_and_only_own_position() {
    let (mut env, args, x) = settled();
    env.send(&[env.ix_claim(&x.a.pubkey(), args)], &[&x.a])
        .unwrap();
    expect_code(
        env.send(&[env.ix_claim(&x.a.pubkey(), args)], &[&x.a]),
        code::ALREADY_CLAIMED,
    );
    // B's genuine position passed by A.
    let theirs = position_address(&args.series(), &x.b.pubkey());
    let mut ix = env.ix_claim(&x.a.pubkey(), args);
    ix.accounts[2].pubkey = theirs;
    expect_code(env.send(&[ix], &[&x.a]), code::INVALID_PDA);
    // B's position bytes at the genuine address of B, under another owner.
    env.forge(&theirs, theirs, Some(ATTACKER_PROGRAM));
    expect_code(
        env.send(&[env.ix_claim(&x.b.pubkey(), args)], &[&x.b]),
        code::INVALID_ACCOUNT_OWNER,
    );
    // A holder who never wrote has no position.
    expect_code(
        env.send(&[env.ix_claim(&x.h1.pubkey(), args)], &[&x.h1]),
        code::INSUFFICIENT_WRITTEN,
    );
}

#[test]
fn test_claim_with_everything_closed_is_rejected() {
    let mut env = Env::new();
    let args = SeriesArgs::xau_put(4_000);
    env.create_series(args);
    let w = env.writer(100_000);
    env.long_account(&w.pubkey(), args);
    env.send(&[env.ix_mint(&w.pubkey(), args, 2)], &[&w])
        .unwrap();
    env.send(&[env.ix_close(&w.pubkey(), args, 2)], &[&w])
        .unwrap();
    env.set_time(EXPIRY + HARD_S + D);
    env.settle(&args.series()).unwrap();
    expect_code(
        env.send(&[env.ix_claim(&w.pubkey(), args)], &[&w]),
        code::ZERO_AMOUNT,
    );
}

#[test]
fn test_redeem_limits_and_signer() {
    let (mut env, args, x) = settled();
    expect_code(
        env.send(&[env.ix_redeem(&x.h1.pubkey(), args, 0)], &[&x.h1]),
        code::ZERO_AMOUNT,
    );
    // More than the series has outstanding.
    expect_code(
        env.send(&[env.ix_redeem(&x.h1.pubkey(), args, 8)], &[&x.h1]),
        code::INSUFFICIENT_LONG_TOKENS,
    );
    // More than the holder has: the burn fails and nothing changes.
    let before = read_series(&env, &args.series());
    assert!(env
        .send(&[env.ix_redeem(&x.h1.pubkey(), args, 3)], &[&x.h1])
        .is_err());
    assert_eq!(read_series(&env, &args.series()), before);
    // Unsigned holder.
    let payer = env.funded();
    let mut ix = env.ix_redeem(&x.h1.pubkey(), args, 1);
    ix.accounts[0].is_signer = false;
    expect_code(env.send(&[ix], &[&payer]), code::MISSING_SIGNATURE);
    let mut ix = env.ix_claim(&x.a.pubkey(), args);
    ix.accounts[0].is_signer = false;
    expect_code(env.send(&[ix], &[&payer]), code::MISSING_SIGNATURE);
}

#[test]
fn test_redeem_and_claim_token_accounts_must_be_the_signers_own() {
    let (mut env, args, x) = settled();
    let usdc = env.usdc;
    let mint = args.long_mint();
    // Burning someone else's long tokens, or paying to someone else's USDC account.
    let cases = [
        (
            2usize,
            ata(&x.h2.pubkey(), &mint),
            code::INVALID_TOKEN_ACCOUNT,
        ),
        (2, ata(&x.h1.pubkey(), &usdc), code::INVALID_TOKEN_ACCOUNT),
        (3, ata(&x.h2.pubkey(), &usdc), code::INVALID_TOKEN_ACCOUNT),
        (3, ata(&x.h1.pubkey(), &mint), code::INVALID_TOKEN_ACCOUNT),
    ];
    for (index, replacement, code) in cases {
        let mut ix = env.ix_redeem(&x.h1.pubkey(), args, 1);
        ix.accounts[index].pubkey = replacement;
        expect_code(env.send(&[ix], &[&x.h1]), code);
    }
    let mut ix = env.ix_claim(&x.a.pubkey(), args);
    ix.accounts[3].pubkey = ata(&x.b.pubkey(), &usdc);
    expect_code(env.send(&[ix], &[&x.a]), code::INVALID_TOKEN_ACCOUNT);
    // A forged USDC account: the holder's own bytes under another owner.
    let fake = Address::new_unique();
    env.forge(&ata(&x.h1.pubkey(), &usdc), fake, Some(ATTACKER_PROGRAM));
    let mut ix = env.ix_redeem(&x.h1.pubkey(), args, 1);
    ix.accounts[3].pubkey = fake;
    expect_code(env.send(&[ix], &[&x.h1]), code::INVALID_ACCOUNT_OWNER);
    let mut ix = env.ix_claim(&x.a.pubkey(), args);
    ix.accounts[3].pubkey = fake;
    expect_code(env.send(&[ix], &[&x.a]), code::INVALID_ACCOUNT_OWNER);
}

#[test]
fn test_redeem_and_claim_series_vault_mints_and_program_are_checked() {
    let mut env = Env::new();
    let args = SeriesArgs::xau_put(4_000);
    let x = open_positions(&mut env, args);
    // A second genuine series, listed before the clock passes expiry.
    let other = SeriesArgs::xau_put(3_000);
    env.create_series(other);
    env.settle_with_window(args, &[3_600_000; 30]);
    let usdc = env.usdc;
    // Forged series: genuine bytes under another owner, then at another address.
    let fake = Address::new_unique();
    env.forge(&args.series(), fake, Some(ATTACKER_PROGRAM));
    let mut r = env.ix_redeem(&x.h1.pubkey(), args, 1);
    r.accounts[1].pubkey = fake;
    expect_code(env.send(&[r], &[&x.h1]), code::INVALID_ACCOUNT_OWNER);
    env.forge(&args.series(), fake, None);
    let mut r = env.ix_redeem(&x.h1.pubkey(), args, 1);
    r.accounts[1].pubkey = fake;
    expect_code(env.send(&[r], &[&x.h1]), code::INVALID_PDA);
    let mut c = env.ix_claim(&x.a.pubkey(), args);
    c.accounts[1].pubkey = fake;
    expect_code(env.send(&[c], &[&x.a]), code::INVALID_PDA);
    // Another series' genuine vault and long mint.
    let other_vault = ata(&other.series(), &usdc);
    let mut r = env.ix_redeem(&x.h1.pubkey(), args, 1);
    r.accounts[4].pubkey = other_vault;
    expect_code(env.send(&[r], &[&x.h1]), code::INVALID_VAULT);
    let mut c = env.ix_claim(&x.a.pubkey(), args);
    c.accounts[4].pubkey = other_vault;
    expect_code(env.send(&[c], &[&x.a]), code::INVALID_VAULT);
    let mut r = env.ix_redeem(&x.h1.pubkey(), args, 1);
    r.accounts[6].pubkey = other.long_mint();
    expect_code(env.send(&[r], &[&x.h1]), code::INVALID_MINT);
    // Another mint as USDC; Token-2022 as the token program.
    let admin = env.admin.insecure_clone();
    let fake_usdc = litesvm_token::CreateMint::new(&mut env.svm, &admin)
        .decimals(6)
        .send()
        .unwrap();
    let mut r = env.ix_redeem(&x.h1.pubkey(), args, 1);
    r.accounts[5].pubkey = fake_usdc;
    expect_code(env.send(&[r], &[&x.h1]), code::INVALID_MINT);
    let mut c = env.ix_claim(&x.a.pubkey(), args);
    c.accounts[5].pubkey = fake_usdc;
    expect_code(env.send(&[c], &[&x.a]), code::INVALID_MINT);
    let mut r = env.ix_redeem(&x.h1.pubkey(), args, 1);
    r.accounts[7].pubkey = TOKEN_2022;
    expect_code(env.send(&[r], &[&x.h1]), code::INVALID_PROGRAM);
    let mut c = env.ix_claim(&x.a.pubkey(), args);
    c.accounts[6].pubkey = TOKEN_2022;
    expect_code(env.send(&[c], &[&x.a]), code::INVALID_PROGRAM);
    // Read-only accounts.
    for index in [1usize, 2, 3, 4, 6] {
        let mut r = env.ix_redeem(&x.h1.pubkey(), args, 1);
        r.accounts[index].is_writable = false;
        expect_code(env.send(&[r], &[&x.h1]), code::ACCOUNT_NOT_WRITABLE);
    }
    for index in [1usize, 2, 3, 4] {
        let mut c = env.ix_claim(&x.a.pubkey(), args);
        c.accounts[index].is_writable = false;
        expect_code(env.send(&[c], &[&x.a]), code::ACCOUNT_NOT_WRITABLE);
    }
}
