//! Off-chain pricing adapter for metalbook series (`docs/PLAN.md` 3.2): payoff-aware marks
//! and two-sided maker quotes in USDC atoms, plus the quotebook quote terms built from them.
//!
//! The vanilla pricer is a dependency behind [`VanillaPricer`]: optfix will be one
//! implementation; the tests use OptionStratLib as an independent reference. This crate only
//! composes vanilla prices into the series' payoff and turns marks into quotes:
//!
//! - a put is one vanilla put at `K`;
//! - a capped call is the call spread `C(K) − C(cap)`: value and every greek, with the error
//!   bound the **sum** of both legs' bounds (errors may cancel; the bound never assumes so);
//! - per contract, everything is scaled by `multiplier / 1000` ounces;
//! - a mark never exceeds the contractual maximum payout, and the pricer is rejected if its
//!   value leaves `[0, max payout]` by more than its error bound;
//! - no quotes while the metal's market is closed or once the settlement window has started
//!   (`docs/PLAN.md` 4.4 and 4.7);
//! - quotes round toward the maker: bids down, asks up, to the tick.
//!
//! Off-chain code: amounts are `rust_decimal::Decimal` in USD, converted to USDC atoms at one
//! boundary with an explicit rounding direction. Nothing panics; every failure is a
//! [`MakerError`].

mod error;
mod listing;
mod mark;
mod pricer;
mod quote;

pub use error::MakerError;
pub use listing::{quote_terms, Listing};
pub use mark::{mark, Mark, MarketInputs, SECONDS_PER_YEAR};
pub use pricer::{VanillaInputs, VanillaKind, VanillaPricer, VanillaValue};
pub use quote::{quote_atoms, TwoSided};
