//! LiteSVM tests of the close-when-empty instructions (`docs/PLAN.md` 4.6, open question 7
//! decided): `close_series` and `close_position` return rent once nothing is owed, owed
//! amounts come from the ledger, a donation to the vault cannot block the close, and every
//! account rule has a negative test with a forged account. Run `make program` first.

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
use litesvm_token::MintTo;
use solana_address::Address;
use solana_keypair::Keypair;
use solana_signer::Signer;

const ATTACKER_PROGRAM: Address = Address::new_from_array([0xaa; 32]);

struct Listed {
    env: Env,
    args: SeriesArgs,
    payer: Keypair,
    writer: Keypair,
}

/// A put listed by `payer` (who also has a USDC account), one writer with 2 contracts, settled
/// in the money at 3600.
fn settled() -> Listed {
    let mut env = Env::new();
    let args = SeriesArgs::xau_put(4_000);
    let payer = env.funded();
    env.send(&[env.ix_create_series(&payer.pubkey(), args)], &[&payer])
        .unwrap();
    let usdc = env.usdc;
    env.token_account(&usdc, &payer.pubkey());
    let writer = env.writer(100_000);
    env.long_account(&writer.pubkey(), args);
    env.send(&[env.ix_mint(&writer.pubkey(), args, 2)], &[&writer])
        .unwrap();
    env.settle_with_window(args, &[3_600_000; 30]);
    Listed {
        env,
        args,
        payer,
        writer,
    }
}

/// Redeems every long token and claims the writer: nothing owed afterwards.
fn pay_everyone(l: &mut Listed) {
    let (w, args) = (l.writer.insecure_clone(), l.args);
    l.env
        .send(&[l.env.ix_redeem(&w.pubkey(), args, 2)], &[&w])
        .unwrap();
    l.env
        .send(&[l.env.ix_claim(&w.pubkey(), args)], &[&w])
        .unwrap();
}

fn close(l: &mut Listed) -> Sent {
    let ix = l.env.ix_close_series(l.args, &l.payer.pubkey());
    let admin = l.env.admin.insecure_clone();
    l.env.send(&[ix], &[&admin])
}

fn lamports(env: &Env, a: &Address) -> u64 {
    env.svm.get_account(a).map_or(0, |acc| acc.lamports)
}

#[test]
fn test_close_series_returns_rent_to_the_payer() {
    let mut l = settled();
    pay_everyone(&mut l);
    let series = l.args.series();
    let vault = ata(&series, &l.env.usdc);
    let rent = lamports(&l.env, &series) + lamports(&l.env, &vault);
    let before = lamports(&l.env, &l.payer.pubkey());
    close(&mut l).unwrap();
    assert_eq!(lamports(&l.env, &l.payer.pubkey()) - before, rent);
    assert_eq!(lamports(&l.env, &series), 0);
    assert_eq!(lamports(&l.env, &vault), 0);
    // The SPL Token long mint cannot be closed and stays.
    assert!(lamports(&l.env, &l.args.long_mint()) > 0);
    // Closed means closed: a second close and late instructions find no series.
    assert!(close(&mut l).is_err());
}

#[test]
fn test_one_atom_donation_cannot_block_close_and_goes_to_the_payer() {
    let mut l = settled();
    pay_everyone(&mut l);
    let usdc = l.env.usdc;
    let vault = ata(&l.args.series(), &usdc);
    let admin = l.env.admin.insecure_clone();
    MintTo::new(&mut l.env.svm, &admin, &usdc, &vault, 1)
        .send()
        .unwrap();
    let payer_usdc = ata(&l.payer.pubkey(), &usdc);
    let before = l.env.token_balance(&payer_usdc);
    close(&mut l).unwrap();
    assert_eq!(l.env.token_balance(&payer_usdc) - before, 1);
}

#[test]
fn test_close_is_rejected_while_anything_is_owed() {
    let mut l = settled();
    let (w, args) = (l.writer.insecure_clone(), l.args);
    // Nothing paid yet.
    expect_code(close(&mut l), code::SERIES_NOT_EMPTY);
    // One long token still unredeemed, the writer claimed.
    l.env
        .send(&[l.env.ix_redeem(&w.pubkey(), args, 1)], &[&w])
        .unwrap();
    l.env
        .send(&[l.env.ix_claim(&w.pubkey(), args)], &[&w])
        .unwrap();
    expect_code(close(&mut l), code::SERIES_NOT_EMPTY);
    l.env
        .send(&[l.env.ix_redeem(&w.pubkey(), args, 1)], &[&w])
        .unwrap();
    close(&mut l).unwrap();

    // Every long redeemed but the writer unclaimed.
    let mut l = settled();
    let (w, args) = (l.writer.insecure_clone(), l.args);
    l.env
        .send(&[l.env.ix_redeem(&w.pubkey(), args, 2)], &[&w])
        .unwrap();
    expect_code(close(&mut l), code::SERIES_NOT_EMPTY);
}

#[test]
fn test_close_before_settlement_is_rejected() {
    let mut env = Env::new();
    let args = SeriesArgs::xau_put(4_000);
    let payer = env.funded();
    env.send(&[env.ix_create_series(&payer.pubkey(), args)], &[&payer])
        .unwrap();
    let usdc = env.usdc;
    env.token_account(&usdc, &payer.pubkey());
    let ix = env.ix_close_series(args, &payer.pubkey());
    expect_code(
        env.send(std::slice::from_ref(&ix), &[&payer]),
        code::SERIES_NOT_SETTLED,
    );
    env.set_time(EXPIRY + 1);
    expect_code(env.send(&[ix], &[&payer]), code::SERIES_NOT_SETTLED);
}

#[test]
fn test_voided_series_closes_after_longs_burn_for_nothing() {
    let mut env = Env::new();
    let args = SeriesArgs::xau_put(4_000);
    let payer = env.funded();
    env.send(&[env.ix_create_series(&payer.pubkey(), args)], &[&payer])
        .unwrap();
    let usdc = env.usdc;
    env.token_account(&usdc, &payer.pubkey());
    let w = env.writer(100_000);
    env.long_account(&w.pubkey(), args);
    env.send(&[env.ix_mint(&w.pubkey(), args, 2)], &[&w])
        .unwrap();
    env.set_time(EXPIRY + HARD_S + D);
    env.settle(&args.series()).unwrap();
    let ix = env.ix_close_series(args, &payer.pubkey());
    expect_code(
        env.send(std::slice::from_ref(&ix), &[&payer]),
        code::SERIES_NOT_EMPTY,
    );
    env.send(&[env.ix_claim(&w.pubkey(), args)], &[&w]).unwrap();
    expect_code(
        env.send(std::slice::from_ref(&ix), &[&payer]),
        code::SERIES_NOT_EMPTY,
    );
    env.send(&[env.ix_redeem(&w.pubkey(), args, 2)], &[&w])
        .unwrap();
    env.send(&[ix], &[&payer]).unwrap();
}

#[test]
fn test_close_series_forged_and_wrong_accounts_are_rejected() {
    let mut l = settled();
    pay_everyone(&mut l);
    let admin = l.env.admin.insecure_clone();
    let usdc = l.env.usdc;
    let stranger = l.env.funded();
    l.env.token_account(&usdc, &stranger.pubkey());
    let base = l.env.ix_close_series(l.args, &l.payer.pubkey());
    let with = |index: usize, a: Address| {
        let mut ix = base.clone();
        ix.accounts[index].pubkey = a;
        ix
    };
    // Rent to someone who did not pay it (with their own USDC account, so only the payer
    // check can catch it).
    let mut ix = with(2, stranger.pubkey());
    ix.accounts[3].pubkey = ata(&stranger.pubkey(), &usdc);
    expect_code(l.env.send(&[ix], &[&admin]), code::INVALID_RENT_PAYER);
    // The payer's address with someone else's USDC account for the excess.
    expect_code(
        l.env
            .send(&[with(3, ata(&stranger.pubkey(), &usdc))], &[&admin]),
        code::INVALID_TOKEN_ACCOUNT,
    );
    // Another account as the vault: the payer's USDC account, and a forged copy of the vault.
    expect_code(
        l.env
            .send(&[with(1, ata(&l.payer.pubkey(), &usdc))], &[&admin]),
        code::INVALID_VAULT,
    );
    let fake_vault = Address::new_unique();
    let vault = ata(&l.args.series(), &usdc);
    l.env.forge(&vault, fake_vault, None);
    expect_code(
        l.env.send(&[with(1, fake_vault)], &[&admin]),
        code::INVALID_VAULT,
    );
    // Forged series: genuine bytes under another owner, then at another address.
    let fake = Address::new_unique();
    l.env.forge(&l.args.series(), fake, Some(ATTACKER_PROGRAM));
    expect_code(
        l.env.send(&[with(0, fake)], &[&admin]),
        code::INVALID_ACCOUNT_OWNER,
    );
    l.env.forge(&l.args.series(), fake, None);
    expect_code(l.env.send(&[with(0, fake)], &[&admin]), code::INVALID_PDA);
    // Wrong mint and token program.
    let other_mint = litesvm_token::CreateMint::new(&mut l.env.svm, &admin)
        .decimals(6)
        .send()
        .unwrap();
    expect_code(
        l.env.send(&[with(4, other_mint)], &[&admin]),
        code::INVALID_MINT,
    );
    expect_code(
        l.env.send(&[with(5, TOKEN_2022)], &[&admin]),
        code::INVALID_PROGRAM,
    );
    // Read-only accounts.
    for index in 0..4 {
        let mut ix = base.clone();
        ix.accounts[index].is_writable = false;
        expect_code(l.env.send(&[ix], &[&admin]), code::ACCOUNT_NOT_WRITABLE);
    }
    // Everything genuine still works.
    l.env.send(&[base], &[&admin]).unwrap();
}

#[test]
fn test_close_position_after_claim_returns_rent_to_the_writer() {
    let mut l = settled();
    let (w, args) = (l.writer.insecure_clone(), l.args);
    let position = position_address(&args.series(), &w.pubkey());
    let admin = l.env.admin.insecure_clone();
    let ix = l.env.ix_close_position(&position, &w.pubkey());
    expect_code(
        l.env.send(std::slice::from_ref(&ix), &[&admin]),
        code::POSITION_NOT_CLAIMED,
    );
    l.env
        .send(&[l.env.ix_claim(&w.pubkey(), args)], &[&w])
        .unwrap();
    // Rent to anyone but the writer is rejected.
    let other = l.env.funded();
    expect_code(
        l.env.send(
            &[l.env.ix_close_position(&position, &other.pubkey())],
            &[&admin],
        ),
        code::INVALID_RENT_PAYER,
    );
    let rent = lamports(&l.env, &position);
    let before = lamports(&l.env, &w.pubkey());
    l.env.send(&[ix], &[&admin]).unwrap();
    assert_eq!(lamports(&l.env, &w.pubkey()) - before, rent);
    assert_eq!(lamports(&l.env, &position), 0);
    // No second claim through a re-created position: there is none to claim with.
    expect_code(
        l.env.send(&[l.env.ix_claim(&w.pubkey(), args)], &[&w]),
        code::INSUFFICIENT_WRITTEN,
    );
    // The series still closes once the long tokens are redeemed.
    l.env
        .send(&[l.env.ix_redeem(&w.pubkey(), args, 2)], &[&w])
        .unwrap();
    close(&mut l).unwrap();
}

#[test]
fn test_close_position_with_nothing_written_and_forged_positions() {
    let mut env = Env::new();
    let args = SeriesArgs::xau_put(4_000);
    env.create_series(args);
    let w = env.writer(100_000);
    env.long_account(&w.pubkey(), args);
    env.send(&[env.ix_mint(&w.pubkey(), args, 1)], &[&w])
        .unwrap();
    let position = position_address(&args.series(), &w.pubkey());
    // Forged: genuine bytes under another owner, then at another address.
    let fake = Address::new_unique();
    env.forge(&position, fake, Some(ATTACKER_PROGRAM));
    expect_code(
        env.send(&[env.ix_close_position(&fake, &w.pubkey())], &[&w]),
        code::INVALID_ACCOUNT_OWNER,
    );
    env.forge(&position, fake, None);
    expect_code(
        env.send(&[env.ix_close_position(&fake, &w.pubkey())], &[&w]),
        code::INVALID_PDA,
    );
    // A series account is not a position.
    expect_code(
        env.send(&[env.ix_close_position(&args.series(), &w.pubkey())], &[&w]),
        code::INVALID_ACCOUNT_DATA,
    );
    // Fully closed before expiry: nothing written, so it can be closed at once.
    expect_code(
        env.send(&[env.ix_close_position(&position, &w.pubkey())], &[&w]),
        code::POSITION_NOT_CLAIMED,
    );
    env.send(&[env.ix_close(&w.pubkey(), args, 1)], &[&w])
        .unwrap();
    env.send(&[env.ix_close_position(&position, &w.pubkey())], &[&w])
        .unwrap();
    // Writing again re-creates it.
    env.send(&[env.ix_mint(&w.pubkey(), args, 1)], &[&w])
        .unwrap();
    assert_eq!(read_written(&env, &position), 1);
}
