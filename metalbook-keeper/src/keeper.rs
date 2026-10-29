//! One keeper step for one series.

use metalbook_core::{Decision, SeriesState, UnixTimestamp};

use crate::{Chain, KeeperError, PriceSource, PrintMsg, SeriesView};

/// What one [`Keeper::tick`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TickReport {
    /// Window slots posted.
    pub slots: u32,
    /// Next-print links posted.
    pub links: u32,
    /// Whether a later fallback was posted.
    pub fallback: bool,
    /// Whether `settle` was called and succeeded.
    pub settled: bool,
    /// Whether the series was already settled or voided (nothing to do).
    pub done: bool,
}

/// The settlement keeper.
#[derive(Debug, Clone, Copy, Default)]
pub struct Keeper;

impl Keeper {
    /// Posts every missing canonical candidate that is already published, then settles if the
    /// cascade allows it. Idempotent: a second call with nothing new posts nothing.
    ///
    /// # Errors
    ///
    /// Source, chain and decoding errors. A rejected post (another keeper posted the same
    /// print first) is not an error: the series is re-read before each step.
    pub fn tick<C: Chain, S: PriceSource>(
        &self,
        chain: &mut C,
        source: &S,
        series: &C::Address,
    ) -> Result<TickReport, KeeperError> {
        let mut report = TickReport::default();
        let now = chain.now()?;
        let view = read(chain, series)?;
        if matches!(view.state, SeriesState::Settled | SeriesState::Voided) {
            report.done = true;
            return Ok(report);
        }
        let p = view.params;
        let feed = view.feed_id;
        let window_start = view
            .expiry
            .checked_sub(i64::from(p.window_s()))
            .ok_or(KeeperError::Config("window overflow"))?;

        // 1. Every empty window slot whose canonical print exists.
        let filled = view.candidates.parts().filled_slots;
        for i in 0..p.slots() {
            if filled.checked_shr(i).unwrap_or(0) & 1 == 1 {
                continue;
            }
            let offset = i64::from(i)
                .checked_mul(i64::from(p.slot_s()))
                .ok_or(KeeperError::Config("slot overflow"))?;
            let boundary = window_start
                .checked_add(offset)
                .ok_or(KeeperError::Config("slot overflow"))?;
            if let Some(m) =
                source.canonical_at(&feed, boundary, i64::from(p.slot_tolerance_s()), now)?
            {
                report.slots = report
                    .slots
                    .saturating_add(u32::from(post(chain, series, &feed, &m)));
            }
        }

        // 2. Every missing link of the next-print chain, in order.
        let mut view = read(chain, series)?;
        loop {
            let parts = view.candidates.parts();
            if parts.next_len >= p.k_next_print() {
                break;
            }
            let last = match usize::from(parts.next_len).checked_sub(1) {
                None => view.expiry,
                Some(j) => *parts
                    .next_times
                    .get(j)
                    .ok_or(KeeperError::Decode("chain"))?,
            };
            let Some(m) = source.next_after(&feed, last, now)? else {
                break;
            };
            if !post(chain, series, &feed, &m) {
                break;
            }
            report.links = report.links.saturating_add(1);
            view = read(chain, series)?;
        }

        // 3. The latest fallback, if later than the stored one.
        let earliest = view
            .expiry
            .checked_sub(i64::from(p.fallback_lookback_s()))
            .ok_or(KeeperError::Config("lookback overflow"))?;
        if let Some(m) = source.latest_at_or_before(&feed, view.expiry.min(now), earliest, now)? {
            if m.publish_time > view.candidates.parts().fallback_time {
                report.fallback = post(chain, series, &feed, &m);
            }
        }

        // 4. Settle as soon as the cascade allows it.
        let view = read(chain, series)?;
        let decision = view
            .candidates
            .decide(&p, UnixTimestamp::new(view.expiry), now)?;
        if !matches!(decision, Decision::Wait { .. }) && now >= view.expiry {
            chain.settle(series)?;
            report.settled = true;
        }
        Ok(report)
    }
}

fn read<C: Chain>(chain: &C, series: &C::Address) -> Result<SeriesView, KeeperError> {
    let data = chain
        .account_data(series)?
        .ok_or(KeeperError::Decode("series account not found"))?;
    SeriesView::decode(&data)
}

/// Posts `m`; a rejection (already stored by someone else, or not usable) is not an error.
fn post<C: Chain>(chain: &mut C, series: &C::Address, feed: &[u8; 32], m: &PrintMsg) -> bool {
    chain.post(series, feed, m).is_ok()
}
