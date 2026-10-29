//! Program-derived addresses: seeds, canonical-bump search at creation and cheap re-derivation
//! with a stored bump afterwards.

use pinocchio::Address;

use crate::error::MetalbookError;

/// Seed prefix of the config.
pub const CONFIG_SEED: &[u8] = b"config";
/// Seed prefix of a calendar.
pub const CALENDAR_SEED: &[u8] = b"calendar";
/// Seed prefix of a series.
pub const SERIES_SEED: &[u8] = b"series";
/// Seed prefix of a long mint.
pub const LONG_SEED: &[u8] = b"long";
/// Seed prefix of a writer position.
pub const WRITER_SEED: &[u8] = b"writer";

/// The upgradeable BPF loader, owner of every upgradeable program's `ProgramData`.
pub const UPGRADEABLE_LOADER_ID: Address =
    Address::from_str_const("BPFLoaderUpgradeab1e11111111111111111111111");

/// Finds the canonical (highest valid) bump. Used once, when an account is created, so that
/// no second address can exist for the same seeds.
///
/// # Errors
///
/// `InvalidPda` if no bump gives an off-curve address (never expected).
#[must_use = "the result carries a possible error"]
pub fn find(seeds: &[&[u8]], program_id: &Address) -> Result<(Address, u8), MetalbookError> {
    Address::try_find_program_address(seeds, program_id).ok_or(MetalbookError::InvalidPda)
}

/// Re-derives an address from its seeds and stored bump (one `sol_sha256`, no curve check:
/// the bump was proven canonical by [`find`] at creation) and compares it with `expected`.
/// Every seed this program uses is at most 32 bytes, so the derivation cannot fail.
///
/// # Errors
///
/// `InvalidPda` if it differs.
pub fn check<const N: usize>(
    seeds: &[&[u8]; N],
    bump: u8,
    program_id: &Address,
    expected: &Address,
) -> Result<(), MetalbookError> {
    let derived = Address::derive_address(seeds, Some(bump), program_id);
    if &derived == expected {
        Ok(())
    } else {
        Err(MetalbookError::InvalidPda)
    }
}

/// Seeds of a series, in the order of `docs/PLAN.md` 3.1, as owned byte arrays so that
/// callers can borrow them for both derivation and signing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeriesSeeds {
    /// `[underlying]`.
    pub underlying: [u8; 1],
    /// `[kind]`.
    pub kind: [u8; 1],
    /// Strike, little endian.
    pub strike: [u8; 8],
    /// Expiry, little endian.
    pub expiry: [u8; 8],
    /// Cap, little endian.
    pub cap: [u8; 8],
    /// Multiplier, little endian.
    pub multiplier: [u8; 4],
}

impl SeriesSeeds {
    /// Seeds from the raw stored fields.
    #[must_use]
    pub const fn new(
        underlying: u8,
        kind: u8,
        strike: u64,
        expiry: i64,
        cap: u64,
        mult: u32,
    ) -> Self {
        Self {
            underlying: [underlying],
            kind: [kind],
            strike: strike.to_le_bytes(),
            expiry: expiry.to_le_bytes(),
            cap: cap.to_le_bytes(),
            multiplier: mult.to_le_bytes(),
        }
    }

    /// The seed slices, without the bump.
    #[must_use]
    pub fn as_slices(&self) -> [&[u8]; 7] {
        [
            SERIES_SEED,
            &self.underlying,
            &self.kind,
            &self.strike,
            &self.expiry,
            &self.cap,
            &self.multiplier,
        ]
    }
}
