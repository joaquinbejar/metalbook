//! Payoff-aware marks of a series, per contract.

use metalbook_core::{OptionKind, SeriesTerms, MILLI_OUNCES_PER_OUNCE};
use rust_decimal::Decimal;

use crate::{MakerError, VanillaInputs, VanillaKind, VanillaPricer, VanillaValue};

/// Seconds in the 365-day year used for time to expiry (calendar time, as optfix).
pub const SECONDS_PER_YEAR: i64 = 31_536_000;

/// USDC atoms per USD.
const ATOMS_PER_USD: i64 = 1_000_000;

/// Market inputs at the time of quoting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarketInputs {
    /// Spot, USD per ounce (the last Pyth print; its age is the caller's concern).
    pub spot: Decimal,
    /// Annualized volatility used for marks (never for settlement).
    pub vol: Decimal,
    /// Continuously compounded risk-free rate.
    pub rate: Decimal,
    /// Continuous carry (the metal's lease rate, or zero).
    pub carry: Decimal,
    /// Current Unix time, seconds.
    pub now: i64,
    /// Whether the metal's market is open now (from the Pyth schedule).
    pub market_open: bool,
}

/// The mark of one contract of a series, in USD and per contract (already scaled by the
/// multiplier). Greeks are in the pricer's conventions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mark {
    /// Value, clamped to `[0, max_payout]`.
    pub value: Decimal,
    /// Delta.
    pub delta: Decimal,
    /// Gamma.
    pub gamma: Decimal,
    /// Vega.
    pub vega: Decimal,
    /// Theta.
    pub theta: Decimal,
    /// Bound on the value's error: the sum of the legs' bounds, scaled.
    pub error_bound: Decimal,
    /// The contractual maximum payout of one contract (the exact one, before the protocol's
    /// rounding of the collateral).
    pub max_payout: Decimal,
}

/// Marks one contract of the series `terms`.
///
/// # Errors
///
/// - `MarketClosed` if `market.market_open` is false.
/// - `SettlementWindow` from `expiry − window_s` on (part of the settlement mean is known).
/// - `InvalidInput` for a non-positive spot or a negative volatility.
/// - The pricer's errors, `PricerInconsistent`, or `Overflow`.
pub fn mark<P: VanillaPricer>(
    pricer: &P,
    terms: &SeriesTerms,
    market: &MarketInputs,
    window_s: u32,
) -> Result<Mark, MakerError> {
    if !market.market_open {
        return Err(MakerError::MarketClosed);
    }
    let expiry = terms.key().expiry().get();
    let window_start = expiry
        .checked_sub(i64::from(window_s))
        .ok_or(MakerError::Overflow)?;
    if market.now >= window_start {
        return Err(MakerError::SettlementWindow);
    }
    if market.spot <= Decimal::ZERO {
        return Err(MakerError::InvalidInput("spot must be positive"));
    }
    if market.vol < Decimal::ZERO {
        return Err(MakerError::InvalidInput("volatility must not be negative"));
    }
    let seconds = expiry.checked_sub(market.now).ok_or(MakerError::Overflow)?;
    let years = div(Decimal::from(seconds), Decimal::from(SECONDS_PER_YEAR))?;
    let strike = usd(terms.key().strike().get())?;
    let inputs = |k: Decimal| VanillaInputs {
        spot: market.spot,
        strike: k,
        years,
        vol: market.vol,
        rate: market.rate,
        carry: market.carry,
    };

    // Per ounce: the leg combination of the payoff and its maximum.
    let (per_oz, bound_per_oz) = match terms.key().kind() {
        OptionKind::Put => (pricer.price(VanillaKind::Put, &inputs(strike))?, strike),
        OptionKind::Call => {
            let cap = usd(terms.cap().get())?;
            let long = pricer.price(VanillaKind::Call, &inputs(strike))?;
            let short = pricer.price(VanillaKind::Call, &inputs(cap))?;
            (spread(&long, &short)?, sub(cap, strike)?)
        }
    };
    if per_oz.error_bound < Decimal::ZERO {
        return Err(MakerError::Pricer("negative error bound".into()));
    }

    // Per contract: multiplier milli-ounces / 1000.
    let ounces = div(
        Decimal::from(terms.multiplier().get()),
        Decimal::from(MILLI_OUNCES_PER_OUNCE),
    )?;
    let raw = mul(per_oz.value, ounces)?;
    let error_bound = mul(per_oz.error_bound, ounces)?;
    let max_payout = mul(bound_per_oz, ounces)?;
    // A value outside the payoff's bounds by more than the pricer's own error is a pricer bug.
    let low = sub(Decimal::ZERO, error_bound)?;
    let high = add(max_payout, error_bound)?;
    if raw < low || raw > high {
        return Err(MakerError::PricerInconsistent);
    }
    Ok(Mark {
        value: raw.max(Decimal::ZERO).min(max_payout),
        delta: mul(per_oz.delta, ounces)?,
        gamma: mul(per_oz.gamma, ounces)?,
        vega: mul(per_oz.vega, ounces)?,
        theta: mul(per_oz.theta, ounces)?,
        error_bound,
        max_payout,
    })
}

/// `long − short` for value and greeks; the error bound is the **sum** of both bounds.
fn spread(long: &VanillaValue, short: &VanillaValue) -> Result<VanillaValue, MakerError> {
    Ok(VanillaValue {
        value: sub(long.value, short.value)?,
        delta: sub(long.delta, short.delta)?,
        gamma: sub(long.gamma, short.gamma)?,
        vega: sub(long.vega, short.vega)?,
        theta: sub(long.theta, short.theta)?,
        error_bound: add(long.error_bound.abs(), short.error_bound.abs())?,
    })
}

/// USDC atoms to USD, exact.
fn usd(atoms: u64) -> Result<Decimal, MakerError> {
    div(Decimal::from(atoms), Decimal::from(ATOMS_PER_USD))
}

pub(crate) fn add(a: Decimal, b: Decimal) -> Result<Decimal, MakerError> {
    a.checked_add(b).ok_or(MakerError::Overflow)
}

pub(crate) fn sub(a: Decimal, b: Decimal) -> Result<Decimal, MakerError> {
    a.checked_sub(b).ok_or(MakerError::Overflow)
}

pub(crate) fn mul(a: Decimal, b: Decimal) -> Result<Decimal, MakerError> {
    a.checked_mul(b).ok_or(MakerError::Overflow)
}

pub(crate) fn div(a: Decimal, b: Decimal) -> Result<Decimal, MakerError> {
    a.checked_div(b).ok_or(MakerError::Overflow)
}
