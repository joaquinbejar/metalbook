//! `mint` and `close`: a writer opens or closes fully collateralized contracts.

use metalbook_core::Contracts;
use pinocchio::cpi::{Seed, Signer};
use pinocchio::sysvars::clock::Clock;
use pinocchio::sysvars::Sysvar;
use pinocchio::{AccountView, Address, ProgramResult};
use pinocchio_token::instructions::{Burn, MintTo, TransferChecked};

use crate::data::Reader;
use crate::error::MetalbookError;
use crate::pda::{self, SeriesSeeds, WRITER_SEED};
use crate::state::{self, AccountTag, Series, Writer, VERSION};
use crate::validate;

/// `mint`. Accounts: `[writer (signer, writable), series (writable), position (writable),
/// writer_usdc (writable), vault (writable), usdc_mint, long_mint (writable), writer_long
/// (writable), token_program, system_program]`. Data: `contracts: u64`.
///
/// Deposits `contracts × max_payout_per_contract` USDC (rounded up per contract, toward the
/// protocol) from `writer_usdc` into the vault and mints `contracts` long tokens to
/// `writer_long`. Creates the writer's position account on first use.
///
/// # Errors
///
/// See `docs/PLAN.md` 3.3.
#[inline(never)]
pub fn mint(program_id: &Address, accounts: &mut [AccountView], data: &[u8]) -> ProgramResult {
    let [writer, series, position, writer_usdc, vault, usdc_mint, long_mint, writer_long, token_program, system_program, ..] =
        accounts
    else {
        return Err(pinocchio::error::ProgramError::NotEnoughAccountKeys);
    };
    let n = read_contracts(data)?;

    validate::signer(writer)?;
    validate::writable(writer)?;
    validate::program(system_program, &pinocchio_system::ID)?;
    let s = load_active_series(program_id, series)?;
    check_series_accounts(&s, vault, usdc_mint, long_mint, token_program)?;
    validate::writable(writer_usdc)?;
    validate::writable(writer_long)?;
    validate::token_account(writer_usdc, usdc_mint.address(), writer.address())?;
    validate::token_account(writer_long, long_mint.address(), writer.address())?;

    // Position: created on first use, otherwise checked.
    validate::writable(position)?;
    if position.is_data_empty() {
        let (address, bump) = pda::find(
            &[
                WRITER_SEED,
                series.address().as_ref(),
                writer.address().as_ref(),
            ],
            program_id,
        )?;
        validate::address(position, &address, MetalbookError::InvalidPda)?;
        let bump_seed = [bump];
        let seeds = [
            Seed::from(WRITER_SEED),
            Seed::from(series.address().as_ref()),
            Seed::from(writer.address().as_ref()),
            Seed::from(&bump_seed),
        ];
        validate::create_pda_account(
            writer,
            position,
            core::mem::size_of::<Writer>(),
            program_id,
            &seeds,
        )?;
        let mut bytes = position.try_borrow_mut()?;
        let w = state::init_mut::<Writer>(&mut bytes)?;
        w.tag = AccountTag::Writer as u8;
        w.version = VERSION;
        w.bump = bump;
        w.series = series.address().to_bytes();
        w.owner = writer.address().to_bytes();
    }

    // Accounting, then state, then the CPIs.
    let mut ledger = s.ledger()?;
    let deposit = ledger.mint(n).map_err(MetalbookError::from)?;
    {
        validate::owned_by(position, program_id)?;
        let position_address = Address::new_from_array(position.address().to_bytes());
        let mut bytes = position.try_borrow_mut()?;
        let w = load_position(
            program_id,
            &position_address,
            &mut bytes,
            series.address(),
            writer.address(),
        )?;
        w.written = w
            .written
            .checked_add(n.get())
            .ok_or(MetalbookError::Overflow)?;
    }
    {
        let mut bytes = series.try_borrow_mut()?;
        state::load_mut::<Series>(&mut bytes)?.store_ledger(&ledger);
    }

    TransferChecked::new(
        writer_usdc,
        usdc_mint,
        vault,
        writer,
        deposit.get(),
        s.usdc_decimals,
    )
    .invoke_with_program(token_program.address())?;
    with_series_signer(&s, |signer| {
        MintTo::new(long_mint, writer_long, series, n.get())
            .invoke_signed_with_program(signer, token_program.address())
    })
}

/// `close`. Accounts: `[writer (signer), series (writable), position (writable), writer_usdc
/// (writable), vault (writable), usdc_mint, long_mint (writable), writer_long (writable),
/// token_program]`. Data: `contracts: u64`.
///
/// Before expiry, burns `contracts` long tokens from the writer's own account and returns
/// `contracts × max_payout_per_contract` USDC from the vault. At most what this writer wrote
/// and has not closed.
///
/// # Errors
///
/// See `docs/PLAN.md` 3.3.
#[inline(never)]
pub fn close(program_id: &Address, accounts: &mut [AccountView], data: &[u8]) -> ProgramResult {
    let [writer, series, position, writer_usdc, vault, usdc_mint, long_mint, writer_long, token_program, ..] =
        accounts
    else {
        return Err(pinocchio::error::ProgramError::NotEnoughAccountKeys);
    };
    let n = read_contracts(data)?;

    validate::signer(writer)?;
    let s = load_active_series(program_id, series)?;
    check_series_accounts(&s, vault, usdc_mint, long_mint, token_program)?;
    validate::writable(writer_usdc)?;
    validate::writable(writer_long)?;
    validate::token_account(writer_usdc, usdc_mint.address(), writer.address())?;
    validate::token_account(writer_long, long_mint.address(), writer.address())?;
    validate::writable(position)?;
    if position.is_data_empty() {
        return Err(MetalbookError::InsufficientWritten.into());
    }

    let mut ledger = s.ledger()?;
    let withdrawal = {
        validate::owned_by(position, program_id)?;
        let position_address = Address::new_from_array(position.address().to_bytes());
        let mut bytes = position.try_borrow_mut()?;
        let w = load_position(
            program_id,
            &position_address,
            &mut bytes,
            series.address(),
            writer.address(),
        )?;
        let left = w
            .written
            .checked_sub(n.get())
            .ok_or(MetalbookError::InsufficientWritten)?;
        let withdrawal = ledger.close(n).map_err(MetalbookError::from)?;
        w.written = left;
        withdrawal
    };
    {
        let mut bytes = series.try_borrow_mut()?;
        state::load_mut::<Series>(&mut bytes)?.store_ledger(&ledger);
    }

    Burn::new(writer_long, long_mint, writer, n.get())
        .invoke_with_program(token_program.address())?;
    with_series_signer(&s, |signer| {
        TransferChecked::new(
            vault,
            usdc_mint,
            writer_usdc,
            series,
            withdrawal.get(),
            s.usdc_decimals,
        )
        .invoke_signed_with_program(signer, token_program.address())
    })
}

fn read_contracts(data: &[u8]) -> Result<Contracts, MetalbookError> {
    let mut r = Reader::new(data);
    let n = r.u64()?;
    r.finish()?;
    if n == 0 {
        return Err(MetalbookError::ZeroAmount);
    }
    Ok(Contracts::new(n))
}

/// Loads a series (owner, layout, PDA from its stored terms and bump), writable, before its
/// expiry. Returns a copy.
#[inline(never)]
fn load_active_series(
    program_id: &Address,
    series: &AccountView,
) -> Result<Series, MetalbookError> {
    validate::writable(series)?;
    let s = load_series(program_id, series)?;
    let now = Clock::get()
        .map_err(|_| MetalbookError::InvalidSysvar)?
        .unix_timestamp;
    if now >= s.expiry {
        return Err(MetalbookError::SeriesExpired);
    }
    Ok(s)
}

/// Loads a series and checks its owner, layout and PDA. Returns a copy.
///
/// # Errors
///
/// `InvalidAccountOwner`, `InvalidAccountData` or `InvalidPda`.
#[inline(never)]
pub fn load_series(program_id: &Address, series: &AccountView) -> Result<Series, MetalbookError> {
    if series.is_data_empty() {
        return Err(MetalbookError::SeriesNotListed);
    }
    validate::owned_by(series, program_id)?;
    let bytes = series
        .try_borrow()
        .map_err(|_| MetalbookError::InvalidAccountData)?;
    let s = *state::load::<Series>(&bytes)?;
    let seeds = SeriesSeeds::new(
        s.underlying,
        s.kind,
        s.strike,
        s.expiry,
        s.cap,
        s.multiplier,
    );
    pda::check(&seeds.as_slices(), s.bump, program_id, series.address())?;
    Ok(s)
}

/// The vault, mints and token program are the ones the series recorded at creation.
fn check_series_accounts(
    s: &Series,
    vault: &AccountView,
    usdc_mint: &AccountView,
    long_mint: &AccountView,
    token_program: &AccountView,
) -> Result<(), MetalbookError> {
    validate::writable(vault)?;
    validate::writable(long_mint)?;
    validate::address(
        vault,
        &Address::new_from_array(s.vault),
        MetalbookError::InvalidVault,
    )?;
    validate::address(
        usdc_mint,
        &Address::new_from_array(s.usdc_mint),
        MetalbookError::InvalidMint,
    )?;
    validate::address(
        long_mint,
        &Address::new_from_array(s.long_mint),
        MetalbookError::InvalidMint,
    )?;
    validate::program(token_program, &Address::new_from_array(s.token_program))
}

/// Loads a position and checks its owner, layout, PDA, series and wallet.
fn load_position<'a>(
    program_id: &Address,
    address: &Address,
    bytes: &'a mut [u8],
    series: &Address,
    owner: &Address,
) -> Result<&'a mut Writer, MetalbookError> {
    let w = state::load_mut::<Writer>(bytes)?;
    pda::check(
        &[WRITER_SEED, series.as_ref(), owner.as_ref()],
        w.bump,
        program_id,
        address,
    )?;
    if w.series != series.to_bytes() || w.owner != owner.to_bytes() {
        return Err(MetalbookError::InvalidAccountData);
    }
    Ok(w)
}

/// Runs `f` with the series PDA's signer seeds.
fn with_series_signer<F>(s: &Series, f: F) -> ProgramResult
where
    F: FnOnce(&[Signer]) -> ProgramResult,
{
    let seeds = SeriesSeeds::new(
        s.underlying,
        s.kind,
        s.strike,
        s.expiry,
        s.cap,
        s.multiplier,
    );
    let bump = [s.bump];
    let [a, b, c, d, e, g, h] = seeds.as_slices();
    let list = [
        Seed::from(a),
        Seed::from(b),
        Seed::from(c),
        Seed::from(d),
        Seed::from(e),
        Seed::from(g),
        Seed::from(h),
        Seed::from(&bump),
    ];
    f(&[Signer::from(&list)])
}
