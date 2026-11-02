//! Batched keeper passes (`Keeper::pass`, `docs/PLAN-improvements.md` P1) in LiteSVM with a
//! synthetic tape: a batched pass leaves every series exactly as per-series ticks do, with
//! fewer posts; a member another keeper filled is dropped before sending; a member filled
//! between revalidation and send fails its chunk, which is resent without it; an uncertain
//! post blocks its series' settle for the pass and is never posted twice; a chunk that keeps
//! failing never lets its series settle a subset. Run `make program` first.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::integer_division,
    clippy::panic_in_result_fn,
    clippy::disallowed_macros,
    missing_docs
)]

mod common;

use common::*;
use metalbook_core::SeriesState;
use metalbook_keeper::{BatchError, Chain, Keeper, KeeperError, PrintMsg, SeriesView, TapeSource};
use solana_address::Address;
use solana_instruction::Instruction;

/// LiteSVM as the keeper's chain, posting each print once (one `PriceUpdateV2` account) and
/// sampling it into a whole chunk in one transaction, so a chunk is atomic like a real post's
/// last transaction.
struct Batched {
    env: Env,
    capacity: usize,
    /// Transactions that carried samples (one per post).
    posts: usize,
    /// Settle transactions.
    settles: usize,
}

impl Batched {
    fn new(env: Env, capacity: usize) -> Self {
        Self {
            env,
            capacity,
            posts: 0,
            settles: 0,
        }
    }

    fn price_update(&mut self, feed: &[u8; 32], m: &PrintMsg) -> Address {
        self.env.price_update(PriceUpdateArgs {
            feed_id: *feed,
            price: m.price,
            conf: m.conf,
            exponent: m.expo,
            publish: m.publish_time,
            prev: m.prev_publish_time,
            ..PriceUpdateArgs::xau(1, 1, 0)
        })
    }

    fn send(&mut self, ixs: &[Instruction]) -> Result<(), KeeperError> {
        let payer = self.env.admin.insecure_clone();
        self.env
            .send(ixs, &[&payer])
            .map(|_| ())
            .map_err(|(e, _)| KeeperError::Chain(e))
    }
}

impl Chain for Batched {
    type Address = Address;

    fn now(&self) -> Result<i64, KeeperError> {
        let clock: solana_clock::Clock = self.env.svm.get_sysvar();
        Ok(clock.unix_timestamp)
    }

    fn account_data(&self, account: &Address) -> Result<Option<Vec<u8>>, KeeperError> {
        Ok(self.env.svm.get_account(account).map(|a| a.data))
    }

    fn post(&mut self, series: &Address, feed: &[u8; 32], m: &PrintMsg) -> Result<(), KeeperError> {
        self.post_batch(std::slice::from_ref(series), feed, m)
            .map_err(|e| match e {
                BatchError::Uncertain(e) | BatchError::Failed(e) => e,
            })
    }

    fn settle(&mut self, series: &Address) -> Result<(), KeeperError> {
        self.settle_batch(std::slice::from_ref(series))
    }

    fn batch_capacity(&self, _: &[u8; 32], _: &PrintMsg) -> Result<usize, KeeperError> {
        Ok(self.capacity)
    }

    fn post_batch(
        &mut self,
        series: &[Address],
        feed: &[u8; 32],
        m: &PrintMsg,
    ) -> Result<(), BatchError> {
        assert!(series.len() <= self.capacity, "chunk above capacity");
        let pu = self.price_update(feed, m);
        let ixs: Vec<Instruction> = series.iter().map(|s| self.env.ix_sample(s, &pu)).collect();
        self.posts += 1;
        self.send(&ixs).map_err(BatchError::Failed)
    }

    fn settle_batch(&mut self, series: &[Address]) -> Result<(), KeeperError> {
        let ixs: Vec<Instruction> = series.iter().map(|s| self.env.ix_settle(s)).collect();
        self.settles += 1;
        self.send(&ixs)
    }
}

/// The synthetic XAU tape of `tests/keeper.rs`: a print every 1 to 3 s from 40 minutes before
/// expiry to 15 minutes after, a same-second repeat every 7th step.
fn tape() -> Vec<PrintMsg> {
    tape_with_halt(None)
}

/// [`tape`] with no prints in `[from, to)`.
fn tape_with_halt(halt: Option<(i64, i64)>) -> Vec<PrintMsg> {
    let mut out = Vec::new();
    let (mut t, mut last_ok, mut px, mut step) =
        (EXPIRY - 2_400, EXPIRY - 2_401, 4_000_000i64, 0u64);
    while t <= EXPIRY + 900 {
        step += 1;
        if let Some((a, b)) = halt {
            if t >= a && t < b {
                t = b;
                continue;
            }
        }
        px += ((step * 7_919) % 41) as i64 - 20;
        out.push(PrintMsg {
            price: px,
            conf: 100,
            expo: -3,
            publish_time: t,
            prev_publish_time: last_ok,
        });
        if step % 7 == 0 {
            out.push(PrintMsg {
                price: px + 500,
                conf: 100,
                expo: -3,
                publish_time: t,
                prev_publish_time: t,
            });
        }
        last_ok = t;
        t += 1 + (step % 3) as i64;
    }
    out
}

/// Eight XAU series of one expiry (four puts, four capped calls), none written.
fn listed() -> (Env, Vec<Address>) {
    let mut env = Env::new();
    let mut series = Vec::new();
    for k in [3_900, 3_950, 4_000, 4_050] {
        series.push(env.create_series(SeriesArgs::xau_put(k)));
        series.push(env.create_series(SeriesArgs::xau_call(k)));
    }
    (env, series)
}

/// The times the keeper runs at: mid window, end of window, after `D`.
const TIMES: [i64; 4] = [EXPIRY - 900, EXPIRY + 30, EXPIRY + 400, EXPIRY + 700];

fn view(env: &Env, s: &Address) -> SeriesView {
    SeriesView::decode(&env.svm.get_account(s).unwrap().data).unwrap()
}

fn settlement_price(env: &Env, s: &Address) -> u64 {
    let stored: metalbook_core::accounts::Series =
        metalbook_core::accounts::read(&env.svm.get_account(s).unwrap().data).unwrap();
    stored.settlement_price
}

fn series_bytes(env: &Env, series: &[Address]) -> Vec<Vec<u8>> {
    series
        .iter()
        .map(|s| env.svm.get_account(s).unwrap().data)
        .collect()
}

#[test]
fn test_a_batched_pass_matches_per_series_ticks_with_fewer_posts() {
    let src = TapeSource::new(FEED_XAU, tape());
    let (env_a, series) = listed();
    let (env_b, _) = listed();
    let mut ticks = Batched::new(env_a, 1);
    let mut batched = Batched::new(env_b, 3);
    for t in TIMES {
        ticks.env.set_time(t);
        batched.env.set_time(t);
        for s in &series {
            Keeper.tick(&mut ticks, &src, s).unwrap();
        }
        let report = Keeper.pass(&mut batched, &src, &series).unwrap();
        assert!(report.series.iter().all(|(_, r)| r.is_ok()), "{report:?}");
        // Same candidates, same timestamps, same settlement: byte for byte.
        assert_eq!(
            series_bytes(&ticks.env, &series),
            series_bytes(&batched.env, &series)
        );
    }
    for s in &series {
        assert_eq!(view(&batched.env, s).state, SeriesState::Settled);
    }
    // Every print is posted once per chunk of 3 instead of once per series.
    assert_eq!(ticks.posts % 8, 0);
    let prints = ticks.posts / 8;
    assert_eq!(batched.posts, prints * 3, "8 series in chunks of 3");
    assert!(batched.settles < ticks.settles);
}

#[test]
fn test_a_member_another_keeper_filled_is_dropped_before_sending() {
    let src = TapeSource::new(FEED_XAU, tape());
    let (env, series) = listed();
    let mut chain = Batched::new(env, 8);
    chain.env.set_time(EXPIRY - 900);
    // Another keeper posted slot 0 to the first series.
    let slot0 = tape()
        .into_iter()
        .find(|m| {
            let b = EXPIRY - WINDOW_S;
            m.prev_publish_time < b && b <= m.publish_time && m.publish_time <= b + TOLERANCE_S
        })
        .unwrap();
    chain
        .env
        .post(
            &series[0],
            slot0.price,
            slot0.publish_time,
            slot0.prev_publish_time,
        )
        .unwrap();
    let report = Keeper.pass(&mut chain, &src, &series).unwrap();
    assert!(report.series.iter().all(|(_, r)| r.is_ok()), "{report:?}");
    // One post per print: nothing failed and nothing was resent.
    assert_eq!(chain.posts as u32, report.groups);
    let (_, first) = &report.series[0];
    let (_, second) = &report.series[1];
    assert_eq!(
        first.as_ref().unwrap().slots + 1,
        second.as_ref().unwrap().slots
    );
}

/// Fills `victim`'s candidate for the first chunk it sees between revalidation and send, as a
/// racing keeper would, so that chunk fails as a whole.
struct Racing {
    inner: Batched,
    victim: Address,
    raced: bool,
}

impl Chain for Racing {
    type Address = Address;
    fn now(&self) -> Result<i64, KeeperError> {
        self.inner.now()
    }
    fn account_data(&self, a: &Address) -> Result<Option<Vec<u8>>, KeeperError> {
        self.inner.account_data(a)
    }
    fn post(&mut self, s: &Address, f: &[u8; 32], m: &PrintMsg) -> Result<(), KeeperError> {
        self.inner.post(s, f, m)
    }
    fn settle(&mut self, s: &Address) -> Result<(), KeeperError> {
        self.inner.settle(s)
    }
    fn batch_capacity(&self, f: &[u8; 32], m: &PrintMsg) -> Result<usize, KeeperError> {
        self.inner.batch_capacity(f, m)
    }
    fn post_batch(&mut self, s: &[Address], f: &[u8; 32], m: &PrintMsg) -> Result<(), BatchError> {
        if !self.raced && s.len() > 1 && s.contains(&self.victim) {
            self.raced = true;
            self.inner.post(&self.victim, f, m).unwrap();
        }
        self.inner.post_batch(s, f, m)
    }
    fn settle_batch(&mut self, s: &[Address]) -> Result<(), KeeperError> {
        self.inner.settle_batch(s)
    }
}

#[test]
fn test_a_member_filled_between_revalidation_and_send_is_resent_without() {
    let src = TapeSource::new(FEED_XAU, tape());
    let (env, series) = listed();
    let mut chain = Racing {
        inner: Batched::new(env, 8),
        victim: series[3],
        raced: false,
    };
    for t in TIMES {
        chain.inner.env.set_time(t);
        let report = Keeper.pass(&mut chain, &src, &series).unwrap();
        assert!(report.series.iter().all(|(_, r)| r.is_ok()), "{report:?}");
    }
    assert!(chain.raced);
    // Everyone, the victim included, has the full window and settled on it.
    let reference = settlement_price(&chain.inner.env, &series[0]);
    for s in &series {
        let v = view(&chain.inner.env, s);
        assert_eq!(v.state, SeriesState::Settled);
        assert_eq!(v.candidates.filled(), 30);
        assert_eq!(settlement_price(&chain.inner.env, s), reference);
    }
}

/// Reports its first batched post as uncertain: the post lands (`lands`) or not, but the keeper
/// is told its outcome is unknown. Like `RpcChain`, it then refuses every post until the
/// uncertain one has `expired` (the payer-wide gate).
struct Uncertain {
    inner: Batched,
    lands: bool,
    fired: bool,
    expired: bool,
}

impl Chain for Uncertain {
    type Address = Address;
    fn now(&self) -> Result<i64, KeeperError> {
        self.inner.now()
    }
    fn account_data(&self, a: &Address) -> Result<Option<Vec<u8>>, KeeperError> {
        self.inner.account_data(a)
    }
    fn post(&mut self, s: &Address, f: &[u8; 32], m: &PrintMsg) -> Result<(), KeeperError> {
        self.inner.post(s, f, m)
    }
    fn settle(&mut self, s: &Address) -> Result<(), KeeperError> {
        self.inner.settle(s)
    }
    fn batch_capacity(&self, f: &[u8; 32], m: &PrintMsg) -> Result<usize, KeeperError> {
        self.inner.batch_capacity(f, m)
    }
    fn post_batch(&mut self, s: &[Address], f: &[u8; 32], m: &PrintMsg) -> Result<(), BatchError> {
        if !self.fired {
            self.fired = true;
            if self.lands {
                self.inner.post_batch(s, f, m).unwrap();
            }
            return Err(BatchError::Uncertain(KeeperError::Chain(
                "confirmation lost".into(),
            )));
        }
        if !self.expired {
            return Err(BatchError::Uncertain(KeeperError::Chain(
                "an earlier post may still land".into(),
            )));
        }
        self.inner.post_batch(s, f, m)
    }
    fn settle_batch(&mut self, s: &[Address]) -> Result<(), KeeperError> {
        self.inner.settle_batch(s)
    }
}

#[test]
fn test_an_uncertain_post_blocks_settle_and_is_never_posted_twice() {
    let mut prints_after = Vec::new();
    for lands in [true, false] {
        let src = TapeSource::new(FEED_XAU, tape());
        let (env, series) = listed();
        let mut chain = Uncertain {
            inner: Batched::new(env, 8),
            lands,
            fired: false,
            expired: false,
        };
        // Past expiry + D, so the window could settle at once if complete.
        chain.inner.env.set_time(EXPIRY + 700);
        let first = Keeper.pass(&mut chain, &src, &series).unwrap();
        // The uncertain chunk was the first print (slot 0), shared by every series: none
        // settles this pass, whatever else was posted.
        assert!(first.series.iter().all(|(_, r)| r.is_err()), "{first:?}");
        for s in &series {
            assert_ne!(view(&chain.inner.env, s).state, SeriesState::Settled);
        }
        // Before the uncertain post expires, a second pass posts nothing at all (the gate) and
        // settles nothing.
        let posts_before = chain.inner.posts;
        let gated = Keeper.pass(&mut chain, &src, &series).unwrap();
        assert!(gated.series.iter().all(|(_, r)| r.is_err()), "{gated:?}");
        assert_eq!(chain.inner.posts, posts_before);
        // Expired: the next pass posts only what is missing (one chunk per print), the slot-0
        // print again only if the uncertain post did not land.
        chain.expired = true;
        let third = Keeper.pass(&mut chain, &src, &series).unwrap();
        assert!(
            third
                .series
                .iter()
                .all(|(_, r)| r.as_ref().is_ok_and(|r| r.settled)),
            "{third:?}"
        );
        assert_eq!(third.posts, third.groups, "{third:?}");
        assert_eq!(chain.inner.posts - posts_before, third.posts as usize);
        prints_after.push(third.groups);
        for s in &series {
            assert_eq!(view(&chain.inner.env, s).candidates.filled(), 30);
        }
    }
    // The only difference between landing and not: the slot-0 print, posted once more.
    assert_eq!(prints_after[1], prints_after[0] + 1, "{prints_after:?}");
}

/// Every post fails; settles work.
struct Down {
    inner: Batched,
}

impl Chain for Down {
    type Address = Address;
    fn now(&self) -> Result<i64, KeeperError> {
        self.inner.now()
    }
    fn account_data(&self, a: &Address) -> Result<Option<Vec<u8>>, KeeperError> {
        self.inner.account_data(a)
    }
    fn post(&mut self, _: &Address, _: &[u8; 32], _: &PrintMsg) -> Result<(), KeeperError> {
        Err(KeeperError::Chain("receiver unavailable".into()))
    }
    fn settle(&mut self, s: &Address) -> Result<(), KeeperError> {
        self.inner.settle(s)
    }
    fn batch_capacity(&self, f: &[u8; 32], m: &PrintMsg) -> Result<usize, KeeperError> {
        self.inner.batch_capacity(f, m)
    }
    fn post_batch(&mut self, _: &[Address], _: &[u8; 32], _: &PrintMsg) -> Result<(), BatchError> {
        Err(BatchError::Failed(KeeperError::Chain(
            "receiver unavailable".into(),
        )))
    }
    fn settle_batch(&mut self, s: &[Address]) -> Result<(), KeeperError> {
        self.inner.settle_batch(s)
    }
}

#[test]
fn test_failing_chunks_never_let_a_pass_settle_a_subset() {
    // Codex #14 per series: an adversary posted the 20 lowest slots to every series; at
    // expiry + 10 + D the 20-slot mean could settle, but every post fails.
    let src = TapeSource::new(FEED_XAU, tape());
    let (env, series) = listed();
    let mut down = Down {
        inner: Batched::new(env, 8),
    };
    let mut slots: Vec<PrintMsg> = (0..30)
        .filter_map(|i| {
            let b = EXPIRY - WINDOW_S + i * SLOT_S;
            tape().into_iter().find(|m| {
                m.prev_publish_time > 0
                    && m.prev_publish_time < b
                    && b <= m.publish_time
                    && m.publish_time <= b + TOLERANCE_S
            })
        })
        .collect();
    slots.sort_by_key(|m| m.price);
    down.inner.env.set_time(EXPIRY + 10);
    for s in &series {
        for m in &slots[..20] {
            down.inner
                .env
                .post(s, m.price, m.publish_time, m.prev_publish_time)
                .unwrap();
        }
    }
    down.inner.env.set_time(EXPIRY + 10 + D);
    let report = Keeper.pass(&mut down, &src, &series).unwrap();
    assert!(report.series.iter().all(|(_, r)| r.is_err()), "{report:?}");
    for s in &series {
        assert_ne!(view(&down.inner.env, s).state, SeriesState::Settled);
    }
    // Once posting works, the next pass completes the window and settles on all 30.
    let mut up = down.inner;
    let report = Keeper.pass(&mut up, &src, &series).unwrap();
    assert!(
        report
            .series
            .iter()
            .all(|(_, r)| r.as_ref().is_ok_and(|r| r.settled)),
        "{report:?}"
    );
    for s in &series {
        assert_eq!(view(&up.env, s).candidates.filled(), 30);
    }
}

#[test]
fn test_a_print_that_is_two_candidates_of_a_series_is_one_sample() {
    // A halt right after the last slot's canonical print: it is also the latest print before
    // expiry, so it is both slot 29 and the fallback of every series. One sample
    // stores both; two in one chunk would fail it.
    let b = EXPIRY - SLOT_S; // slot 29's boundary
    let last_slot = tape()
        .into_iter()
        .find(|m| {
            m.prev_publish_time < b && b <= m.publish_time && m.publish_time <= b + TOLERANCE_S
        })
        .unwrap();
    let src = TapeSource::new(
        FEED_XAU,
        tape_with_halt(Some((last_slot.publish_time + 1, EXPIRY + 60))),
    );
    let (env, series) = listed();
    let mut chain = Batched::new(env, 8);
    chain.env.set_time(EXPIRY + 700);
    let report = Keeper.pass(&mut chain, &src, &series).unwrap();
    assert!(report.series.iter().all(|(_, r)| r.is_ok()), "{report:?}");
    // No chunk failed: one post per print.
    assert_eq!(chain.posts as u32, report.groups);
    for (s, r) in &report.series {
        let r = r.as_ref().unwrap();
        assert!(r.fallback, "{r:?}");
        let v = view(&chain.env, s);
        assert_eq!(v.candidates.filled(), 30);
        assert_eq!(v.candidates.parts().fallback_time, last_slot.publish_time);
    }
}

/// Where a source error (Hermes down or rate limited) strikes during a pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceFault {
    /// `batch_capacity` cannot fetch the update.
    Capacity,
    /// The first chunk's post.
    FirstPost,
    /// The first chunk is rejected, its resend hits the source error.
    Resend,
    /// Every batch is rejected; the one-by-one fallback hits the source error.
    Fallback,
}

/// [`Batched`] with a source error at one point of the pass.
struct SourceDown {
    inner: Batched,
    fault: SourceFault,
    batches: usize,
}

fn source_error() -> KeeperError {
    KeeperError::Source("Hermes returned HTTP 429 (rate limited)".into())
}

impl Chain for SourceDown {
    type Address = Address;
    fn now(&self) -> Result<i64, KeeperError> {
        self.inner.now()
    }
    fn account_data(&self, a: &Address) -> Result<Option<Vec<u8>>, KeeperError> {
        self.inner.account_data(a)
    }
    fn post(&mut self, s: &Address, f: &[u8; 32], m: &PrintMsg) -> Result<(), KeeperError> {
        if self.fault == SourceFault::Fallback {
            return Err(source_error());
        }
        self.inner.post(s, f, m)
    }
    fn settle(&mut self, s: &Address) -> Result<(), KeeperError> {
        self.inner.settle(s)
    }
    fn batch_capacity(&self, f: &[u8; 32], m: &PrintMsg) -> Result<usize, KeeperError> {
        if self.fault == SourceFault::Capacity {
            return Err(source_error());
        }
        self.inner.batch_capacity(f, m)
    }
    fn post_batch(&mut self, s: &[Address], f: &[u8; 32], m: &PrintMsg) -> Result<(), BatchError> {
        self.batches += 1;
        match (self.fault, self.batches) {
            (SourceFault::FirstPost, 1) | (SourceFault::Resend, 2) => {
                Err(BatchError::Failed(source_error()))
            }
            (SourceFault::Resend, 1) | (SourceFault::Fallback, _) => {
                Err(BatchError::Failed(KeeperError::Chain("rejected".into())))
            }
            _ => self.inner.post_batch(s, f, m),
        }
    }
    fn settle_batch(&mut self, s: &[Address]) -> Result<(), KeeperError> {
        self.inner.settle_batch(s)
    }
}

/// A [`TapeSource`] whose canonical slot queries fail: planning hits the source error.
struct PlanningDown(TapeSource);

impl metalbook_keeper::PriceSource for PlanningDown {
    fn canonical_at(
        &self,
        _: &[u8; 32],
        _: i64,
        _: i64,
        _: i64,
    ) -> Result<Option<PrintMsg>, KeeperError> {
        Err(source_error())
    }
    fn next_after(&self, f: &[u8; 32], t: i64, now: i64) -> Result<Option<PrintMsg>, KeeperError> {
        self.0.next_after(f, t, now)
    }
    fn latest_at_or_before(
        &self,
        f: &[u8; 32],
        t: i64,
        earliest: i64,
        now: i64,
    ) -> Result<Option<PrintMsg>, KeeperError> {
        self.0.latest_at_or_before(f, t, earliest, now)
    }
}

/// Codex's probe (#47, review of 8c4f1af): eight series with 29 of 30 slots and the complete
/// chain and fallback, one missing-slot group, capacity 3, past `D`. A source error at any
/// point must leave every series unsettled (it can settle on 29 only by skipping a print the
/// keeper knows exists), and a later working pass settles all of them on 30.
fn twenty_nine_of_thirty() -> (Env, Vec<Address>, TapeSource) {
    let all = tape();
    let src = TapeSource::new(FEED_XAU, all.clone());
    let (mut env, series) = listed();
    env.set_time(EXPIRY + 900);
    for s in &series {
        for i in 1..30 {
            let b = EXPIRY - WINDOW_S + i * SLOT_S;
            let m = all
                .iter()
                .find(|m| {
                    m.prev_publish_time < b
                        && b <= m.publish_time
                        && m.publish_time <= b + TOLERANCE_S
                })
                .unwrap();
            env.post(s, m.price, m.publish_time, m.prev_publish_time)
                .unwrap();
        }
        let mut last = EXPIRY;
        let k = view(&env, s).params.k_next_print();
        for _ in 0..k {
            let m = metalbook_keeper::PriceSource::next_after(&src, &FEED_XAU, last, EXPIRY + 900)
                .unwrap()
                .unwrap();
            env.post(s, m.price, m.publish_time, m.prev_publish_time)
                .unwrap();
            last = m.publish_time;
        }
        let m = metalbook_keeper::PriceSource::latest_at_or_before(
            &src,
            &FEED_XAU,
            EXPIRY,
            EXPIRY - 3_600,
            EXPIRY + 900,
        )
        .unwrap()
        .unwrap();
        let _ = env.post(s, m.price, m.publish_time, m.prev_publish_time);
        assert_eq!(view(&env, s).candidates.filled(), 29);
    }
    env.set_time(EXPIRY + 900 + D);
    (env, series, src)
}

#[test]
fn test_a_source_error_anywhere_in_a_pass_settles_no_subset() {
    for fault in [
        SourceFault::Capacity,
        SourceFault::FirstPost,
        SourceFault::Resend,
        SourceFault::Fallback,
    ] {
        let (env, series, src) = twenty_nine_of_thirty();
        let mut chain = SourceDown {
            inner: Batched::new(env, 3),
            fault,
            batches: 0,
        };
        let report = Keeper.pass(&mut chain, &src, &series).unwrap();
        assert_eq!(report.groups, 1, "{fault:?}: {report:?}");
        let settled = series
            .iter()
            .filter(|s| view(&chain.inner.env, s).state == SeriesState::Settled)
            .count();
        assert_eq!(settled, 0, "{fault:?}: settled on 29 of 30: {report:?}");
        // Hermes back: the next pass posts slot 0 and settles everyone on the full window.
        let mut up = chain.inner;
        let report = Keeper.pass(&mut up, &src, &series).unwrap();
        assert!(
            report
                .series
                .iter()
                .all(|(_, r)| r.as_ref().is_ok_and(|r| r.settled)),
            "{fault:?}: {report:?}"
        );
        for s in &series {
            assert_eq!(view(&up.env, s).candidates.filled(), 30, "{fault:?}");
        }
    }
    // Planning: the canonical slot queries fail.
    let (env, series, src) = twenty_nine_of_thirty();
    let mut chain = Batched::new(env, 3);
    let report = Keeper
        .pass(&mut chain, &PlanningDown(src), &series)
        .unwrap();
    assert!(report.series.iter().all(|(_, r)| r.is_err()), "{report:?}");
    assert!(series
        .iter()
        .all(|s| view(&chain.env, s).state != SeriesState::Settled));
}
