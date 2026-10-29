//! `close_series` and `close_position`: rent recovery once nothing is owed (`docs/PLAN.md` 4.6,
//! open question 7 decided). Payouts stay claimable forever; these instructions only close
//! accounts that can no longer hold anyone's money.

use metalbook_core::{SeriesState, UsdcAtoms};
use pinocchio::{AccountView, Address, ProgramResult};
use pinocchio_token::instructions::{CloseAccount, TransferChecked};
use pinocchio_token::state::Account as TokenAccount;

use crate::error::MetalbookError;
use crate::pda::{self, WRITER_SEED};
use crate::processor::position::{load_series, with_series_signer};
use crate::state::{self, Writer};
use crate::validate;

/// `close_series`. Accounts: `[series (writable), vault (writable), rent_payer (writable),
/// payer_usdc (writable), usdc_mint, token_program]`. Data: none. Anyone may call it.
///
/// Requires a `Settled` or `Voided` series whose ledger owes nothing: every long token
/// redeemed (after a void too, for zero), every writer position claimed, and an owed vault
/// balance of zero. Owed amounts come from the ledger, never from the token balance. Any
/// balance the vault still holds was sent to it without being owed to anyone (a donation);
/// it goes to the rent payer's USDC account, so a one-atom transfer cannot block the close.
/// Then the vault is closed and the series account too, both rents going to `rent_payer`,
/// the account that paid them at `create_series`. The long mint stays: SPL Token cannot
/// close a mint.
///
/// # Errors
///
/// `SeriesNotSettled`, `SeriesNotEmpty`, `InvalidRentPayer`, and the account checks.
#[inline(never)]
pub fn close_series(
    program_id: &Address,
    accounts: &mut [AccountView],
    data: &[u8],
) -> ProgramResult {
    let [series, vault, rent_payer, payer_usdc, usdc_mint, token_program, ..] = accounts else {
        return Err(pinocchio::error::ProgramError::NotEnoughAccountKeys);
    };
    if !data.is_empty() {
        return Err(MetalbookError::InvalidInstructionData.into());
    }
    validate::writable(series)?;
    validate::writable(vault)?;
    validate::writable(rent_payer)?;
    validate::writable(payer_usdc)?;
    let s = load_series(program_id, series)?;

    // Nothing owed, from the ledger alone.
    let ledger = s.ledger().map_err(MetalbookError::from)?;
    if !matches!(ledger.state(), SeriesState::Settled | SeriesState::Voided) {
        return Err(MetalbookError::SeriesNotSettled.into());
    }
    let p = ledger.parts();
    if p.longs_outstanding.get() != 0
        || p.writers_unclaimed.get() != 0
        || p.vault != UsdcAtoms::ZERO
    {
        return Err(MetalbookError::SeriesNotEmpty.into());
    }

    validate::address(
        vault,
        &Address::new_from_array(s.vault),
        MetalbookError::InvalidVault,
    )?;
    validate::address(
        rent_payer,
        &Address::new_from_array(s.rent_payer),
        MetalbookError::InvalidRentPayer,
    )?;
    validate::address(
        usdc_mint,
        &Address::new_from_array(s.usdc_mint),
        MetalbookError::InvalidMint,
    )?;
    validate::program(token_program, &Address::new_from_array(s.token_program))?;
    validate::vault_account(vault, usdc_mint.address(), series.address())?;
    validate::token_account(payer_usdc, usdc_mint.address(), rent_payer.address())?;
    let unowed = TokenAccount::from_account_view(vault)
        .map_err(|_| MetalbookError::InvalidVault)?
        .amount();

    with_series_signer(&s, |signer| {
        if unowed > 0 {
            TransferChecked::new(
                vault,
                usdc_mint,
                payer_usdc,
                series,
                unowed,
                s.usdc_decimals,
            )
            .invoke_signed_with_program(signer, token_program.address())?;
        }
        CloseAccount::new(vault, rent_payer, series)
            .invoke_signed_with_program(signer, token_program.address())
    })?;

    // Last: nothing reads the series after this.
    let lamports = rent_payer
        .lamports()
        .checked_add(series.lamports())
        .ok_or(MetalbookError::Overflow)?;
    rent_payer.set_lamports(lamports);
    series.set_lamports(0);
    series.close()
}

/// `close_position`. Accounts: `[position (writable), owner (writable)]`. Data: none. Anyone
/// may call it; the rent goes to the writer who paid it.
///
/// Requires a genuine writer position that has claimed, or that has nothing written. The
/// series is not read, so this also works after `close_series`.
///
/// # Errors
///
/// `PositionNotClaimed`, `InvalidRentPayer` if `owner` is not the position's writer, and the
/// account checks.
#[inline(never)]
pub fn close_position(
    program_id: &Address,
    accounts: &mut [AccountView],
    data: &[u8],
) -> ProgramResult {
    let [position, owner, ..] = accounts else {
        return Err(pinocchio::error::ProgramError::NotEnoughAccountKeys);
    };
    if !data.is_empty() {
        return Err(MetalbookError::InvalidInstructionData.into());
    }
    validate::writable(position)?;
    validate::writable(owner)?;
    validate::owned_by(position, program_id)?;
    let w = {
        let bytes = position
            .try_borrow()
            .map_err(|_| MetalbookError::InvalidAccountData)?;
        *state::load::<Writer>(&bytes)?
    };
    pda::check(
        &[WRITER_SEED, &w.series, &w.owner],
        w.bump,
        program_id,
        position.address(),
    )?;
    validate::address(
        owner,
        &Address::new_from_array(w.owner),
        MetalbookError::InvalidRentPayer,
    )?;
    if w.claimed == 0 && w.written != 0 {
        return Err(MetalbookError::PositionNotClaimed.into());
    }

    let lamports = owner
        .lamports()
        .checked_add(position.lamports())
        .ok_or(MetalbookError::Overflow)?;
    owner.set_lamports(lamports);
    position.set_lamports(0);
    position.close()
}
