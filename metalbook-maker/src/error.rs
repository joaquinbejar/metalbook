//! Errors of the pricing adapter.

/// Why a mark or a quote could not be produced.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MakerError {
    /// The metal's market is closed: no quotes (`docs/PLAN.md` 4.4).
    #[error("market closed: not quoting")]
    MarketClosed,
    /// The settlement window has started (or the series expired): no quotes (4.7).
    #[error("settlement window started: not quoting")]
    SettlementWindow,
    /// An input is out of range (negative spot, volatility, spread, a zero tick, ...).
    #[error("invalid input: {0}")]
    InvalidInput(&'static str),
    /// The vanilla pricer failed.
    #[error("pricer failed: {0}")]
    Pricer(String),
    /// The pricer's value leaves `[0, max payout]` by more than its own error bound.
    #[error("pricer value inconsistent with the payoff bounds")]
    PricerInconsistent,
    /// A decimal operation overflowed, or an amount does not fit a `u64` of atoms.
    #[error("arithmetic overflow")]
    Overflow,
    /// A price is not a whole number of quote lots.
    #[error("price not on the quote lot grid")]
    OffLotGrid,
    /// The quotebook quote terms were rejected.
    #[error("quote rejected by quotebook-core: {0:?}")]
    Quote(quotebook_core::QuotebookError),
}
