//! Series identity and terms, and the stored enums.

use crate::{CoreError, OuncesPerContract, StrikeUsd, UnixTimestamp};

/// The metal an option is written on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Underlying {
    /// Gold, Pyth feed `Metal.XAU/USD`.
    Xau = 0,
    /// Silver, Pyth feed `Metal.XAG/USD`.
    Xag = 1,
}

impl TryFrom<u8> for Underlying {
    type Error = CoreError;

    /// # Errors
    ///
    /// `InvalidDiscriminant` for any value other than 0 or 1.
    #[inline]
    fn try_from(value: u8) -> Result<Self, CoreError> {
        match value {
            0 => Ok(Self::Xau),
            1 => Ok(Self::Xag),
            _ => Err(CoreError::InvalidDiscriminant),
        }
    }
}

/// Call or put. Defined here rather than taken from optfix so that the domain and payoff
/// math do not depend on the pricing library; marks arrive in milestone 5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum OptionKind {
    /// Pays `min(S − K, cap − K)` per ounce when positive (a capped call).
    Call = 0,
    /// Pays `K − S` per ounce when positive.
    Put = 1,
}

impl TryFrom<u8> for OptionKind {
    type Error = CoreError;

    /// # Errors
    ///
    /// `InvalidDiscriminant` for any value other than 0 or 1.
    #[inline]
    fn try_from(value: u8) -> Result<Self, CoreError> {
        match value {
            0 => Ok(Self::Call),
            1 => Ok(Self::Put),
            _ => Err(CoreError::InvalidDiscriminant),
        }
    }
}

/// Lifecycle of a series (`docs/PLAN.md`, section 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum SeriesState {
    /// Before expiry: writers mint and close.
    Active = 0,
    /// Past expiry, awaiting a settlement price.
    Expired = 1,
    /// Settlement price fixed: longs redeem, writers claim.
    Settled = 2,
    /// No price could be found by the hard deadline: collateral returns to writers.
    Voided = 3,
}

impl TryFrom<u8> for SeriesState {
    type Error = CoreError;

    /// # Errors
    ///
    /// `InvalidDiscriminant` for any value above 3.
    #[inline]
    fn try_from(value: u8) -> Result<Self, CoreError> {
        match value {
            0 => Ok(Self::Active),
            1 => Ok(Self::Expired),
            2 => Ok(Self::Settled),
            3 => Ok(Self::Voided),
            _ => Err(CoreError::InvalidDiscriminant),
        }
    }
}

/// What identifies a series apart from its contract terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SeriesKey {
    underlying: Underlying,
    kind: OptionKind,
    strike: StrikeUsd,
    expiry: UnixTimestamp,
}

impl SeriesKey {
    /// Checks and builds a series key.
    ///
    /// # Errors
    ///
    /// `InvalidStrike` if the strike is zero.
    #[must_use = "the result carries a possible error"]
    #[inline]
    pub fn try_new(
        underlying: Underlying,
        kind: OptionKind,
        strike: StrikeUsd,
        expiry: UnixTimestamp,
    ) -> Result<Self, CoreError> {
        if strike.get() == 0 {
            return Err(CoreError::InvalidStrike);
        }
        Ok(Self {
            underlying,
            kind,
            strike,
            expiry,
        })
    }

    /// The metal.
    #[must_use]
    #[inline]
    pub const fn underlying(&self) -> Underlying {
        self.underlying
    }

    /// Call or put.
    #[must_use]
    #[inline]
    pub const fn kind(&self) -> OptionKind {
        self.kind
    }

    /// The strike `K`, in USDC atoms per ounce. Never zero.
    #[must_use]
    #[inline]
    pub const fn strike(&self) -> StrikeUsd {
        self.strike
    }

    /// The expiry timestamp.
    #[must_use]
    #[inline]
    pub const fn expiry(&self) -> UnixTimestamp {
        self.expiry
    }
}

/// Everything that fixes a series' payoff: its key, the contract multiplier and, for calls,
/// the cap.
///
/// The constructor guarantees that the maximum payout per contract fits a `u64` of USDC
/// atoms, so the per-contract formulas in this crate cannot overflow for a valid series.
/// Rounding favors the protocol: the maximum payout (the collateral per contract) is
/// rounded up and the settlement payout is rounded down; the difference, including the
/// rounding dust, is the writer's residual.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SeriesTerms {
    key: SeriesKey,
    multiplier: OuncesPerContract,
    cap: StrikeUsd,
}

impl SeriesTerms {
    /// Checks and builds the terms of a series.
    ///
    /// # Errors
    ///
    /// - `InvalidCap` if a call's `cap` is not above its strike, or a put's `cap` is not zero.
    /// - `Overflow` if the maximum payout per contract does not fit a `u64` of USDC atoms.
    #[must_use = "the result carries a possible error"]
    pub fn try_new(
        key: SeriesKey,
        multiplier: OuncesPerContract,
        cap: StrikeUsd,
    ) -> Result<Self, CoreError> {
        let cap_ok = match key.kind() {
            OptionKind::Call => cap > key.strike(),
            OptionKind::Put => cap.get() == 0,
        };
        if !cap_ok {
            return Err(CoreError::InvalidCap);
        }
        let terms = Self {
            key,
            multiplier,
            cap,
        };
        terms.max_payout_per_contract()?;
        Ok(terms)
    }

    /// The series key.
    #[must_use]
    #[inline]
    pub const fn key(&self) -> &SeriesKey {
        &self.key
    }

    /// The contract multiplier.
    #[must_use]
    #[inline]
    pub const fn multiplier(&self) -> OuncesPerContract {
        self.multiplier
    }

    /// The cap in USDC atoms per ounce: above the strike for a call, zero for a put.
    #[must_use]
    #[inline]
    pub const fn cap(&self) -> StrikeUsd {
        self.cap
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

    fn key(kind: OptionKind, strike: u64) -> SeriesKey {
        SeriesKey::try_new(
            Underlying::Xau,
            kind,
            StrikeUsd::new(strike),
            UnixTimestamp::new(1_800_000_000),
        )
        .unwrap()
    }

    #[test]
    fn test_enum_discriminants_round_trip() {
        for u in [Underlying::Xau, Underlying::Xag] {
            assert_eq!(Underlying::try_from(u as u8), Ok(u));
        }
        for k in [OptionKind::Call, OptionKind::Put] {
            assert_eq!(OptionKind::try_from(k as u8), Ok(k));
        }
        for s in [
            SeriesState::Active,
            SeriesState::Expired,
            SeriesState::Settled,
            SeriesState::Voided,
        ] {
            assert_eq!(SeriesState::try_from(s as u8), Ok(s));
        }
    }

    #[test]
    fn test_enum_unknown_discriminant_is_rejected() {
        assert_eq!(Underlying::try_from(2), Err(CoreError::InvalidDiscriminant));
        assert_eq!(OptionKind::try_from(2), Err(CoreError::InvalidDiscriminant));
        assert_eq!(
            SeriesState::try_from(4),
            Err(CoreError::InvalidDiscriminant)
        );
    }

    #[test]
    fn test_series_key_zero_strike_is_rejected() {
        let k = SeriesKey::try_new(
            Underlying::Xag,
            OptionKind::Put,
            StrikeUsd::new(0),
            UnixTimestamp::new(0),
        );
        assert_eq!(k, Err(CoreError::InvalidStrike));
    }

    #[test]
    fn test_series_terms_call_cap_must_exceed_strike() {
        let k = key(OptionKind::Call, 4_000_000_000);
        let m = OuncesPerContract::ONE_OUNCE;
        for cap in [0, 3_999_999_999, 4_000_000_000] {
            assert_eq!(
                SeriesTerms::try_new(k, m, StrikeUsd::new(cap)),
                Err(CoreError::InvalidCap)
            );
        }
        assert!(SeriesTerms::try_new(k, m, StrikeUsd::new(4_000_000_001)).is_ok());
    }

    #[test]
    fn test_series_terms_put_cap_must_be_zero() {
        let k = key(OptionKind::Put, 4_000_000_000);
        let m = OuncesPerContract::ONE_OUNCE;
        assert_eq!(
            SeriesTerms::try_new(k, m, StrikeUsd::new(1)),
            Err(CoreError::InvalidCap)
        );
        assert!(SeriesTerms::try_new(k, m, StrikeUsd::new(0)).is_ok());
    }

    #[test]
    fn test_series_terms_unrepresentable_max_payout_is_rejected() {
        let k = key(OptionKind::Put, u64::MAX);
        let m = OuncesPerContract::try_new(1_001).unwrap();
        assert_eq!(
            SeriesTerms::try_new(k, m, StrikeUsd::new(0)),
            Err(CoreError::Overflow)
        );
    }
}
