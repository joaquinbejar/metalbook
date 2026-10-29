//! Account layouts.
//!
//! Every account is `#[repr(C)]`, `bytemuck::Pod`, 8-aligned and without implicit padding:
//! padding is an explicit `_pad` field. Enums and addresses are stored as integers and byte
//! arrays and validated on load. The first byte is an [`AccountTag`], the second the layout
//! [`VERSION`]. Milestone 3 extends `Series` with the settlement fields of `docs/PLAN.md` and
//! bumps the version.

use bytemuck::Pod;
use metalbook_core::{
    Contracts, LedgerParts, OptionKind, OuncesPerContract, SeriesKey, SeriesLedger, SeriesState,
    SeriesTerms, StrikeUsd, Underlying, UnixTimestamp, UsdcAtoms,
};

use crate::error::MetalbookError;

/// Layout version of every account written by this build.
pub const VERSION: u8 = 1;

/// Number of expiries a calendar holds.
pub const CALENDAR_CAPACITY: usize = 64;

/// Number of cap offsets in the cap grid.
pub const CAP_GRID_LEN: usize = 4;

/// First byte of every account: which layout follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum AccountTag {
    /// [`Config`].
    Config = 1,
    /// [`Calendar`].
    Calendar = 2,
    /// [`Series`].
    Series = 3,
    /// [`Writer`].
    Writer = 4,
}

/// An account layout with a tag and a version in its first two bytes.
pub trait Tagged: Pod {
    /// The tag this layout carries.
    const TAG: AccountTag;

    /// The stored tag byte.
    fn tag(&self) -> u8;

    /// The stored version byte.
    fn version(&self) -> u8;
}

/// Reads `T` from account data: exact length, alignment, tag and version.
///
/// # Errors
///
/// `InvalidAccountData` on any mismatch.
#[must_use = "the result carries a possible error"]
pub fn load<T: Tagged>(data: &[u8]) -> Result<&T, MetalbookError> {
    let value: &T =
        bytemuck::try_from_bytes(data).map_err(|_| MetalbookError::InvalidAccountData)?;
    check_header(value)?;
    Ok(value)
}

/// Mutable form of [`load`].
///
/// # Errors
///
/// `InvalidAccountData` on any mismatch.
#[must_use = "the result carries a possible error"]
pub fn load_mut<T: Tagged>(data: &mut [u8]) -> Result<&mut T, MetalbookError> {
    let value: &mut T =
        bytemuck::try_from_bytes_mut(data).map_err(|_| MetalbookError::InvalidAccountData)?;
    check_header(value)?;
    Ok(value)
}

/// Reinterprets freshly created (zeroed) account data as `T`, without a header check.
///
/// # Errors
///
/// `InvalidAccountData` if the length or alignment is wrong.
#[must_use = "the result carries a possible error"]
pub fn init_mut<T: Tagged>(data: &mut [u8]) -> Result<&mut T, MetalbookError> {
    bytemuck::try_from_bytes_mut(data).map_err(|_| MetalbookError::InvalidAccountData)
}

fn check_header<T: Tagged>(value: &T) -> Result<(), MetalbookError> {
    if value.tag() != T::TAG as u8 || value.version() != VERSION {
        return Err(MetalbookError::InvalidAccountData);
    }
    Ok(())
}

/// The account structs. They live in their own module because `Pod`'s derive proves the
/// absence of padding with a `const` assertion: it is evaluated at compile time and cannot
/// panic at run time, but clippy reports the macro it uses, and an `allow` only reaches derive
/// output at module level. Nothing but the struct definitions is in this module.
#[allow(clippy::disallowed_macros)]
mod layout {
    use bytemuck::{Pod, Zeroable};

    use super::{CALENDAR_CAPACITY, CAP_GRID_LEN};

    /// Global parameters. PDA `["config"]`. 144 bytes.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Pod, Zeroable)]
    #[repr(C)]
    pub struct Config {
        /// [`AccountTag::Config`](crate::state::AccountTag::Config).
        pub tag: u8,
        /// [`VERSION`](crate::state::VERSION).
        pub version: u8,
        /// Canonical bump of the PDA.
        pub bump: u8,
        /// Padding.
        pub _pad0: [u8; 5],
        /// May change the config and the calendars.
        pub admin: [u8; 32],
        /// The collateral and settlement mint.
        pub usdc_mint: [u8; 32],
        /// The token program of `usdc_mint` and of the long mints: SPL Token.
        pub token_program: [u8; 32],
        /// Minimum time from listing to expiry, in seconds.
        pub min_listing_s: u32,
        /// Padding.
        pub _pad1: [u8; 4],
        /// Strike tick per underlying (index = `Underlying` discriminant), in USDC atoms per
        /// ounce. Never zero.
        pub strike_tick: [u64; 2],
        /// Contract multiplier per underlying, in milli-ounces. Never zero.
        pub multiplier: [u32; 2],
        /// Cap grid for calls: `cap = K × (10_000 + bps) / 10_000` exactly, for a non-zero entry.
        pub cap_bps: [u16; CAP_GRID_LEN],
    }

    /// Allowed expiries of one underlying. PDA `["calendar", [underlying]]`. 520 bytes.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Pod, Zeroable)]
    #[repr(C)]
    pub struct Calendar {
        /// [`AccountTag::Calendar`](crate::state::AccountTag::Calendar).
        pub tag: u8,
        /// [`VERSION`](crate::state::VERSION).
        pub version: u8,
        /// Canonical bump of the PDA.
        pub bump: u8,
        /// `Underlying` discriminant.
        pub underlying: u8,
        /// Number of valid entries in `expiries`, at most [`CALENDAR_CAPACITY`].
        pub count: u8,
        /// Padding.
        pub _pad: [u8; 3],
        /// Expiry timestamps in seconds, strictly increasing in the first `count` entries; the
        /// rest are zero.
        pub expiries: [i64; CALENDAR_CAPACITY],
    }

    /// One option series: terms, snapshot of the config it needs, and vault accounting. PDA
    /// `["series", [underlying], [kind], strike_le, expiry_le, cap_le, multiplier_le]`. 216 bytes.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Pod, Zeroable)]
    #[repr(C)]
    pub struct Series {
        /// [`AccountTag::Series`](crate::state::AccountTag::Series).
        pub tag: u8,
        /// [`VERSION`](crate::state::VERSION).
        pub version: u8,
        /// Canonical bump of the series PDA.
        pub bump: u8,
        /// Canonical bump of the long mint PDA.
        pub long_mint_bump: u8,
        /// `SeriesState` discriminant.
        pub state: u8,
        /// `Underlying` discriminant.
        pub underlying: u8,
        /// `OptionKind` discriminant.
        pub kind: u8,
        /// Decimals of the USDC mint, copied at creation for `transfer_checked`.
        pub usdc_decimals: u8,
        /// Contract multiplier in milli-ounces.
        pub multiplier: u32,
        /// Padding.
        pub _pad: [u8; 4],
        /// Strike, USDC atoms per ounce.
        pub strike: u64,
        /// Cap, USDC atoms per ounce (zero for a put).
        pub cap: u64,
        /// Expiry timestamp, seconds.
        pub expiry: i64,
        /// Snapshot: the USDC mint.
        pub usdc_mint: [u8; 32],
        /// Snapshot: the token program.
        pub token_program: [u8; 32],
        /// The long token mint.
        pub long_mint: [u8; 32],
        /// The collateral vault (associated token account of the series for `usdc_mint`).
        pub vault: [u8; 32],
        /// Contracts written and not closed.
        pub open: u64,
        /// Long tokens not redeemed.
        pub longs_outstanding: u64,
        /// Written contracts whose residual is not claimed.
        pub writers_unclaimed: u64,
        /// Payout per long token, USDC atoms (zero until settlement).
        pub payout: u64,
        /// Residual per written contract, USDC atoms (zero until settlement).
        pub residual: u64,
        /// USDC the vault owes, atoms.
        pub vault_owed: u64,
    }

    /// One writer's position in one series. PDA `["writer", series, owner]`. 80 bytes.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Pod, Zeroable)]
    #[repr(C)]
    pub struct Writer {
        /// [`AccountTag::Writer`](crate::state::AccountTag::Writer).
        pub tag: u8,
        /// [`VERSION`](crate::state::VERSION).
        pub version: u8,
        /// Canonical bump of the PDA.
        pub bump: u8,
        /// `1` once the residual is claimed (milestone 4).
        pub claimed: u8,
        /// Padding.
        pub _pad: [u8; 4],
        /// The series.
        pub series: [u8; 32],
        /// The writer's wallet.
        pub owner: [u8; 32],
        /// Contracts written and not closed.
        pub written: u64,
    }
}

pub use layout::{Calendar, Config, Series, Writer};

impl Tagged for Config {
    const TAG: AccountTag = AccountTag::Config;
    fn tag(&self) -> u8 {
        self.tag
    }
    fn version(&self) -> u8 {
        self.version
    }
}

impl Tagged for Calendar {
    const TAG: AccountTag = AccountTag::Calendar;
    fn tag(&self) -> u8 {
        self.tag
    }
    fn version(&self) -> u8 {
        self.version
    }
}

impl Calendar {
    /// Whether `expiry` is one of the first `count` entries.
    #[must_use]
    pub fn contains(&self, expiry: i64) -> bool {
        self.expiries
            .iter()
            .take(usize::from(self.count))
            .any(|&e| e == expiry)
    }
}

impl Tagged for Series {
    const TAG: AccountTag = AccountTag::Series;
    fn tag(&self) -> u8 {
        self.tag
    }
    fn version(&self) -> u8 {
        self.version
    }
}

impl Series {
    /// The validated key.
    ///
    /// # Errors
    ///
    /// `InvalidAccountData` for a bad discriminant, or the core validation error.
    #[must_use = "the result carries a possible error"]
    pub fn key(&self) -> Result<SeriesKey, MetalbookError> {
        Ok(SeriesKey::try_new(
            Underlying::try_from(self.underlying)?,
            OptionKind::try_from(self.kind)?,
            StrikeUsd::new(self.strike),
            UnixTimestamp::new(self.expiry),
        )?)
    }

    /// The validated terms.
    ///
    /// # Errors
    ///
    /// As [`Series::key`], or the core validation error of the terms.
    #[must_use = "the result carries a possible error"]
    pub fn terms(&self) -> Result<SeriesTerms, MetalbookError> {
        Ok(SeriesTerms::try_new(
            self.key()?,
            OuncesPerContract::try_new(self.multiplier)?,
            StrikeUsd::new(self.cap),
        )?)
    }

    /// The validated vault accounting.
    ///
    /// # Errors
    ///
    /// As [`Series::terms`], or `InvariantViolated` if the stored accounting is inconsistent.
    #[must_use = "the result carries a possible error"]
    pub fn ledger(&self) -> Result<SeriesLedger, MetalbookError> {
        let parts = LedgerParts {
            state: SeriesState::try_from(self.state)?,
            open: Contracts::new(self.open),
            longs_outstanding: Contracts::new(self.longs_outstanding),
            writers_unclaimed: Contracts::new(self.writers_unclaimed),
            payout: UsdcAtoms::new(self.payout),
            residual: UsdcAtoms::new(self.residual),
            vault: UsdcAtoms::new(self.vault_owed),
        };
        Ok(SeriesLedger::restore(self.terms()?, parts)?)
    }

    /// Stores the accounting of `ledger`.
    pub fn store_ledger(&mut self, ledger: &SeriesLedger) {
        let p = ledger.parts();
        self.state = p.state as u8;
        self.open = p.open.get();
        self.longs_outstanding = p.longs_outstanding.get();
        self.writers_unclaimed = p.writers_unclaimed.get();
        self.payout = p.payout.get();
        self.residual = p.residual.get();
        self.vault_owed = p.vault.get();
    }
}

impl Tagged for Writer {
    const TAG: AccountTag = AccountTag::Writer;
    fn tag(&self) -> u8 {
        self.tag
    }
    fn version(&self) -> u8 {
        self.version
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::disallowed_macros
)]
mod tests {
    use super::*;

    #[test]
    fn test_account_sizes_are_fixed() {
        assert_eq!(core::mem::size_of::<Config>(), 144);
        assert_eq!(core::mem::size_of::<Calendar>(), 520);
        assert_eq!(core::mem::size_of::<Series>(), 216);
        assert_eq!(core::mem::size_of::<Writer>(), 80);
        assert_eq!(core::mem::align_of::<Series>(), 8);
    }
}
