//! metalbook domain and payoff math.
//!
//! Cash-settled European calls and puts on gold (XAU/USD) and silver (XAG/USD), fully
//! collateralized in USDC. This crate holds the parts the on-chain program and the off-chain
//! services share: the series terms, the payoff and collateral formulas, and the vault
//! accounting of one series. It is `no_std`, allocation free, uses integers only and never
//! panics; every fallible operation returns a [`CoreError`].
//!
//! Units:
//! - [`UsdcAtoms`]: USDC amounts, 6 decimals.
//! - [`StrikeUsd`]: prices (strike, cap, settlement price) in USDC atoms per troy ounce.
//! - [`OuncesPerContract`]: the contract multiplier in milli-ounces (`1_000` is 1 oz).
//! - [`Contracts`]: option tokens, 0 decimals.
//!
//! Rounding always favors the protocol: collateral is rounded up, payouts are rounded down,
//! and the rounding dust belongs to the writer (see [`SeriesTerms`]). The vault of a series
//! therefore empties exactly once every long has redeemed and every writer has claimed
//! ([`SeriesLedger`]).
//!
//! Calls are capped (economically a call spread `K / cap`) so that they can be fully
//! collateralized; puts are bounded by their strike. See `docs/PLAN.md`, section 4.1.

#![no_std]

mod error;
mod ledger;
mod payoff;
mod series;
mod units;

pub use error::CoreError;
pub use ledger::{LedgerParts, SeriesLedger};
pub use payoff::MILLI_OUNCES_PER_OUNCE;
pub use series::{OptionKind, SeriesKey, SeriesState, SeriesTerms, Underlying};
pub use units::{Contracts, OuncesPerContract, StrikeUsd, UnixTimestamp, UsdcAtoms};
