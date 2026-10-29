//! One keeper step for one series.

use metalbook_core::{accept_print, Decision, SeriesState, UnixTimestamp};

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
    /// Canonical prints skipped because the series' own rules reject them (confidence,
    /// exponent, price): the program would reject them too, so their candidate stays empty.
    pub skipped: u32,
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
    /// Source, chain and decoding errors. A failed post is benign only if the series, re-read
    /// right after, already holds the intended candidate (another keeper posted it first);
    /// otherwise the tick returns the error **without settling**.
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
                if !eligible(&view, &m) {
                    report.skipped = report.skipped.saturating_add(1);
                    continue;
                }
                if post(chain, series, &feed, &m, Intent::Slot(i))? {
                    report.slots = report.slots.saturating_add(1);
                }
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
            if !eligible(&view, &m) {
                // The canonical next link is ineligible: no later link can chain past it.
                report.skipped = report.skipped.saturating_add(1);
                break;
            }
            if post(chain, series, &feed, &m, Intent::Link(m.publish_time))? {
                report.links = report.links.saturating_add(1);
            }
            view = read(chain, series)?;
        }

        // 3. The latest fallback, if later than the stored one.
        let earliest = view
            .expiry
            .checked_sub(i64::from(p.fallback_lookback_s()))
            .ok_or(KeeperError::Config("lookback overflow"))?;
        // The latest *eligible* print: step back past ineligible ones.
        let stored = view.candidates.parts().fallback_time;
        let mut upto = view.expiry.min(now);
        while let Some(m) = source.latest_at_or_before(&feed, upto, earliest, now)? {
            if m.publish_time <= stored {
                break;
            }
            if eligible(&view, &m) {
                report.fallback = post(chain, series, &feed, &m, Intent::Fallback(m.publish_time))?;
                break;
            }
            report.skipped = report.skipped.saturating_add(1);
            upto = m
                .publish_time
                .checked_sub(1)
                .ok_or(KeeperError::Config("time overflow"))?;
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

/// Whether the series accepts `m` (`metalbook_core::accept_print`, the program's own rule).
fn eligible(view: &SeriesView, m: &PrintMsg) -> bool {
    accept_print(
        m.price,
        m.conf,
        m.expo,
        m.publish_time,
        m.prev_publish_time,
        view.expo,
        view.max_conf_bps,
    )
    .is_ok()
}

/// The candidate a post is meant to store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Intent {
    /// Window slot `i`.
    Slot(u32),
    /// The chain link with this publish time.
    Link(i64),
    /// A fallback at least this late.
    Fallback(i64),
}

/// Posts `m`. Returns `true` if this post stored it, `false` if the post failed but the
/// intended candidate is on chain anyway (another keeper won the race).
///
/// Any other failure (receiver, RPC, sample rejected with the candidate still missing) is an
/// error: the tick stops before `settle`, so a failed post can never let a subset settle that
/// the keeper knew how to complete (codex, #14).
fn post<C: Chain>(
    chain: &mut C,
    series: &C::Address,
    feed: &[u8; 32],
    m: &PrintMsg,
    intent: Intent,
) -> Result<bool, KeeperError> {
    let Err(error) = chain.post(series, feed, m) else {
        return Ok(true);
    };
    let parts = read(chain, series)?.candidates.parts();
    let stored = match intent {
        Intent::Slot(i) => parts.filled_slots.checked_shr(i).unwrap_or(0) & 1 == 1,
        Intent::Link(t) => parts
            .next_times
            .iter()
            .take(usize::from(parts.next_len))
            .any(|&x| x == t),
        Intent::Fallback(t) => parts.fallback_time >= t,
    };
    if stored {
        Ok(false)
    } else {
        Err(KeeperError::Chain(format!(
            "post of {intent:?} failed and the candidate is not on chain: {error}"
        )))
    }
}
