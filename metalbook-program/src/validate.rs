//! Account checks shared by the instructions, and account creation.

use pinocchio::cpi::{Seed, Signer};
use pinocchio::sysvars::rent::Rent;
use pinocchio::sysvars::Sysvar;
use pinocchio::{AccountView, Address, ProgramResult};
use pinocchio_system::instructions::{Allocate, Assign, CreateAccount, Transfer};
use pinocchio_token::state::{Account as TokenAccount, AccountState};

use crate::error::MetalbookError;

/// `account` signed the transaction.
///
/// # Errors
///
/// `MissingSignature`.
pub fn signer(account: &AccountView) -> Result<(), MetalbookError> {
    if account.is_signer() {
        Ok(())
    } else {
        Err(MetalbookError::MissingSignature)
    }
}

/// `account` is writable.
///
/// # Errors
///
/// `AccountNotWritable`.
pub fn writable(account: &AccountView) -> Result<(), MetalbookError> {
    if account.is_writable() {
        Ok(())
    } else {
        Err(MetalbookError::AccountNotWritable)
    }
}

/// `account` is the program `id`.
///
/// # Errors
///
/// `InvalidProgram`.
pub fn program(account: &AccountView, id: &Address) -> Result<(), MetalbookError> {
    if account.address() == id {
        Ok(())
    } else {
        Err(MetalbookError::InvalidProgram)
    }
}

/// `account` is owned by `owner`.
///
/// # Errors
///
/// `InvalidAccountOwner`.
pub fn owned_by(account: &AccountView, owner: &Address) -> Result<(), MetalbookError> {
    if account.owned_by(owner) {
        Ok(())
    } else {
        Err(MetalbookError::InvalidAccountOwner)
    }
}

/// `account` has the address `expected`.
///
/// # Errors
///
/// `error`.
pub fn address(
    account: &AccountView,
    expected: &Address,
    error: MetalbookError,
) -> Result<(), MetalbookError> {
    if account.address() == expected {
        Ok(())
    } else {
        Err(error)
    }
}

/// `account` is an initialized, unfrozen SPL Token account of `mint` owned by `owner`.
///
/// # Errors
///
/// `InvalidAccountOwner` if the token program does not own it, `InvalidTokenAccount` for a
/// wrong length, mint, owner or state.
#[inline(never)]
pub fn token_account(
    account: &AccountView,
    mint: &Address,
    owner: &Address,
) -> Result<(), MetalbookError> {
    owned_by(account, &pinocchio_token::ID)?;
    let data = TokenAccount::from_account_view(account)
        .map_err(|_| MetalbookError::InvalidTokenAccount)?;
    if data.mint() != mint || data.owner() != owner || data.state() != AccountState::Initialized {
        return Err(MetalbookError::InvalidTokenAccount);
    }
    Ok(())
}

/// Creates the program-owned (or `owner`-owned) account `target` at a PDA, signed by
/// `seeds`, funded by `payer` to rent exemption.
///
/// A plain `CreateAccount` fails if anyone sent lamports to the address first, which would
/// let them block the creation. So a pre-funded empty system account is topped up, allocated
/// and assigned instead.
///
/// # Errors
///
/// `AlreadyInitialized` if `target` has data or is not a system account; a system program
/// error from the CPIs.
#[inline(never)]
pub fn create_pda_account(
    payer: &AccountView,
    target: &AccountView,
    space: usize,
    owner: &Address,
    seeds: &[Seed],
) -> ProgramResult {
    if !target.is_data_empty() || !target.owned_by(&pinocchio_system::ID) {
        return Err(MetalbookError::AlreadyInitialized.into());
    }
    let signer = [Signer::from(seeds)];
    let space_u64 = u64::try_from(space).map_err(|_| MetalbookError::Overflow)?;
    let current = target.lamports();
    if current == 0 {
        return CreateAccount::with_minimum_balance(payer, target, space_u64, owner, None)?
            .invoke_signed(&signer);
    }
    let required = Rent::get()?.try_minimum_balance(space)?;
    if let Some(top_up) = required.checked_sub(current) {
        if top_up > 0 {
            Transfer {
                from: payer,
                to: target,
                lamports: top_up,
            }
            .invoke()?;
        }
    }
    Allocate {
        account: target,
        space: space_u64,
    }
    .invoke_signed(&signer)?;
    Assign {
        account: target,
        owner,
    }
    .invoke_signed(&signer)
}
