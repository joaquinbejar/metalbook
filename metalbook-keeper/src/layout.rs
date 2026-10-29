//! Decoding of the series account (layout version 3, `metalbook-program/src/state.rs`), only the
//! fields the keeper needs. The offsets are restated here; the LiteSVM tests decode genuine
//! accounts, so a drift between the two fails them. (When the program's layouts move into a
//! shared crate, as quotebook's are moving into quotebook-core, this module goes away.)

use metalbook_core::{
    OracleParams, SeriesState, SettlementParts, SettlementState, MAX_NEXT_PRINTS,
};

use crate::KeeperError;

/// Size of a series account.
pub const SERIES_LEN: usize = 504;

/// Account tag of a series.
const TAG_SERIES: u8 = 3;

/// Layout version this keeper reads.
const VERSION: u8 = 3;

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
    /// Decodes and validates a series account.
    ///
    /// # Errors
    ///
    /// `KeeperError::Decode` for a wrong size, tag or version; `KeeperError::Core` if
    /// `metalbook-core` rejects the stored terms or candidates.
    pub fn decode(data: &[u8]) -> Result<Self, KeeperError> {
        if data.len() != SERIES_LEN {
            return Err(KeeperError::Decode("wrong size"));
        }
        let r = Bytes(data);
        if r.u8(0)? != TAG_SERIES || r.u8(1)? != VERSION {
            return Err(KeeperError::Decode("not a version-3 series"));
        }
        let params = OracleParams::try_new(
            r.u32(288)?,
            r.u32(292)?,
            r.u16(284)?,
            r.u16(286)?,
            r.u8(281)?,
            r.u32(296)?,
            r.u32(300)?,
            r.u32(304)?,
            i8::from_le_bytes([r.u8(280)?]),
        )?;
        let mut next_times = [0i64; MAX_NEXT_PRINTS];
        let mut next_prices = [0u64; MAX_NEXT_PRINTS];
        for (j, (t, p)) in next_times
            .iter_mut()
            .zip(next_prices.iter_mut())
            .enumerate()
        {
            let off = j.checked_mul(8).ok_or(KeeperError::Decode("offset"))?;
            *t = r.i64(
                336usize
                    .checked_add(off)
                    .ok_or(KeeperError::Decode("offset"))?,
            )?;
            *p = r.u64(
                376usize
                    .checked_add(off)
                    .ok_or(KeeperError::Decode("offset"))?,
            )?;
        }
        let parts = SettlementParts {
            sample_sum: u128::from_le_bytes(r.array::<16>(312)?),
            filled_slots: r.u32(328)?,
            next_times,
            next_prices,
            next_len: r.u8(332)?,
            fallback_time: r.i64(416)?,
            fallback_price: r.u64(424)?,
            window_changed: r.i64(432)?,
            next_print_changed: r.i64(440)?,
            fallback_changed: r.i64(448)?,
        };
        Ok(Self {
            state: SeriesState::try_from(r.u8(4)?)?,
            expiry: r.i64(32)?,
            feed_id: r.array::<32>(248)?,
            params,
            candidates: SettlementState::restore(&params, parts)?,
        })
    }
}

/// Bounds-checked little-endian reads at fixed offsets.
struct Bytes<'a>(&'a [u8]);

impl Bytes<'_> {
    fn array<const N: usize>(&self, at: usize) -> Result<[u8; N], KeeperError> {
        let end = at.checked_add(N).ok_or(KeeperError::Decode("offset"))?;
        let slice = self
            .0
            .get(at..end)
            .ok_or(KeeperError::Decode("truncated"))?;
        <[u8; N]>::try_from(slice).map_err(|_| KeeperError::Decode("truncated"))
    }

    fn u8(&self, at: usize) -> Result<u8, KeeperError> {
        self.array::<1>(at).map(|[b]| b)
    }

    fn u16(&self, at: usize) -> Result<u16, KeeperError> {
        self.array(at).map(u16::from_le_bytes)
    }

    fn u32(&self, at: usize) -> Result<u32, KeeperError> {
        self.array(at).map(u32::from_le_bytes)
    }

    fn u64(&self, at: usize) -> Result<u64, KeeperError> {
        self.array(at).map(u64::from_le_bytes)
    }

    fn i64(&self, at: usize) -> Result<i64, KeeperError> {
        self.array(at).map(i64::from_le_bytes)
    }
}
