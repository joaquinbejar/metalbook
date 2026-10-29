//! `redeem` and `claim_writer`: paying out a settled (or voided) series.
//!
//! The vault empties exactly: every long token receives `payout` (rounded down, toward the
//! protocol) and every written contract returns `max_payout − payout` to its writer, who
//! therefore owns the rounding dust. After a void, `payout` is zero and writers recover their
//! whole collateral.

use pinocchio::{AccountView, Address, ProgramResult};
use pinocchio_token::instructions::{Burn, TransferChecked};

use crate::error::MetalbookError;
use crate::processor::position::{
    check_series_accounts, load_position, load_series, read_contracts, with_series_signer,
};
use crate::state::{self, Series};
use crate::validate;

/// `redeem`. Accounts: `[holder (signer), series (writable), holder_long (writable),
/// holder_usdc (writable), vault (writable), usdc_mint, long_mint (writable),
/// token_program]`. Data: `contracts: u64`.
///
/// Burns `contracts` long tokens from the holder's own account and pays
/// `contracts × payout` USDC from the vault to the holder's own USDC account. On a `Settled`
/// series, and on a `Voided` one where the payout is zero: burning worthless long tokens lets
/// the series be closed (`docs/PLAN.md` 4.6).
///
/// # Errors
///
/// `SeriesNotSettled`, `InsufficientLongTokens`, and the account checks of `docs/PLAN.md` 3.3.
#[inline(never)]
pub fn redeem(program_id: &Address, accounts: &mut [AccountView], data: &[u8]) -> ProgramResult {
    let [holder, series, holder_long, holder_usdc, vault, usdc_mint, long_mint, token_program, ..] =
        accounts
    else {
        return Err(pinocchio::error::ProgramError::NotEnoughAccountKeys);
    };
    let n = read_contracts(data)?;

    validate::signer(holder)?;
    validate::writable(series)?;
    let s = load_series(program_id, series)?;
    check_series_accounts(&s, vault, usdc_mint, long_mint, token_program)?;
    validate::writable(holder_long)?;
    validate::writable(holder_usdc)?;
    validate::token_account(holder_long, long_mint.address(), holder.address())?;
    validate::token_account(holder_usdc, usdc_mint.address(), holder.address())?;

    let mut ledger = s.ledger().map_err(MetalbookError::from)?;
    let amount = ledger.redeem(n).map_err(MetalbookError::from)?;
    {
        let mut bytes = series.try_borrow_mut()?;
        state::load_mut::<Series>(&mut bytes)?.store_ledger(&ledger);
    }

    Burn::new(holder_long, long_mint, holder, n.get())
        .invoke_with_program(token_program.address())?;
    if amount.get() == 0 {
        return Ok(());
    }
    with_series_signer(&s, |signer| {
        TransferChecked::new(
            vault,
            usdc_mint,
            holder_usdc,
            series,
            amount.get(),
            s.usdc_decimals,
        )
        .invoke_signed_with_program(signer, token_program.address())
    })
}

/// `claim_writer`. Accounts: `[writer (signer), series (writable), position (writable),
/// writer_usdc (writable), vault (writable), usdc_mint, token_program]`. Data: none.
///
/// Once per position, on a `Settled` or `Voided` series: pays
/// `written × (max_payout − payout)` USDC from the vault to the writer's own USDC account.
///
/// # Errors
///
/// `SeriesNotSettled`, `AlreadyClaimed`, `ZeroAmount` for a position with nothing written,
/// and the account checks of `docs/PLAN.md` 3.3.
#[inline(never)]
pub fn claim_writer(
    program_id: &Address,
    accounts: &mut [AccountView],
    data: &[u8],
) -> ProgramResult {
    let [writer, series, position, writer_usdc, vault, usdc_mint, token_program, ..] = accounts
    else {
        return Err(pinocchio::error::ProgramError::NotEnoughAccountKeys);
    };
    if !data.is_empty() {
        return Err(MetalbookError::InvalidInstructionData.into());
    }

    validate::signer(writer)?;
    validate::writable(series)?;
    let s = load_series(program_id, series)?;
    validate::writable(vault)?;
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
    validate::program(token_program, &Address::new_from_array(s.token_program))?;
    validate::writable(writer_usdc)?;
    validate::token_account(writer_usdc, usdc_mint.address(), writer.address())?;
    validate::writable(position)?;
    if position.is_data_empty() {
        return Err(MetalbookError::InsufficientWritten.into());
    }
    validate::owned_by(position, program_id)?;

    let mut ledger = s.ledger().map_err(MetalbookError::from)?;
    let amount = {
        let position_address = Address::new_from_array(position.address().to_bytes());
        let mut bytes = position.try_borrow_mut()?;
        let w = load_position(
            program_id,
            &position_address,
            &mut bytes,
            series.address(),
            writer.address(),
        )?;
        if w.claimed != 0 {
            return Err(MetalbookError::AlreadyClaimed.into());
        }
        let amount = ledger
            .claim_writer(metalbook_core::Contracts::new(w.written))
            .map_err(MetalbookError::from)?;
        w.claimed = 1;
        amount
    };
    {
        let mut bytes = series.try_borrow_mut()?;
        state::load_mut::<Series>(&mut bytes)?.store_ledger(&ledger);
    }

    if amount.get() == 0 {
        return Ok(());
    }
    with_series_signer(&s, |signer| {
        TransferChecked::new(
            vault,
            usdc_mint,
            writer_usdc,
            series,
            amount.get(),
            s.usdc_decimals,
        )
        .invoke_signed_with_program(signer, token_program.address())
    })
}
