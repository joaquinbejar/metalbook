//! Program error codes.

use metalbook_core::CoreError;
use pinocchio::error::ProgramError;
use pyth_lite::PythError;

/// Every error the program returns, as `ProgramError::Custom(code)`. Codes start at 100
/// (matchbook convention) and never change; new variants are appended.
///
/// Codes 125 (`NotEnoughSamples`) and 127 (`Pricing`, milestone 5) are reserved; nothing returns
/// them yet.
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
    /// The price update is not owned by the series' Pyth receiver program.
    OracleOwnerMismatch = 119,
    /// The price update is not `Full`-verified.
    OracleNotFullyVerified = 120,
    /// The price update is for another feed.
    OracleFeedMismatch = 121,
    /// The price update is too old (unused: settlement bounds time itself).
    OracleStale = 122,
    /// `conf / price` above the series' bound.
    OracleConfidenceTooWide = 123,
    /// The print qualifies as no settlement candidate (slot, chain link or fallback).
    SampleOutsideWindow = 124,
    /// Reserved.
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
    /// No settlement step is eligible yet: a candidate is inside its challenge delay, or the
    /// hard deadline has not passed.
    SettlementNotReady = 141,
    /// The price update's exponent is not the series' pinned exponent.
    OracleExponentMismatch = 142,
    /// A print with a non-positive price or `prev_publish_time <= 0`.
    InvalidPrint = 143,
    /// The writer position has already claimed its residual.
    AlreadyClaimed = 144,
    /// The series cannot be closed: long tokens or writer claims are outstanding, or the
    /// vault still owes USDC.
    SeriesNotEmpty = 145,
    /// The rent destination is not the account that paid the rent.
    InvalidRentPayer = 146,
    /// The writer position cannot be closed before it has claimed.
    PositionNotClaimed = 147,
}

impl From<MetalbookError> for ProgramError {
    #[cold]
    fn from(e: MetalbookError) -> Self {
        ProgramError::Custom(e as u32)
    }
}

impl From<PythError> for MetalbookError {
    #[cold]
    fn from(e: PythError) -> Self {
        match e {
            PythError::WrongOwner => Self::OracleOwnerMismatch,
            PythError::WrongDiscriminator
            | PythError::Truncated
            | PythError::UnknownVerificationLevel => Self::InvalidAccountData,
            PythError::NotFullyVerified => Self::OracleNotFullyVerified,
            PythError::WrongFeed => Self::OracleFeedMismatch,
            PythError::NonPositivePrice => Self::InvalidPrint,
            PythError::Stale => Self::OracleStale,
            PythError::ConfidenceTooWide => Self::OracleConfidenceTooWide,
            PythError::ArithmeticOverflow => Self::Overflow,
        }
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
            CoreError::InvalidOracleParams => Self::InvalidConfig,
            CoreError::InvalidPrint => Self::InvalidPrint,
            CoreError::PrintNotUsable => Self::SampleOutsideWindow,
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
        assert_eq!(MetalbookError::InvalidPrint as u32, 143);
    }
}
