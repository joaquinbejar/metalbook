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
    assert!(matches!(
        SeriesView::decode(&data[..500]),
        Err(KeeperError::Decode(_))
    ));
    let config = env.account(&config_address()).data;
    assert!(matches!(
        SeriesView::decode(&config),
        Err(KeeperError::Decode(_))
    ));
}
