//! Program error codes.

use metalbook_core::CoreError;
use pinocchio::error::ProgramError;

/// Every error the program returns, as `ProgramError::Custom(code)`. Codes start at 100
/// (matchbook convention) and never change; new variants are appended.
///
/// Codes 119 to 125 and 127 are reserved for the oracle (milestone 3) and pricing
/// (milestone 5) checks of `docs/PLAN.md`; nothing returns them yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum MetalbookError {
    /// Empty instruction data or an unknown tag.
    InvalidInstruction = 100,
    /// Instruction data of the wrong length, or a field out of range.
    InvalidInstructionData = 101,
    /// An account that must sign did not.
    MissingSignature = 102,
    /// An account is not owned by the program that must own it.
    InvalidAccountOwner = 103,
    /// An account is not the program-derived address its seeds give.
    InvalidPda = 104,
    /// A mint is not the one the series or the config names.
    InvalidMint = 105,
    /// The vault is not the series' vault.
    InvalidVault = 106,
    /// A sysvar account is not the expected one.
    InvalidSysvar = 107,
    /// The signer is not the config admin or the program's upgrade authority.
    NotAuthority = 108,
    /// The series account does not exist.
    SeriesNotListed = 109,
    /// The expiry is not in the calendar of the underlying.
    ExpiryNotInCalendar = 110,
    /// The strike is not a multiple of the underlying's strike tick.
    StrikeOffGrid = 111,
    /// A call's cap is not on the cap grid, or a put's cap is not zero.
    InvalidCap = 112,
    /// The series is at or past its expiry.
    SeriesExpired = 113,
    /// The series has not expired yet.
    SeriesNotExpired = 114,
    /// The series is not settled.
    SeriesNotSettled = 115,
    /// The series is already settled.
    SeriesAlreadySettled = 116,
    /// Not enough collateral for the operation.
    InsufficientCollateral = 117,
    /// More long tokens than outstanding.
    InsufficientLongTokens = 118,
    /// Reserved (oracle, milestone 3).
    OracleOwnerMismatch = 119,
    /// Reserved (oracle, milestone 3).
    OracleNotFullyVerified = 120,
    /// Reserved (oracle, milestone 3).
    OracleFeedMismatch = 121,
    /// Reserved (oracle, milestone 3).
    OracleStale = 122,
    /// Reserved (oracle, milestone 3).
    OracleConfidenceTooWide = 123,
    /// Reserved (oracle, milestone 3).
    SampleOutsideWindow = 124,
    /// Reserved (oracle, milestone 3).
    NotEnoughSamples = 125,
    /// An amount does not fit its integer type.
    Overflow = 126,
    /// Reserved (optfix pricing error, milestone 5).
    Pricing = 127,
    /// Account data of the wrong length, discriminator or version.
    InvalidAccountData = 128,
    /// A program account (token, system, associated token) is not the expected program.
    InvalidProgram = 129,
    /// A token account has the wrong mint or owner, or is not initialized.
    InvalidTokenAccount = 130,
    /// An account that must be writable is not.
    AccountNotWritable = 131,
    /// The account to create already exists.
    AlreadyInitialized = 132,
    /// Config parameters out of range.
    InvalidConfig = 133,
    /// The multiplier is not the one configured for the underlying.
    InvalidMultiplier = 134,
    /// The expiry is sooner than the minimum listing time from now.
    ListingTooLate = 135,
    /// More contracts than the writer has written and not closed.
    InsufficientWritten = 136,
    /// Zero contracts.
    ZeroAmount = 137,
    /// The vault accounting does not match the series state. Never expected.
    InvariantViolated = 138,
    /// The strike is zero.
    InvalidStrike = 139,
    /// The series is not active.
    SeriesNotActive = 140,
}

impl From<MetalbookError> for ProgramError {
    #[cold]
    fn from(e: MetalbookError) -> Self {
        ProgramError::Custom(e as u32)
    }
}

impl From<CoreError> for MetalbookError {
    #[cold]
    fn from(e: CoreError) -> Self {
        match e {
            CoreError::Overflow => Self::Overflow,
            CoreError::InvalidDiscriminant => Self::InvalidAccountData,
            CoreError::InvalidStrike => Self::InvalidStrike,
            CoreError::InvalidMultiplier => Self::InvalidMultiplier,
            CoreError::InvalidCap => Self::InvalidCap,
            CoreError::ZeroAmount => Self::ZeroAmount,
            CoreError::SeriesNotActive => Self::SeriesNotActive,
            CoreError::SeriesNotExpired => Self::SeriesNotExpired,
            CoreError::SeriesNotSettled => Self::SeriesNotSettled,
            CoreError::InsufficientWritten => Self::InsufficientWritten,
            CoreError::InsufficientLongTokens => Self::InsufficientLongTokens,
            CoreError::InvariantViolated => Self::InvariantViolated,
        }
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
    use super::MetalbookError;

    #[test]
    fn test_metalbook_error_codes_are_stable() {
        assert_eq!(MetalbookError::InvalidInstruction as u32, 100);
        assert_eq!(MetalbookError::Overflow as u32, 126);
        assert_eq!(MetalbookError::Pricing as u32, 127);
        assert_eq!(MetalbookError::InvalidAccountData as u32, 128);
        assert_eq!(MetalbookError::SeriesNotActive as u32, 140);
    }
}
