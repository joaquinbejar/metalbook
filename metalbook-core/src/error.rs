//! The error type shared by every metalbook-core function.

use core::fmt;

/// Why a metalbook-core operation failed.
///
/// `#[repr(u8)]` with stable discriminants, so the on-chain program can map it into its own
/// error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum CoreError {
    /// An intermediate or final amount does not fit its integer type, or would go below zero.
    Overflow = 0,
    /// A stored enum discriminant is not one of the defined values.
    InvalidDiscriminant = 1,
    /// The strike is zero.
    InvalidStrike = 2,
    /// The contract multiplier is zero.
    InvalidMultiplier = 3,
    /// A call's cap is not above its strike, or a put's cap is not zero.
    InvalidCap = 4,
    /// A mint, close, redeem or claim of zero contracts.
    ZeroAmount = 5,
    /// The operation needs an `Active` series.
    SeriesNotActive = 6,
    /// The operation needs an `Expired` series.
    SeriesNotExpired = 7,
    /// The operation needs a `Settled` series.
    SeriesNotSettled = 8,
    /// A close or writer claim exceeds the contracts written and not yet closed or claimed.
    InsufficientWritten = 9,
    /// A redeem exceeds the long tokens outstanding.
    InsufficientLongTokens = 10,
    /// The vault accounting does not match the series state. Never expected; returned instead
    /// of asserting.
    InvariantViolated = 11,
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::Overflow => "amount does not fit its integer type",
            Self::InvalidDiscriminant => "stored enum discriminant is not defined",
            Self::InvalidStrike => "strike is zero",
            Self::InvalidMultiplier => "contract multiplier is zero",
            Self::InvalidCap => "call cap not above strike, or put cap not zero",
            Self::ZeroAmount => "amount of contracts is zero",
            Self::SeriesNotActive => "series is not active",
            Self::SeriesNotExpired => "series is not expired",
            Self::SeriesNotSettled => "series is not settled",
            Self::InsufficientWritten => "more contracts than written and still open",
            Self::InsufficientLongTokens => "more long tokens than outstanding",
            Self::InvariantViolated => "vault accounting does not match the series state",
        };
        f.write_str(text)
    }
}

impl core::error::Error for CoreError {}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::disallowed_macros
)]
mod tests {
    extern crate std;

    use super::CoreError;
    use std::string::ToString;

    const ALL: [CoreError; 12] = [
        CoreError::Overflow,
        CoreError::InvalidDiscriminant,
        CoreError::InvalidStrike,
        CoreError::InvalidMultiplier,
        CoreError::InvalidCap,
        CoreError::ZeroAmount,
        CoreError::SeriesNotActive,
        CoreError::SeriesNotExpired,
        CoreError::SeriesNotSettled,
        CoreError::InsufficientWritten,
        CoreError::InsufficientLongTokens,
        CoreError::InvariantViolated,
    ];

    #[test]
    fn test_core_error_discriminants_are_stable() {
        for (expected, e) in ALL.iter().enumerate() {
            assert_eq!(*e as u8 as usize, expected);
        }
    }

    #[test]
    fn test_core_error_display_is_not_empty() {
        for e in ALL {
            assert!(!e.to_string().is_empty());
        }
    }
}
