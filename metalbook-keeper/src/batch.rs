//! Bounded batching across series (`docs/PLAN-improvements.md`, P1).
//!
//! Every series of a feed and expiry needs the same canonical prints: on Oct 30 the 16 XAU
//! series settled at one price and the 7 XAG series at another. [`Keeper::pass`] therefore
//! plans every series first (exactly the candidates [`Keeper::tick`] would post), groups the
//! intents by feed and **exact signed message** ([`PrintMsg`] equality on every field), and
//! posts each print once for as many series as one transaction carries
//! ([`Chain::batch_capacity`]), one `sample_settlement` per series.
//!
//! Each group runs in publish-time order (window slots and the fallback before the chain links,
//! and links in chain order), and each member is **revalidated** just before its chunk is sent:
//! re-read, and kept only if `metalbook_core::SettlementState::add_print`, the program's own
//! rule, still accepts the print for it. A stale member would fail the whole transaction (a
//! filled slot is rejected), so this keeps one keeper's race with another from failing chunks.
//!
//! Failures are isolated by outcome, never by assuming the chunk failed atomically:
//! - [`BatchError::Uncertain`]: the post may still land (its first transaction's confirmation
//!   was lost). No retry and no fallback for that chunk in this pass; its series do not
//!   settle this pass and are planned again on the next, after the chain has resolved the
//!   in-flight account. A late landing is then found stored, so nothing is posted twice.
//! - [`BatchError::Failed`] with a source error (Hermes rate limit or outage): the pass stops
//!   posting; every series with an intent not yet posted does not settle this pass.
//! - [`BatchError::Failed`] otherwise (a confirmed rejection): re-read the chunk, drop the
//!   members that now hold their candidate, resend the rest once as one chunk; if that fails
//!   too, post them one by one with [`Keeper::tick`]'s rule (a failure is benign only if the
//!   candidate is on chain anyway).
//!
//! A failure blocks `settle` only for the series whose candidate is still missing (codex, #14,
//! per series). Settles run last, several per transaction ([`Chain::settle_batch`]).

use std::collections::HashMap;

use metalbook_core::{Decision, Print, SeriesState, UnixTimestamp};

use crate::keeper::{eligible, holds, post, read, Intent};
use crate::{Chain, Keeper, KeeperError, PriceSource, PrintMsg, SeriesView, TickReport};

/// Why a batched post failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BatchError {
    /// The post may still land: its outcome is unknown until its blockhash expires.
    #[error("post outcome unknown: {0}")]
    Uncertain(KeeperError),
    /// The post failed (rejected, or never sent).
    #[error("{0}")]
    Failed(KeeperError),
}

/// What [`Keeper::pass`] did, per series and in total.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PassReport<A> {
    /// Each series' outcome, in the order given.
    pub series: Vec<(A, Result<TickReport, KeeperError>)>,
    /// Posts made (one per chunk sent, including resends and single fallbacks).
    pub posts: u32,
    /// Distinct prints the pass needed.
    pub groups: u32,
}

/// One planned sample: the series and what the print is for. One print can be two
/// candidates of one series (the last slot's print can also be the latest fallback); it is
/// still one sample, since `add_print` stores every candidate a print qualifies for.
#[derive(Debug, Clone, Copy)]
struct Want<A> {
    series: A,
    intent: Intent,
    also: Option<Intent>,
}

/// One print and the series that need it.
struct Group<A> {
    feed: [u8; 32],
    print: PrintMsg,
    members: Vec<Want<A>>,
}

/// Per-series state while a pass runs.
#[derive(Debug, Clone, Default)]
struct Track {
    report: TickReport,
    /// Why this series must not settle this pass (an intent is missing or unknown). Its other
    /// candidates are still posted: revalidation drops any that depend on the missing one.
    blocked: Option<KeeperError>,
    /// Post nothing more for this series this pass (it could not be read or planned).
    halted: bool,
}

impl Keeper {
    /// One batched pass over `series`: plan, group by signed print, post in chunks, settle.
    /// Equivalent to [`Keeper::tick`] on each series in turn, with fewer transactions.
    ///
    /// # Errors
    ///
    /// Only reading the clock; every other failure is reported per series in the
    /// [`PassReport`].
    pub fn pass<C, S>(
        &self,
        chain: &mut C,
        source: &S,
        series: &[C::Address],
    ) -> Result<PassReport<C::Address>, KeeperError>
    where
        C: Chain,
        C::Address: Eq + std::hash::Hash,
        S: PriceSource,
    {
        let now = chain.now()?;
        let mut tracks: HashMap<C::Address, Track> = HashMap::new();
        let mut groups: Vec<Group<C::Address>> = Vec::new();

        // 1. Plan. A source error stops planning: the remaining series are not planned and
        // must not settle this pass.
        let mut source_down: Option<KeeperError> = None;
        for &s in series {
            let track = tracks.entry(s).or_default();
            if let Some(e) = &source_down {
                track.blocked = Some(e.clone());
                track.halted = true;
                continue;
            }
            let view = match read(chain, &s) {
                Ok(v) => v,
                Err(e) => {
                    track.blocked = Some(e);
                    track.halted = true;
                    continue;
                }
            };
            if matches!(view.state, SeriesState::Settled | SeriesState::Voided) {
                track.report.done = true;
                continue;
            }
            match plan(&view, source, now) {
                Ok((wants, skipped)) => {
                    track.report.skipped = skipped;
                    for (intent, print) in wants {
                        let want = Want {
                            series: s,
                            intent,
                            also: None,
                        };
                        match groups
                            .iter_mut()
                            .find(|g| g.feed == view.feed_id && g.print == print)
                        {
                            Some(g) => match g.members.iter_mut().find(|w| w.series == s) {
                                Some(w) => w.also = Some(intent),
                                None => g.members.push(want),
                            },
                            None => groups.push(Group {
                                feed: view.feed_id,
                                print,
                                members: vec![want],
                            }),
                        }
                    }
                }
                Err(e) => {
                    if matches!(e, KeeperError::Source(_)) {
                        source_down = Some(e.clone());
                    }
                    track.blocked = Some(e);
                    track.halted = true;
                }
            }
        }
        // Slots and the fallback precede expiry and links follow it; links in chain order.
        groups.sort_by_key(|g| g.print.publish_time);
        let group_count = u32::try_from(groups.len()).unwrap_or(u32::MAX);

        // 2. Post each group in chunks.
        let mut posts: u32 = 0;
        let mut stop: Option<KeeperError> = None;
        for group in &groups {
            if let Some(e) = &stop {
                block_all(&mut tracks, &group.members, e);
                continue;
            }
            // Revalidate: only members whose series still accepts this print, and that
            // nothing earlier in the pass blocked (a missing earlier link, for one).
            let mut live = Vec::new();
            for want in &group.members {
                let track = tracks.entry(want.series).or_default();
                if track.halted {
                    continue;
                }
                match read(chain, &want.series) {
                    Ok(view) if accepts(&view, &group.print, now) => live.push(*want),
                    Ok(view) if holds(&view, want.intent) => {}
                    Ok(_) => {
                        // Neither accepted nor held: the series moved on (settled) or an
                        // earlier candidate it needs is missing. Do not settle on a guess.
                        track.blocked = Some(KeeperError::Chain(format!(
                            "{:?} no longer accepted and not stored",
                            want.intent
                        )));
                    }
                    Err(e) => {
                        track.blocked = Some(e);
                        track.halted = true;
                    }
                }
            }
            let capacity = match chain.batch_capacity(&group.feed, &group.print) {
                Ok(n) => n.max(1),
                Err(e) => {
                    block_all(&mut tracks, &live, &e);
                    continue;
                }
            };
            for chunk in live.chunks(capacity) {
                let addresses: Vec<C::Address> = chunk.iter().map(|w| w.series).collect();
                posts = posts.saturating_add(1);
                match chain.post_batch(&addresses, &group.feed, &group.print) {
                    Ok(()) => credit(&mut tracks, chunk),
                    Err(BatchError::Uncertain(e)) => block_all(&mut tracks, chunk, &e),
                    Err(BatchError::Failed(e)) if matches!(e, KeeperError::Source(_)) => {
                        block_all(&mut tracks, chunk, &e);
                        stop = Some(e);
                        break;
                    }
                    Err(BatchError::Failed(_)) => {
                        posts = posts.saturating_add(recover(
                            chain,
                            &mut tracks,
                            chunk,
                            &group.feed,
                            &group.print,
                            now,
                        ));
                    }
                }
            }
        }

        // 3. Settle every series that may, several per transaction.
        let now = chain.now()?;
        let mut due = Vec::new();
        for &s in series {
            let track = tracks.entry(s).or_default();
            if track.blocked.is_some() || track.report.done {
                continue;
            }
            match read(chain, &s) {
                Ok(view) => {
                    match view
                        .candidates
                        .decide(&view.params, UnixTimestamp::new(view.expiry), now)
                    {
                        Ok(Decision::Wait { .. }) => {}
                        Ok(_) if now >= view.expiry => due.push(s),
                        Ok(_) => {}
                        Err(e) => track.blocked = Some(e.into()),
                    }
                }
                Err(e) => track.blocked = Some(e),
            }
        }
        if !due.is_empty() {
            if chain.settle_batch(&due).is_ok() {
                for s in &due {
                    tracks.entry(*s).or_default().report.settled = true;
                }
            } else {
                // One by one, re-reading first: part of the batch may have landed.
                for s in &due {
                    let track = tracks.entry(*s).or_default();
                    match read(chain, s) {
                        Ok(v) if matches!(v.state, SeriesState::Settled | SeriesState::Voided) => {
                            track.report.settled = true;
                        }
                        Ok(_) => match chain.settle(s) {
                            Ok(()) => track.report.settled = true,
                            Err(e) => track.blocked = Some(e),
                        },
                        Err(e) => track.blocked = Some(e),
                    }
                }
            }
        }

        let outcomes = series
            .iter()
            .map(|s| {
                let track = tracks.remove(s).unwrap_or_default();
                let outcome = match track.blocked {
                    Some(e) => Err(e),
                    None => Ok(track.report),
                };
                (*s, outcome)
            })
            .collect();
        Ok(PassReport {
            series: outcomes,
            posts,
            groups: group_count,
        })
    }
}

/// The candidates `view` lacks whose canonical print is already published, as [`Keeper::tick`]
/// computes them, and the number of canonical prints skipped as ineligible.
fn plan<S: PriceSource>(
    view: &SeriesView,
    source: &S,
    now: i64,
) -> Result<(Vec<(Intent, PrintMsg)>, u32), KeeperError> {
    let p = view.params;
    let feed = view.feed_id;
    let parts = view.candidates.parts();
    let mut wants = Vec::new();
    let mut skipped: u32 = 0;
    let window_start = view
        .expiry
        .checked_sub(i64::from(p.window_s()))
        .ok_or(KeeperError::Config("window overflow"))?;

    // Window slots.
    for i in 0..p.slots() {
        if parts.filled_slots.checked_shr(i).unwrap_or(0) & 1 == 1 {
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
            if eligible(view, &m) {
                wants.push((Intent::Slot(i), m));
            } else {
                skipped = skipped.saturating_add(1);
            }
        }
    }

    // Next-print links after the stored ones, in chain order.
    let mut len = parts.next_len;
    let mut last = match usize::from(len).checked_sub(1) {
        None => view.expiry,
        Some(j) => *parts
            .next_times
            .get(j)
            .ok_or(KeeperError::Decode("chain"))?,
    };
    while len < p.k_next_print() {
        let Some(m) = source.next_after(&feed, last, now)? else {
            break;
        };
        if !eligible(view, &m) {
            // An ineligible link ends the chain: nothing can chain past it.
            skipped = skipped.saturating_add(1);
            break;
        }
        wants.push((Intent::Link(m.publish_time), m));
        last = m.publish_time;
        len = len.saturating_add(1);
    }

    // The latest eligible fallback, from expiry on, if later than the stored one.
    let earliest = view
        .expiry
        .checked_sub(i64::from(p.fallback_lookback_s()))
        .ok_or(KeeperError::Config("lookback overflow"))?;
    let mut next = if now >= view.expiry {
        source.latest_at_or_before(&feed, view.expiry, earliest, now)?
    } else {
        None
    };
    while let Some(m) = next.take() {
        if m.publish_time <= parts.fallback_time {
            break;
        }
        if eligible(view, &m) {
            wants.push((Intent::Fallback(m.publish_time), m));
            break;
        }
        skipped = skipped.saturating_add(1);
        let upto = m
            .publish_time
            .checked_sub(1)
            .ok_or(KeeperError::Config("time overflow"))?;
        next = source.latest_at_or_before(&feed, upto, earliest, now)?;
    }
    Ok((wants, skipped))
}

/// Whether the series would store `m` now: open, eligible, and accepted by
/// `SettlementState::add_print` (the program's rule) on a copy of its candidates.
fn accepts(view: &SeriesView, m: &PrintMsg, now: i64) -> bool {
    if !matches!(view.state, SeriesState::Active | SeriesState::Expired) || !eligible(view, m) {
        return false;
    }
    let Ok(price) = u64::try_from(m.price) else {
        return false;
    };
    let Ok(print) = Print::try_new(price, m.publish_time, m.prev_publish_time) else {
        return false;
    };
    let mut copy = view.candidates;
    copy.add_print(&view.params, UnixTimestamp::new(view.expiry), print, now)
        .is_ok()
}

/// Counts each member's posted candidate.
fn credit<A: Copy + Eq + std::hash::Hash>(tracks: &mut HashMap<A, Track>, chunk: &[Want<A>]) {
    for want in chunk {
        let r = &mut tracks.entry(want.series).or_default().report;
        for intent in std::iter::once(want.intent).chain(want.also) {
            match intent {
                Intent::Slot(_) => r.slots = r.slots.saturating_add(1),
                Intent::Link(_) => r.links = r.links.saturating_add(1),
                Intent::Fallback(_) => r.fallback = true,
            }
        }
    }
}

/// Marks every member blocked by `e` (it does not settle this pass).
fn block_all<A: Copy + Eq + std::hash::Hash>(
    tracks: &mut HashMap<A, Track>,
    members: &[Want<A>],
    e: &KeeperError,
) {
    for want in members {
        let track = tracks.entry(want.series).or_default();
        if track.blocked.is_none() {
            track.blocked = Some(e.clone());
        }
    }
}

/// After a confirmed rejection: drop the members that now hold their candidate, resend the
/// rest once, then post them one by one. Returns the posts it made.
fn recover<C>(
    chain: &mut C,
    tracks: &mut HashMap<C::Address, Track>,
    chunk: &[Want<C::Address>],
    feed: &[u8; 32],
    print: &PrintMsg,
    now: i64,
) -> u32
where
    C: Chain,
    C::Address: Eq + std::hash::Hash,
{
    let mut posts: u32 = 0;
    let mut rest = Vec::new();
    for want in chunk {
        match read(chain, &want.series) {
            // Another keeper (or a landing whose confirmation failed) stored it.
            Ok(view) if holds(&view, want.intent) => {}
            Ok(view) if accepts(&view, print, now) => rest.push(*want),
            Ok(_) => {
                tracks.entry(want.series).or_default().blocked = Some(KeeperError::Chain(format!(
                    "{:?} rejected and not stored",
                    want.intent
                )));
            }
            Err(e) => tracks.entry(want.series).or_default().blocked = Some(e),
        }
    }
    if rest.is_empty() {
        return posts;
    }
    let addresses: Vec<C::Address> = rest.iter().map(|w| w.series).collect();
    posts = posts.saturating_add(1);
    match chain.post_batch(&addresses, feed, print) {
        Ok(()) => credit(tracks, &rest),
        Err(BatchError::Uncertain(e)) => block_all(tracks, &rest, &e),
        Err(BatchError::Failed(_)) => {
            for want in &rest {
                posts = posts.saturating_add(1);
                match post(chain, &want.series, feed, print, want.intent) {
                    Ok(true) => credit(tracks, std::slice::from_ref(want)),
                    Ok(false) => {}
                    Err(e) => tracks.entry(want.series).or_default().blocked = Some(e),
                }
            }
        }
    }
    posts
}
