//! Two-sided quotes in USDC atoms per long token.

use rust_decimal::{Decimal, RoundingStrategy};

use crate::mark::{add, mul, sub};
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
    let bid = to_ticks(bid_usd, tick_atoms, RoundingStrategy::ToNegativeInfinity)?;
    let ask = to_ticks(ask_usd, tick_atoms, RoundingStrategy::ToPositiveInfinity)?;
    let ceiling = to_ticks(
        mark.max_payout,
        tick_atoms,
        RoundingStrategy::ToNegativeInfinity,
    )?;
    let ask = ask.min(ceiling);
    Ok(TwoSided {
        bid: bid.min(ask),
        ask,
    })
}

/// USD to USDC atoms on the tick grid, rounded with `strategy`.
fn to_ticks(usd: Decimal, tick_atoms: u64, strategy: RoundingStrategy) -> Result<u64, MakerError> {
    let atoms = mul(usd, Decimal::from(1_000_000u64))?;
    let ticks = atoms
        .checked_div(Decimal::from(tick_atoms))
        .ok_or(MakerError::Overflow)?
        .round_dp_with_strategy(0, strategy);
    let ticks = u64::try_from(ticks).map_err(|_| MakerError::Overflow)?;
    ticks.checked_mul(tick_atoms).ok_or(MakerError::Overflow)
}
