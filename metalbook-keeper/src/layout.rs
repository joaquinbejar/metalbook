//! Decoding of the series account, through the shared layouts of `metalbook_core::accounts`
//! (the same definitions the program writes with).

use metalbook_core::accounts::{self, Series};
use metalbook_core::{OracleParams, SeriesState, SettlementState};

use crate::KeeperError;

/// Size of a series account.
pub const SERIES_LEN: usize = core::mem::size_of::<Series>();

/// The keeper's view of a series.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeriesView {
    /// Lifecycle state.
    pub state: SeriesState,
    /// Expiry, seconds.
    pub expiry: i64,
    /// Pyth feed id of the underlying.
    pub feed_id: [u8; 32],
    /// Oracle terms (snapshot).
    pub params: OracleParams,
    /// Stored settlement candidates.
    pub candidates: SettlementState,
}

impl SeriesView {
    /// Decodes and validates a series account (any alignment).
    ///
    /// # Errors
    ///
    /// `KeeperError::Core` with `InvalidAccountData` for a wrong size, tag or version, or the
    /// core error if the stored terms or candidates are rejected.
    pub fn decode(data: &[u8]) -> Result<Self, KeeperError> {
        let s: Series = accounts::read(data)?;
        let params = s.oracle_params()?;
        Ok(Self {
            state: SeriesState::try_from(s.state)?,
            expiry: s.expiry,
            feed_id: s.feed_id,
            params,
            candidates: s.settlement(&params)?,
        })
    }
}
