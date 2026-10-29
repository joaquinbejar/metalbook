//! The vanilla pricer boundary.

use rust_decimal::Decimal;

use crate::MakerError;

/// A European vanilla option's kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VanillaKind {
    /// Pays `max(S − K, 0)`.
    Call,
    /// Pays `max(K − S, 0)`.
    Put,
}

/// Inputs of one vanilla price, per troy ounce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VanillaInputs {
    /// Spot, USD per ounce.
    pub spot: Decimal,
    /// Strike, USD per ounce.
    pub strike: Decimal,
    /// Time to expiry, years (365-day calendar time).
    pub years: Decimal,
    /// Annualized volatility (`0.20` is 20%).
    pub vol: Decimal,
    /// Continuously compounded risk-free rate.
    pub rate: Decimal,
    /// Continuous carry (dividend yield or lease rate).
    pub carry: Decimal,
}

/// A vanilla price and its sensitivities, per ounce, in the pricer's greek conventions (the
/// adapter only adds, subtracts and scales them, so any consistent convention works).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VanillaValue {
    /// Value, USD per ounce.
    pub value: Decimal,
    /// Delta.
    pub delta: Decimal,
    /// Gamma.
    pub gamma: Decimal,
    /// Vega.
    pub vega: Decimal,
    /// Theta.
    pub theta: Decimal,
    /// Absolute bound on the value's error, USD per ounce (non-negative).
    pub error_bound: Decimal,
}

/// Prices European vanillas: [`crate::OptfixPricer`]; tests also use OptionStratLib.
pub trait VanillaPricer {
    /// Value and greeks of one vanilla.
    ///
    /// # Errors
    ///
    /// `MakerError::Pricer` (or any other variant) if the pricer cannot price the inputs.
    fn price(&self, kind: VanillaKind, inputs: &VanillaInputs) -> Result<VanillaValue, MakerError>;
}
