//! Off-chain settlement keeper for metalbook (`docs/PLAN.md` 4.3, milestone 7).
//!
//! MetalBook's settlement is correct under one assumption: at least one honest keeper submits,
//! within the challenge delay `D` of any change, every available candidate of higher priority
//! than the one that would otherwise settle. [`Keeper::tick`] does exactly that for one series:
//!
//! 1. reads the series account and restores its oracle terms and stored candidates with
//!    `metalbook-core` (so the keeper and the program agree on every rule);
//! 2. posts the canonical print of every window slot that is still empty and already
//!    published, every missing link of the next-print chain, and the latest fallback if it is
//!    later than the stored one;
//! 3. calls `settle` as soon as the cascade allows it.
//!
//! Prices come from a [`PriceSource`] (Hermes in production, synthetic tapes in tests) and
//! transactions go through a [`Chain`] (an RPC client in production, LiteSVM in tests). Run
//! `tick` more often than `D` (for example every minute), and an adversary posting a subset
//! cannot settle on it: every improvement the keeper posts restarts `D`, and a complete window
//! settles at once.
//!
//! The series account is decoded with the program's own layouts (`metalbook_core::accounts`).
//!
//! The Hermes API key is read only from the environment ([`HermesConfig::from_env`]), never
//! from a file, and never printed.

mod chain;
mod error;
mod hermes;
mod keeper;
mod layout;
mod source;

pub use chain::Chain;
pub use error::KeeperError;
pub use hermes::{HermesConfig, DEFAULT_HERMES_URL, HERMES_API_KEY_VAR, HERMES_URL_VAR};
pub use keeper::{Keeper, TickReport};
pub use layout::{SeriesView, SERIES_LEN};
pub use source::{PriceSource, PrintMsg, TapeSource};
