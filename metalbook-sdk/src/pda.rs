//! Program-derived addresses, with the program's seeds (`metalbook-program/src/pda.rs`).

use crate::{Address, ATA_PROGRAM, TOKEN_PROGRAM, UPGRADEABLE_LOADER};

/// The terms that name a series (its PDA seeds).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SeriesSpec {
    /// `Underlying` discriminant: 0 XAU, 1 XAG.
    pub underlying: u8,
    /// `OptionKind` discriminant: 0 call, 1 put.
    pub kind: u8,
    /// Strike, USDC atoms per ounce.
    pub strike: u64,
    /// Expiry, Unix seconds.
    pub expiry: i64,
    /// Cap, USDC atoms per ounce (0 for a put).
    pub cap: u64,
    /// Multiplier, milli-ounces.
    pub multiplier: u32,
}

/// `["config"]`.
#[must_use]
pub fn config(program_id: &Address) -> (Address, u8) {
    Address::find_program_address(&[b"config"], program_id)
}

/// `["calendar", [underlying]]`.
#[must_use]
pub fn calendar(program_id: &Address, underlying: u8) -> (Address, u8) {
    Address::find_program_address(&[b"calendar", &[underlying]], program_id)
}

/// `["series", [underlying], [kind], strike, expiry, cap, multiplier]`, little endian.
#[must_use]
pub fn series(program_id: &Address, s: &SeriesSpec) -> (Address, u8) {
    Address::find_program_address(
        &[
            b"series",
            &[s.underlying],
            &[s.kind],
            &s.strike.to_le_bytes(),
            &s.expiry.to_le_bytes(),
            &s.cap.to_le_bytes(),
            &s.multiplier.to_le_bytes(),
        ],
        program_id,
    )
}

/// `["long", series]`.
#[must_use]
pub fn long_mint(program_id: &Address, series: &Address) -> (Address, u8) {
    Address::find_program_address(&[b"long", series.as_ref()], program_id)
}

/// `["writer", series, owner]`.
#[must_use]
pub fn position(program_id: &Address, series: &Address, owner: &Address) -> (Address, u8) {
    Address::find_program_address(&[b"writer", series.as_ref(), owner.as_ref()], program_id)
}

/// The SPL Token associated token account of `owner` for `mint` (the series vault is the
/// series' one for USDC).
#[must_use]
pub fn ata(owner: &Address, mint: &Address) -> Address {
    Address::find_program_address(
        &[owner.as_ref(), TOKEN_PROGRAM.as_ref(), mint.as_ref()],
        &ATA_PROGRAM,
    )
    .0
}

/// The program's `ProgramData` account under the upgradeable loader.
#[must_use]
pub fn program_data(program_id: &Address) -> (Address, u8) {
    Address::find_program_address(&[program_id.as_ref()], &UPGRADEABLE_LOADER)
}
