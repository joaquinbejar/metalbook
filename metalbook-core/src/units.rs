//! Quantity newtypes. Each wraps one integer and states its unit; there are no conversions
//! between them, so the compiler rejects a strike passed as a USDC amount.

use core::num::NonZeroU32;

use crate::CoreError;

/// An amount of USDC in atoms (6 decimals: `1_000_000` is 1 USDC).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(transparent)]
pub struct UsdcAtoms(u64);

impl UsdcAtoms {
    /// No USDC.
    pub const ZERO: Self = Self(0);

    /// Wraps an amount in atoms. Every `u64` is a valid amount.
    #[must_use]
    #[inline]
    pub const fn new(atoms: u64) -> Self {
        Self(atoms)
    }

    /// The amount in atoms.
    #[must_use]
    #[inline]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// `self + other`.
    ///
    /// # Errors
    ///
    /// `Overflow` if the sum exceeds `u64::MAX` atoms.
    #[must_use = "the result carries a possible error"]
    #[inline]
    pub fn checked_add(self, other: Self) -> Result<Self, CoreError> {
        self.0
            .checked_add(other.0)
            .map(Self)
            .ok_or(CoreError::Overflow)
    }

    /// `self - other`.
    ///
    /// # Errors
    ///
    /// `Overflow` if `other > self`.
    #[must_use = "the result carries a possible error"]
    #[inline]
    pub fn checked_sub(self, other: Self) -> Result<Self, CoreError> {
        self.0
            .checked_sub(other.0)
            .map(Self)
            .ok_or(CoreError::Overflow)
    }

    /// `self × n`: an amount per contract times a number of contracts. Exact, no rounding.
    ///
    /// # Errors
    ///
    /// `Overflow` if the product exceeds `u64::MAX` atoms.
    #[must_use = "the result carries a possible error"]
    #[inline]
    pub fn checked_mul_contracts(self, n: Contracts) -> Result<Self, CoreError> {
        self.0.checked_mul(n.0).map(Self).ok_or(CoreError::Overflow)
    }
}

/// A price in USDC atoms per troy ounce: a strike, a call's cap or a settlement price.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(transparent)]
pub struct StrikeUsd(u64);

impl StrikeUsd {
    /// Wraps a price in USDC atoms per ounce. Zero is representable (a put's cap is zero);
    /// [`crate::SeriesKey::try_new`] rejects a zero strike.
    #[must_use]
    #[inline]
    pub const fn new(atoms_per_ounce: u64) -> Self {
        Self(atoms_per_ounce)
    }

    /// The price in USDC atoms per ounce.
    #[must_use]
    #[inline]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// A number of option contracts, which is also a number of long tokens (0 decimals).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(transparent)]
pub struct Contracts(u64);

impl Contracts {
    /// No contracts.
    pub const ZERO: Self = Self(0);

    /// Wraps a number of contracts.
    #[must_use]
    #[inline]
    pub const fn new(n: u64) -> Self {
        Self(n)
    }

    /// The number of contracts.
    #[must_use]
    #[inline]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// `self + other`.
    ///
    /// # Errors
    ///
    /// `Overflow` if the sum exceeds `u64::MAX`.
    #[must_use = "the result carries a possible error"]
    #[inline]
    pub fn checked_add(self, other: Self) -> Result<Self, CoreError> {
        self.0
            .checked_add(other.0)
            .map(Self)
            .ok_or(CoreError::Overflow)
    }

    /// `self - other`.
    ///
    /// # Errors
    ///
    /// `Overflow` if `other > self`.
    #[must_use = "the result carries a possible error"]
    #[inline]
    pub fn checked_sub(self, other: Self) -> Result<Self, CoreError> {
        self.0
            .checked_sub(other.0)
            .map(Self)
            .ok_or(CoreError::Overflow)
    }
}

/// The contract multiplier in milli-ounces: `1_000` is one troy ounce per contract. Never
/// zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct OuncesPerContract(NonZeroU32);

impl OuncesPerContract {
    /// One troy ounce per contract.
    pub const ONE_OUNCE: Self = match NonZeroU32::new(1_000) {
        Some(v) => Self(v),
        // Unreachable: 1_000 is not zero. Written as a match because `const` has no `?`.
        None => Self(NonZeroU32::MIN),
    };

    /// Checks and wraps a multiplier in milli-ounces. Values read from storage must come
    /// back through this constructor.
    ///
    /// # Errors
    ///
    /// `InvalidMultiplier` if `milli_ounces` is zero.
    #[must_use = "the result carries a possible error"]
    #[inline]
    pub fn try_new(milli_ounces: u32) -> Result<Self, CoreError> {
        NonZeroU32::new(milli_ounces)
            .map(Self)
            .ok_or(CoreError::InvalidMultiplier)
    }

    /// The multiplier in milli-ounces.
    #[must_use]
    #[inline]
    pub const fn get(self) -> u32 {
        self.0.get()
    }
}

/// A Unix timestamp in seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct UnixTimestamp(i64);

impl UnixTimestamp {
    /// Wraps seconds since the Unix epoch.
    #[must_use]
    #[inline]
    pub const fn new(seconds: i64) -> Self {
        Self(seconds)
    }

    /// Seconds since the Unix epoch.
    #[must_use]
    #[inline]
    pub const fn get(self) -> i64 {
        self.0
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
    fn test_ounces_per_contract_one_ounce_is_1000() {
        assert_eq!(OuncesPerContract::ONE_OUNCE.get(), 1_000);
    }

    #[test]
    fn test_ounces_per_contract_zero_is_rejected() {
        assert_eq!(
            OuncesPerContract::try_new(0),
            Err(CoreError::InvalidMultiplier)
        );
        assert_eq!(OuncesPerContract::try_new(50_000).unwrap().get(), 50_000);
    }

    #[test]
    fn test_usdc_atoms_checked_ops_report_overflow() {
        let max = UsdcAtoms::new(u64::MAX);
        assert_eq!(max.checked_add(UsdcAtoms::new(1)), Err(CoreError::Overflow));
        assert_eq!(
            UsdcAtoms::ZERO.checked_sub(UsdcAtoms::new(1)),
            Err(CoreError::Overflow)
        );
        assert_eq!(
            max.checked_mul_contracts(Contracts::new(2)),
            Err(CoreError::Overflow)
        );
        assert_eq!(
            UsdcAtoms::new(7).checked_mul_contracts(Contracts::new(3)),
            Ok(UsdcAtoms::new(21))
        );
    }

    #[test]
    fn test_contracts_checked_ops_report_overflow() {
        assert_eq!(
            Contracts::new(u64::MAX).checked_add(Contracts::new(1)),
            Err(CoreError::Overflow)
        );
        assert_eq!(
            Contracts::ZERO.checked_sub(Contracts::new(1)),
            Err(CoreError::Overflow)
        );
    }
}
