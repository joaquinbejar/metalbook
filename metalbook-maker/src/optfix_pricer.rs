//! [`VanillaPricer`] over optfix's Black-Scholes-Merton (binary Q64.64 fixed point).
//!
//! Decimal inputs are converted to Q64.64 rounding down (each moves by less than one ulp,
//! `2^-64`), priced with `optfix::price_and_greeks`, and converted back to decimals with 18
//! places (fewer for a greek too large for a `Decimal` at 18). The value is rounded down and
//! its [`VanillaValue::error_bound`] is the sum of:
//!
//! - optfix's price envelope at the converted inputs (`optfix::price_error_bound`, rounded up);
//! - the output rounding, `10^-18`;
//! - the effect of the input rounding, bounded over the whole ulp interval rather than from
//!   the greeks at one point. With `D = e^3 > max(Dq, Dr)` over optfix's domain
//!   (`r, q >= -0.1`, `T <= 30`): `|dV/dS| <= Dq` and `|dV/dK| <= Dr`; vega
//!   `S Dq n(d1) √T <= S D (1 + T)`; `|dV/dr| <= K T Dr` and `|dV/dq| <= S T Dq`; and in `T`,
//!   `|dV/dT| <= S Dq σ n(d1) / (2√T) + |r| K Dr + |q| S Dq` with `n <= 0.4`. Its first term
//!   integrates over `[T', T]` (`T'` the rounded value, `T − T' < ε`) to at most
//!   `0.4 S D σ min(√ε, ε / (2√T'))`: `√T − √T' <= √ε` always, and `1/(2√t)` is largest at
//!   `t = T'`, so a `T` near zero is covered too. Each partial is taken at its sup over the
//!   interval, so the bound is not a first-order estimate. At metal prices and `T` of days or
//!   more this input term is about `10^-15` dollars per ounce; only for `T` within a few ulps
//!   of zero does the `√ε` form, about `10^-7`, apply.
//!
//! In total the bound is dominated by optfix's envelope: about `8.7 × 10^-12` dollars per
//! ounce at `S, K` around 2400.
//!
//! The greeks are optfix's, within its published envelopes (`optfix::DELTA_ERR` and the
//! relative ones); the error bound covers the value only, as the trait specifies. Greek
//! conventions: vega per 1.00 of volatility, theta `dV/dt` per calendar year.

use optfix::{
    price_and_greeks, price_error_bound, EuropeanOption, Fixed, MarketInputs, OptionKind, Rate,
    Rounding, Spot, Strike, Volatility, Years,
};
use rust_decimal::Decimal;

use crate::pricer::{VanillaInputs, VanillaKind, VanillaPricer, VanillaValue};
use crate::MakerError;

/// Decimal places of the outputs (a `Decimal` mantissa holds `7.9 × 10^28`, so 18 places fit
/// values up to `7.9 × 10^10`).
const OUT_DECIMALS: u32 = 18;

/// One Q64.64 ulp, `2^-64 = 5.42101086242752217003... × 10^-20`, rounded up.
const ULP: Decimal = Decimal::from_parts(542_101_087, 0, 0, false, 28);

/// `√(2^-64) = 2^-32 = 2.3283064365386962890625 × 10^-10`, rounded up.
const SQRT_ULP: Decimal = Decimal::from_parts(232_830_644, 0, 0, false, 18);

/// `D = 21 > e^3`, the largest discount factor over optfix's domain.
const D: Decimal = Decimal::from_parts(21, 0, 0, false, 0);

/// `0.4 > 1/√(2π) = 0.3989...`, the normal density's maximum.
const N_MAX: Decimal = Decimal::from_parts(4, 0, 0, false, 1);

/// The optfix Black-Scholes-Merton pricer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OptfixPricer;

fn pricer_error(what: &'static str) -> impl Fn(optfix::PricingError) -> MakerError {
    move |e| MakerError::Pricer(format!("optfix {what}: {e:?}"))
}

/// A decimal as Q64.64, rounded down (less than one ulp below).
fn to_fixed(d: Decimal, what: &'static str) -> Result<Fixed, MakerError> {
    let scale = u8::try_from(d.scale()).map_err(|_| MakerError::InvalidInput(what))?;
    Fixed::from_scaled(d.mantissa(), scale, Rounding::Down).map_err(pricer_error(what))
}

/// A Q64.64 value as a decimal with [`OUT_DECIMALS`] places in direction `r`, or with fewer
/// places when it does not fit (greeks only; the value always fits).
fn to_decimal(f: Fixed, r: Rounding) -> Result<Decimal, MakerError> {
    let mut places = OUT_DECIMALS;
    loop {
        let decimals = u8::try_from(places).map_err(|_| MakerError::Overflow)?;
        let scaled = f
            .to_scaled(decimals, r)
            .map_err(pricer_error("output conversion"))?;
        if let Ok(d) = Decimal::try_from_i128_with_scale(scaled, places) {
            return Ok(d);
        }
        places = places.checked_sub(1).ok_or(MakerError::Overflow)?;
    }
}

fn add(a: Decimal, b: Decimal) -> Result<Decimal, MakerError> {
    a.checked_add(b).ok_or(MakerError::Overflow)
}

fn mul(a: Decimal, b: Decimal) -> Result<Decimal, MakerError> {
    a.checked_mul(b).ok_or(MakerError::Overflow)
}

/// The bound on `|V(inputs) − V(rounded inputs)|` described in the module documentation.
/// `years` is the rounded (Q64.64) time, the lower end of the interval.
fn input_rounding_bound(i: &VanillaInputs, years: Fixed) -> Result<Decimal, MakerError> {
    let (s, k, t) = (i.spot, i.strike, i.years);
    let one_plus_t = add(Decimal::ONE, t)?;
    // Per ulp of each input: S and K, σ (vega), r and q (rho), and the |r| K + |q| S part of T.
    let per_ulp = [
        Decimal::ONE,
        Decimal::ONE,
        mul(s, one_plus_t)?,
        mul(k, t)?,
        mul(s, t)?,
        mul(i.rate.abs(), k)?,
        mul(i.carry.abs(), s)?,
    ]
    .into_iter()
    .try_fold(Decimal::ZERO, add)?;
    let linear = mul(mul(D, ULP)?, per_ulp)?;
    // The √T part of dV/dT: 0.4 S D (σ + ε) min(√ε, ε / (2√T')), σ taken one ulp up for the
    // interval. √T' is rounded down, so ε / (2√T') is rounded up.
    let root = years.sqrt_down().map_err(pricer_error("sqrt"))?;
    let root = to_decimal(root, Rounding::Down)?;
    let mut growth = SQRT_ULP;
    if root > Decimal::ZERO {
        let ratio = ULP
            .checked_div(mul(Decimal::TWO, root)?)
            .ok_or(MakerError::Overflow)?;
        // The division rounded; one unit in its last place keeps it an upper bound.
        let ratio = add(ratio, Decimal::new(1, ratio.scale()))?;
        growth = growth.min(ratio);
    }
    let sigma = add(i.vol, ULP)?;
    let sqrt_part = mul(mul(mul(mul(N_MAX, s)?, D)?, sigma)?, growth)?;
    add(linear, sqrt_part)
}

impl VanillaPricer for OptfixPricer {
    fn price(&self, kind: VanillaKind, i: &VanillaInputs) -> Result<VanillaValue, MakerError> {
        let years = Years::try_new(to_fixed(i.years, "years")?).map_err(pricer_error("years"))?;
        let opt = EuropeanOption {
            kind: match kind {
                VanillaKind::Call => OptionKind::Call,
                VanillaKind::Put => OptionKind::Put,
            },
            strike: Strike::try_new(to_fixed(i.strike, "strike")?)
                .map_err(pricer_error("strike"))?,
            expiry: years,
        };
        let m = MarketInputs {
            spot: Spot::try_new(to_fixed(i.spot, "spot")?).map_err(pricer_error("spot"))?,
            vol: Volatility::try_new(to_fixed(i.vol, "vol")?).map_err(pricer_error("vol"))?,
            rate: Rate::try_new(to_fixed(i.rate, "rate")?).map_err(pricer_error("rate"))?,
            carry: Rate::try_new(to_fixed(i.carry, "carry")?).map_err(pricer_error("carry"))?,
        };
        let priced = price_and_greeks(&opt, &m).map_err(pricer_error("price"))?;
        let envelope = price_error_bound(&opt, &m).map_err(pricer_error("price envelope"))?;
        let error_bound = [
            to_decimal(envelope, Rounding::Up)?,
            Decimal::new(1, OUT_DECIMALS),
            input_rounding_bound(i, years.get())?,
        ]
        .into_iter()
        .try_fold(Decimal::ZERO, add)?;
        let g = priced.greeks;
        Ok(VanillaValue {
            value: to_decimal(priced.price.get(), Rounding::Down)?,
            delta: to_decimal(g.delta, Rounding::Down)?,
            gamma: to_decimal(g.gamma, Rounding::Down)?,
            vega: to_decimal(g.vega, Rounding::Down)?,
            theta: to_decimal(g.theta, Rounding::Down)?,
            error_bound,
        })
    }
}
