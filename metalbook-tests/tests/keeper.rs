//! End-to-end tests of `metalbook-keeper` in LiteSVM with synthetic Pyth tapes: a keeper alone
//! settles a series at the full-window price; a keeper competing with an adversary who posts a
//! favorable subset still settles at the full-window price; with a halt over the window the
//! keeper settles on the next-print chain; and ticks are idempotent. Run `make program` first.

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
use metalbook_keeper::{Chain, Keeper, KeeperError, PrintMsg, SeriesView, TapeSource};
use solana_address::Address;
use solana_signer::Signer;

/// The keeper's chain: LiteSVM through the test harness.
struct Svm<'a>(&'a mut Env);

impl Chain for Svm<'_> {
    type Address = Address;

    fn now(&self) -> Result<i64, KeeperError> {
        let clock: solana_clock::Clock = self.0.svm.get_sysvar();
        Ok(clock.unix_timestamp)
    }

    fn account_data(&self, account: &Address) -> Result<Option<Vec<u8>>, KeeperError> {
        Ok(self.0.svm.get_account(account).map(|a| a.data))
    }

    fn post(&mut self, series: &Address, feed: &[u8; 32], m: &PrintMsg) -> Result<(), KeeperError> {
        let pu = self.0.price_update(PriceUpdateArgs {
            feed_id: *feed,
            price: m.price,
            conf: m.conf,
            exponent: m.expo,
            publish: m.publish_time,
            prev: m.prev_publish_time,
            ..PriceUpdateArgs::xau(1, 1, 0)
        });
        let ix = self.0.ix_sample(series, &pu);
        let payer = self.0.admin.insecure_clone();
        self.0
            .send(&[ix], &[&payer])
            .map(|_| ())
            .map_err(|(e, _)| KeeperError::Chain(e))
    }

    fn settle(&mut self, series: &Address) -> Result<(), KeeperError> {
        self.0
            .settle(series)
            .map(|_| ())
            .map_err(|(e, _)| KeeperError::Chain(e))
    }
}

/// A synthetic XAU/USD tape (exponent -3) from 40 minutes before expiry to 15 minutes after:
/// a print every 1 to 3 seconds with a repeat in the same second every 7th step, prices on a
/// deterministic walk around 4000 USD, and an optional halt `[from, to)` with no prints.
fn tape(halt: Option<(i64, i64)>) -> Vec<PrintMsg> {
    let mut out = Vec::new();
    let (mut t, mut last_ok, mut px, mut step) =
        (EXPIRY - 2_400, EXPIRY - 2_401, 4_000_000i64, 0u64);
    while t <= EXPIRY + 900 {
        step += 1;
        if halt.is_some_and(|(a, b)| t >= a && t < b) {
            t = halt.unwrap().1;
            continue;
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

/// The canonical print of each window slot, computed here from the definition.
fn canonical_slots(tape: &[PrintMsg]) -> Vec<Option<i64>> {
    (0..30)
        .map(|i| {
            let b = EXPIRY - WINDOW_S + i * SLOT_S;
            tape.iter()
                .find(|m| {
                    m.prev_publish_time > 0
                        && m.prev_publish_time < b
                        && b <= m.publish_time
                        && m.publish_time <= b + TOLERANCE_S
                })
                .map(|m| m.price)
        })
        .collect()
}

/// Mean in atoms, rounded to nearest, of XAU mantissas (exponent -3).
fn mean_atoms(prices: &[i64]) -> u64 {
    let sum: i128 = prices.iter().map(|&p| i128::from(p)).sum();
    let (num, den) = (sum * 1_000, prices.len() as i128);
    (num / den + i128::from(2 * (num % den) >= den)) as u64
}

fn listed() -> (Env, SeriesArgs) {
    let mut env = Env::new();
    let args = SeriesArgs::xau_put(4_000);
    env.create_series(args);
    let w = env.writer(100_000);
    env.long_account(&w.pubkey(), args);
    env.send(&[env.ix_mint(&w.pubkey(), args, 1)], &[&w])
        .unwrap();
    (env, args)
}

#[test]
fn test_keeper_alone_settles_at_the_full_window_price() {
    let (mut env, args) = listed();
    let series = args.series();
    let t = tape(None);
    let slots = canonical_slots(&t);
    assert!(
        slots.iter().all(Option::is_some),
        "the synthetic tape covers every slot"
    );
    let source = TapeSource::new(FEED_XAU, t);

    // Before expiry the keeper samples what exists and does not settle.
    env.set_time(EXPIRY - 600);
    let r = Keeper.tick(&mut Svm(&mut env), &source, &series).unwrap();
    assert!(r.slots >= 20 && !r.settled, "{r:?}");
    // After expiry it completes the window and settles at once.
    env.set_time(EXPIRY + 60);
    let r = Keeper.tick(&mut Svm(&mut env), &source, &series).unwrap();
    assert!(r.settled, "{r:?}");
    let v = read_settlement(&env, &series);
    let prices: Vec<i64> = slots.into_iter().flatten().collect();
    assert_eq!((v.method, v.samples, v.price), (0, 30, mean_atoms(&prices)));
    // Idempotent afterwards.
    let r = Keeper.tick(&mut Svm(&mut env), &source, &series).unwrap();
    assert!(r.done && r.slots == 0 && r.links == 0);
}

#[test]
fn test_keeper_beats_an_adversary_posting_a_favorable_subset() {
    let (mut env, args) = listed();
    let series = args.series();
    let t = tape(None);
    let slots: Vec<(usize, i64)> = canonical_slots(&t)
        .into_iter()
        .map(Option::unwrap)
        .enumerate()
        .collect();
    let full: Vec<i64> = slots.iter().map(|&(_, p)| p).collect();

    // The adversary (a put holder wants a low price) posts the 20 lowest canonical slots.
    let mut low = slots.clone();
    low.sort_by_key(|&(_, p)| p);
    env.set_time(EXPIRY + 10);
    for &(i, _) in low.iter().take(20) {
        let b = EXPIRY - WINDOW_S + i as i64 * SLOT_S;
        let m = t
            .iter()
            .find(|m| {
                m.prev_publish_time < b && b <= m.publish_time && m.publish_time <= b + TOLERANCE_S
            })
            .unwrap();
        env.post(&series, m.price, m.publish_time, m.prev_publish_time)
            .unwrap();
    }
    let subset: Vec<i64> = low.iter().take(20).map(|&(_, p)| p).collect();
    assert!(mean_atoms(&subset) < mean_atoms(&full));

    // The keeper ticks once within D and posts the rest: the window is complete and settles.
    env.set_time(EXPIRY + 10 + D - 60);
    let r = Keeper
        .tick(&mut Svm(&mut env), &TapeSource::new(FEED_XAU, t), &series)
        .unwrap();
    assert!(r.slots == 10 && r.settled, "{r:?}");
    let v = read_settlement(&env, &series);
    assert_eq!((v.method, v.samples, v.price), (0, 30, mean_atoms(&full)));
}

#[test]
fn test_keeper_settles_on_the_chain_when_a_halt_empties_the_window() {
    let (mut env, args) = listed();
    let series = args.series();
    // No prints from 25 minutes before expiry to expiry: at most 5 slots exist.
    let t = tape(Some((EXPIRY - 1_500, EXPIRY)));
    let source = TapeSource::new(FEED_XAU, t.clone());
    env.set_time(EXPIRY + 120);
    let r = Keeper.tick(&mut Svm(&mut env), &source, &series).unwrap();
    assert!(r.slots < 20 && r.links == 5 && !r.settled, "{r:?}");
    // A complete chain settles from expiry + D.
    env.set_time(EXPIRY + D);
    let r = Keeper.tick(&mut Svm(&mut env), &source, &series).unwrap();
    assert!(r.settled, "{r:?}");
    let v = read_settlement(&env, &series);
    // The chain: the first five strictly advancing canonical prints after expiry.
    let mut chain = Vec::new();
    let mut last = EXPIRY;
    while chain.len() < 5 {
        let m = t
            .iter()
            .find(|m| m.prev_publish_time <= last && last < m.publish_time)
            .unwrap();
        chain.push(m.price);
        last = m.publish_time;
    }
    assert_eq!((v.method, v.samples, v.price), (1, 5, mean_atoms(&chain)));
}

#[test]
fn test_keeper_decodes_only_genuine_series() {
    let (env, args) = listed();
    let data = env.account(&args.series()).data;
    let view = SeriesView::decode(&data).unwrap();
    assert_eq!((view.expiry, view.feed_id), (EXPIRY, FEED_XAU));
    assert_eq!(view.params.min_samples(), MIN_SAMPLES);
    let bad = Err(KeeperError::Core(
        metalbook_core::CoreError::InvalidAccountData,
    ));
    assert_eq!(SeriesView::decode(&data[..500]), bad);
    let config = env.account(&config_address()).data;
    assert_eq!(SeriesView::decode(&config), bad);
    // Unaligned buffers decode too (RPC data has no alignment guarantee).
    let mut shifted = vec![0u8];
    shifted.extend_from_slice(&data);
    assert_eq!(SeriesView::decode(&shifted[1..]).unwrap(), view);
}

/// A chain whose posts fail (receiver or RPC down) but which can still read and settle;
/// with `lie`, each post lands and is then reported as failed (another keeper won the race).
struct Flaky<'a> {
    inner: Svm<'a>,
    lie: bool,
}

impl Chain for Flaky<'_> {
    type Address = Address;

    fn now(&self) -> Result<i64, KeeperError> {
        self.inner.now()
    }

    fn account_data(&self, account: &Address) -> Result<Option<Vec<u8>>, KeeperError> {
        self.inner.account_data(account)
    }

    fn post(&mut self, series: &Address, feed: &[u8; 32], m: &PrintMsg) -> Result<(), KeeperError> {
        if self.lie {
            self.inner.post(series, feed, m)?;
        }
        Err(KeeperError::Chain("receiver unavailable".into()))
    }

    fn settle(&mut self, series: &Address) -> Result<(), KeeperError> {
        self.inner.settle(series)
    }
}

/// Codex's reproduction on #14: the adversary's 20 low slots are on chain, the tick runs at
/// expiry + 10 + D, every post fails while settle would be allowed. The tick must fail without
/// settling the 20-slot mean; a later working tick settles the full window.
#[test]
fn test_failed_posts_never_let_the_tick_settle_a_subset() {
    let (mut env, args) = listed();
    let series = args.series();
    let t = tape(None);
    let slots: Vec<(usize, i64)> = canonical_slots(&t)
        .into_iter()
        .map(Option::unwrap)
        .enumerate()
        .collect();
    let full: Vec<i64> = slots.iter().map(|&(_, p)| p).collect();
    let mut low = slots.clone();
    low.sort_by_key(|&(_, p)| p);
    env.set_time(EXPIRY + 10);
    for &(i, _) in low.iter().take(20) {
        let b = EXPIRY - WINDOW_S + i as i64 * SLOT_S;
        let m = t
            .iter()
            .find(|m| {
                m.prev_publish_time < b && b <= m.publish_time && m.publish_time <= b + TOLERANCE_S
            })
            .unwrap();
        env.post(&series, m.price, m.publish_time, m.prev_publish_time)
            .unwrap();
    }
    env.set_time(EXPIRY + 10 + D);
    let source = TapeSource::new(FEED_XAU, t);
    let r = Keeper.tick(
        &mut Flaky {
            inner: Svm(&mut env),
            lie: false,
        },
        &source,
        &series,
    );
    assert!(matches!(r, Err(KeeperError::Chain(_))), "{r:?}");
    let v = read_settlement(&env, &series);
    assert_eq!(
        (v.filled_slots.count_ones(), v.method),
        (20, 255),
        "not settled"
    );
    // The receiver is back: the next tick completes the window and settles the full mean.
    let r = Keeper.tick(&mut Svm(&mut env), &source, &series).unwrap();
    assert!(r.settled && r.slots == 10, "{r:?}");
    let v = read_settlement(&env, &series);
    assert_eq!((v.method, v.samples, v.price), (0, 30, mean_atoms(&full)));
}

/// A post reported as failed whose candidate is on chain anyway is a lost race, not an error.
#[test]
fn test_lost_races_are_benign() {
    let (mut env, args) = listed();
    let series = args.series();
    let source = TapeSource::new(FEED_XAU, tape(None));
    env.set_time(EXPIRY + 60);
    let r = Keeper
        .tick(
            &mut Flaky {
                inner: Svm(&mut env),
                lie: true,
            },
            &source,
            &series,
        )
        .unwrap();
    assert!(r.settled && r.slots == 0, "{r:?}");
    assert_eq!(read_settlement(&env, &series).samples, 30);
}

/// Codex's reproduction on #14: slot 0's canonical print has conf 1_000_000 at a price of about
/// 4_000_000 (25% against the series' 1%), so the program would reject it. The keeper skips
/// it, still posts the 29 later slots, and settles the window once its delay has passed.
#[test]
fn test_ineligible_canonical_slot_is_skipped_not_blocking() {
    let (mut env, args) = listed();
    let series = args.series();
    let mut t = tape(None);
    let b0 = EXPIRY - WINDOW_S;
    let i0 = t
        .iter()
        .position(|m| {
            m.prev_publish_time < b0 && b0 <= m.publish_time && m.publish_time <= b0 + TOLERANCE_S
        })
        .unwrap();
    t[i0].conf = 1_000_000;
    let source = TapeSource::new(FEED_XAU, t.clone());
    env.set_time(EXPIRY + 60);
    let r = Keeper.tick(&mut Svm(&mut env), &source, &series).unwrap();
    assert!(r.slots == 29 && r.skipped >= 1 && !r.settled, "{r:?}");
    assert_eq!(read_settlement(&env, &series).filled_slots & 1, 0);
    env.set_time(EXPIRY + 60 + D);
    let r = Keeper.tick(&mut Svm(&mut env), &source, &series).unwrap();
    assert!(r.settled, "{r:?}");
    let valid: Vec<i64> = canonical_slots(&t)
        .into_iter()
        .skip(1)
        .map(Option::unwrap)
        .collect();
    let v = read_settlement(&env, &series);
    assert_eq!((v.method, v.samples, v.price), (0, 29, mean_atoms(&valid)));
}

/// An ineligible next-print link ends the chain there: no link past it is posted.
#[test]
fn test_ineligible_chain_link_ends_the_chain() {
    let (mut env, args) = listed();
    let series = args.series();
    let mut t = tape(Some((EXPIRY - 1_500, EXPIRY)));
    // The third link after expiry.
    let mut last = EXPIRY;
    let mut third = 0;
    for _ in 0..3 {
        third = t
            .iter()
            .position(|m| m.prev_publish_time <= last && last < m.publish_time)
            .unwrap();
        last = t[third].publish_time;
    }
    t[third].conf = 1_000_000;
    env.set_time(EXPIRY + 120);
    let r = Keeper
        .tick(&mut Svm(&mut env), &TapeSource::new(FEED_XAU, t), &series)
        .unwrap();
    assert!(r.links == 2 && r.skipped >= 1 && !r.settled, "{r:?}");
    assert_eq!(read_settlement(&env, &series).next_len, 2);
}

/// The fallback is the latest *eligible* print at or before expiry.
#[test]
fn test_fallback_steps_back_past_ineligible_prints() {
    let (mut env, args) = listed();
    let series = args.series();
    let mut t = tape(None);
    let latest = t
        .iter()
        .rposition(|m| m.prev_publish_time < m.publish_time && m.publish_time <= EXPIRY)
        .unwrap();
    t[latest].conf = 1_000_000;
    let previous = t[..latest]
        .iter()
        .rposition(|m| m.prev_publish_time < m.publish_time)
        .unwrap();
    env.set_time(EXPIRY + 60);
    let r = Keeper
        .tick(
            &mut Svm(&mut env),
            &TapeSource::new(FEED_XAU, t.clone()),
            &series,
        )
        .unwrap();
    assert!(r.fallback && r.skipped >= 1, "{r:?}");
    let d = env.account(&series).data;
    let stored = i64::from_le_bytes(d[416..424].try_into().unwrap());
    assert_eq!(stored, t[previous].publish_time);
}

// codex's starvation probes (#28): Hermes allows `limit` requests per pass, then answers 429.
// With nothing published (404 everywhere) past the hard deadline, a keeper that re-asked the
// same empty slots every pass never reached `settle` and the series never voided.

/// An HTTP double with a per-pass quota: `limit` answers, then 429.
struct Quota {
    count: std::cell::Cell<usize>,
    limit: usize,
    /// Answer `/latest` with a print (published at 100) instead of 404.
    latest_print: bool,
}

impl metalbook_keeper::HttpGet for &Quota {
    fn get(
        &self,
        url: &str,
        _headers: &[(String, String)],
    ) -> Result<metalbook_keeper::HttpResponse, KeeperError> {
        self.count.set(self.count.get() + 1);
        if self.count.get() > self.limit {
            return Ok(metalbook_keeper::HttpResponse {
                status: 429,
                body: String::new(),
            });
        }
        if self.latest_print && url.contains("/latest?") {
            let feed: String = FEED_XAU.iter().map(|x| format!("{x:02x}")).collect();
            return Ok(metalbook_keeper::HttpResponse {
                status: 200,
                body: format!(
                    r#"{{"binary":{{"encoding":"base64","data":["AAEC"]}},"parsed":[{{"id":"{feed}","price":{{"price":"1","conf":"0","expo":-3,"publish_time":100}},"metadata":{{"prev_publish_time":99}}}}]}}"#
                ),
            });
        }
        Ok(metalbook_keeper::HttpResponse {
            status: 404,
            body: String::new(),
        })
    }
}

/// Ticks until the series settles (voids) or `passes` run out; returns the pass it settled in.
fn settles_under_quota(latest_print: bool, passes: usize) -> Option<usize> {
    let (mut env, args) = listed();
    let series = args.series();
    env.set_time(EXPIRY + HARD_S + D + 1);
    let q = Quota {
        count: std::cell::Cell::new(0),
        limit: 6,
        latest_print,
    };
    let cfg = metalbook_keeper::HermesConfig::from_lookup(|n| {
        (n == metalbook_keeper::HERMES_API_KEY_VAR).then(|| "local-placeholder".into())
    })
    .unwrap();
    let source = metalbook_keeper::HermesSource::new(cfg, &q)
        .with_cooldown(std::time::Duration::from_millis(2));
    for pass in 0..passes {
        q.count.set(0);
        if Keeper
            .tick(&mut Svm(&mut env), &source, &series)
            .is_ok_and(|r| r.settled)
        {
            let v = SeriesView::decode(&env.svm.get_account(&series).unwrap().data).unwrap();
            assert_eq!(v.state, metalbook_core::SeriesState::Voided);
            return Some(pass);
        }
        // Past the cooldown.
        std::thread::sleep(std::time::Duration::from_millis(3));
    }
    None
}

#[test]
fn test_nothing_published_and_a_tight_quota_still_void_past_the_deadline() {
    // Remembered misses cost nothing while nothing newer exists, so each pass makes new
    // progress and the series voids within a bounded number of passes (it starved forever
    // at a25a028).
    let pass = settles_under_quota(false, 40);
    assert!(pass.is_some(), "starved");
}

#[test]
fn test_a_published_latest_also_lets_the_keeper_progress_under_the_quota() {
    // codex's positive control: `/latest` answers with an old print.
    assert!(settles_under_quota(true, 40).is_some());
}
