//! metalbook on-chain program: cash-settled, fully collateralized European options on gold
//! and silver, settled in USDC.
//!
//! Milestone 2 implements listing and writing: the config, the expiry calendars, series
//! creation (series account, long mint and USDC vault) and writers minting and closing
//! contracts. Settlement, redemption and claims follow in milestones 3 and 4
//! (`docs/PLAN.md`). The payoff, collateral and vault accounting come from `metalbook-core`.
//!
//! Instruction data is a one-byte [`Instruction`] tag followed by the fields documented on
//! each handler, little endian.

#![no_std]

pub mod data;
pub mod error;
pub mod pda;
pub mod processor;
pub mod state;
pub mod validate;

use pinocchio::{AccountView, Address, ProgramResult};

use crate::error::MetalbookError;

// The macros expand to the `extern "C"` entrypoint and the target's allocator and panic hooks;
// that generated code is the only `unsafe` in this crate. `no_allocator!` also defines
// `allocate_unchecked`, whose offset helper asserts its bounds; nothing here calls it.
#[allow(unsafe_code, clippy::disallowed_macros)]
mod entry {
    pinocchio::program_entrypoint!(super::process_instruction);
    pinocchio::no_allocator!();
    pinocchio::nostd_panic_handler!();
}

/// Instruction tags (the first byte of the instruction data).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Instruction {
    /// [`processor::admin::init_config`].
    InitConfig = 0,
    /// [`processor::admin::set_config`].
    SetConfig = 1,
    /// [`processor::admin::set_calendar`].
    SetCalendar = 2,
    /// [`processor::series::create_series`].
    CreateSeries = 3,
    /// [`processor::position::mint`].
    Mint = 4,
    /// [`processor::position::close`].
    Close = 5,
}

impl TryFrom<u8> for Instruction {
    type Error = MetalbookError;

    fn try_from(tag: u8) -> Result<Self, MetalbookError> {
        match tag {
            0 => Ok(Self::InitConfig),
            1 => Ok(Self::SetConfig),
            2 => Ok(Self::SetCalendar),
            3 => Ok(Self::CreateSeries),
            4 => Ok(Self::Mint),
            5 => Ok(Self::Close),
            _ => Err(MetalbookError::InvalidInstruction),
        }
    }
}

/// Program entrypoint body: decodes the tag and dispatches.
///
/// # Errors
///
/// `InvalidInstruction` for empty data or an unknown tag; otherwise the handler's error.
pub fn process_instruction(
    program_id: &Address,
    accounts: &mut [AccountView],
    data: &[u8],
) -> ProgramResult {
    let (&tag, rest) = data
        .split_first()
        .ok_or(MetalbookError::InvalidInstruction)?;
    match Instruction::try_from(tag)? {
        Instruction::InitConfig => processor::admin::init_config(program_id, accounts, rest),
        Instruction::SetConfig => processor::admin::set_config(program_id, accounts, rest),
        Instruction::SetCalendar => processor::admin::set_calendar(program_id, accounts, rest),
        Instruction::CreateSeries => processor::series::create_series(program_id, accounts, rest),
        Instruction::Mint => processor::position::mint(program_id, accounts, rest),
        Instruction::Close => processor::position::close(program_id, accounts, rest),
    }
}
