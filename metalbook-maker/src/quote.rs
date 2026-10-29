//! Two-sided quotes in USDC atoms per long token.

use rust_decimal::{Decimal, RoundingStrategy};

use crate::mark::{add, sub};
use crate::{MakerError, Mark};

/// A bid and an ask for one long token, USDC atoms, both multiples of the tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TwoSided {
    /// What the maker pays for one long token.
    pub bid: u64,
    /// What the maker charges for one long token.
    pub ask: u64,
}

/// Quotes around `mark`: `bid = value − error − half_spread` rounded **down** to the tick,
/// `ask = value + error + half_spread` rounded **up** (both toward the maker), the bid never
/// below zero and the ask never above the maximum payout rounded down to the tick (nobody
/// should pay more than a long token can ever return).
///
/// # Errors
///
/// `InvalidInput` for a zero tick or a negative half spread, `Overflow` if an amount does not
/// fit a `u64` of atoms.
pub fn quote_atoms(
    mark: &Mark,
    half_spread: Decimal,
    tick_atoms: u64,
) -> Result<TwoSided, MakerError> {
    if tick_atoms == 0 {
        return Err(MakerError::InvalidInput("tick must be positive"));
    }
    if half_spread < Decimal::ZERO {
        return Err(MakerError::InvalidInput("half spread must not be negative"));
    }
    let width = add(mark.error_bound, half_spread)?;
    let bid_usd = sub(mark.value, width)?.max(Decimal::ZERO);
    let ask_usd = add(mark.value, width)?;
    let bid = to_ticks(bid_usd, tick_atoms, Dir::Down)?;
    let ask = to_ticks(ask_usd, tick_atoms, Dir::Up)?;
    let ceiling = to_ticks(mark.max_payout, tick_atoms, Dir::Down)?;
    let ask = ask.min(ceiling);
    Ok(TwoSided {
        bid: bid.min(ask),
        ask,
    })
}

/// Rounding direction of one conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dir {
    /// Toward negative infinity.
    Down,
    /// Toward positive infinity.
    Up,
}

/// USD to USDC atoms on the tick grid, rounded in direction `dir`, exactly.
///
/// No `Decimal` quotient is ever rounded: the amount is first rounded to whole atoms in
/// direction `dir` on its exact decimal representation (`round_dp`, which only drops digits),
/// read as an integer from its mantissa and scale, and then put on the tick grid with integer
/// floor or ceiling division. `floor(floor(x) / t) == floor(x / t)` and likewise for the
/// ceiling, so the two steps round the exact value once.
fn to_ticks(usd: Decimal, tick_atoms: u64, dir: Dir) -> Result<u64, MakerError> {
    let strategy = match dir {
        Dir::Down => RoundingStrategy::ToNegativeInfinity,
        Dir::Up => RoundingStrategy::ToPositiveInfinity,
    };
    let rounded = usd.round_dp_with_strategy(USDC_DECIMALS, strategy);
    let shift = USDC_DECIMALS
        .checked_sub(rounded.scale())
        .ok_or(MakerError::Overflow)?;
    let atoms = rounded
        .mantissa()
        .checked_mul(10i128.checked_pow(shift).ok_or(MakerError::Overflow)?)
        .ok_or(MakerError::Overflow)?;
    let atoms = u128::try_from(atoms).map_err(|_| MakerError::InvalidInput("negative amount"))?;
    let tick = u128::from(tick_atoms);
    let ticks = match dir {
        Dir::Down => atoms.checked_div(tick),
        Dir::Up => atoms
            .checked_add(tick.checked_sub(1).ok_or(MakerError::Overflow)?)
            .and_then(|a| a.checked_div(tick)),
    }
    .ok_or(MakerError::Overflow)?;
    let on_grid = ticks.checked_mul(tick).ok_or(MakerError::Overflow)?;
    u64::try_from(on_grid).map_err(|_| MakerError::Overflow)
}

/// USDC decimals.
const USDC_DECIMALS: u32 = 6;
