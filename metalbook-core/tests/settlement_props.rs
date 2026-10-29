//! The canonical-uniqueness gate of `docs/PLAN.md` 4.2, on synthetic Pyth tapes (no Hermes
//! key yet for mainnet tapes).
//!
//! A tape is a stream of `PriceFeedMessage`s as Pythnet emits them: a successful aggregate
//! carries `prev_publish_time` = the previous successful `publish_time`; later aggregates in
//! the same second, and messages from slots where aggregation failed, repeat
//! `prev_publish_time == publish_time`; halts leave gaps of up to 25 minutes. The candidates
//! are computed here from their definitions by scanning the whole tape, independently of the
//! crate, then:
//! - posting the whole tape in any order (re-posting until nothing changes, as a keeper
//!   would for chain links that arrive before their predecessor) gives exactly those
//!   candidates;
//! - posting any subset in any order never stores anything else: slots are a subset of the
//!   reference with the same prices, the chain is a prefix of the reference chain, and the
//!   fallback is never later than the reference fallback.
//!
//! This proves that each stored value is canonical (no choice of price for a given slot, link
//! or fallback). It does not prove that the settlement price is independent of which prints
//! were posted: an adversary alone can post a subset, or an older fallback, and settle on it
//! after `D`. That case relies on the honest-keeper assumption of `docs/PLAN.md` 4.3, and on
//! improvements restarting `D` (tested in the crate's unit tests and in LiteSVM).

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

use metalbook_core::{OracleParams, Print, SettlementState, UnixTimestamp, MAX_NEXT_PRINTS};
use proptest::prelude::*;
use proptest::test_runner::{Config, RngSeed};

const EXPIRY: i64 = 1_800_000_000;

/// Cases per property (each a whole tape of up to 2_500 messages). `METALBOOK_PROPTEST_CASES`
/// overrides the default of 2_000.
fn config() -> Config {
    let cases = std::env::var("METALBOOK_PROPTEST_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2_000);
    Config {
        cases,
        rng_seed: RngSeed::Fixed(0x3E7A_1B00_C0DE_0003),
        failure_persistence: None,
        ..Config::default()
    }
}

fn params() -> OracleParams {
    // 30 slots of 60 s, tolerance 10 s, 20 samples, k = 5, D = 600 s, X = 2 h, hard 7 days.
    OracleParams::try_new(1_800, 60, 10, 20, 5, 600, 7_200, 604_800, -3).unwrap()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Msg {
    price: u64,
    publish: i64,
    prev: i64,
}

/// One tape step: time advance kind, whether aggregation failed, price.
type Step = (u8, bool, u64);

/// Builds a tape starting 40 minutes before expiry and running past it: mostly one print
/// every 1 to 3 seconds (about 16% repeats in the same second), with halts of 30 or 90
/// seconds (0.8% of steps) and of 15 minutes (0.4%). Half the tapes are calm: no halts.
fn tape((calm, steps): &(bool, Vec<Step>)) -> Vec<Msg> {
    let mut t = EXPIRY - 2_400;
    let mut last_ok = t - 1;
    let mut last_second_with_success = i64::MIN;
    let mut out = Vec::new();
    for &(kind, failed, price) in steps {
        t += match kind {
            0..=39 => 0,
            40..=252 => 1 + i64::from(kind % 3),
            // A calm tape has no halts.
            253..=255 if *calm => 1,
            253 | 254 => 30 + i64::from(kind - 253) * 60,
            255 => 900,
        };
        let msg = if failed {
            // A slot where aggregation failed repeats the last successful time.
            Msg {
                price,
                publish: last_ok,
                prev: last_ok,
            }
        } else if last_second_with_success == t {
            // A later aggregate in the same second.
            Msg {
                price,
                publish: t,
                prev: t,
            }
        } else {
            let m = Msg {
                price,
                publish: t,
                prev: last_ok,
            };
            last_ok = t;
            last_second_with_success = t;
            m
        };
        out.push(msg);
    }
    out
}

struct Reference {
    slots: Vec<Option<u64>>,
    chain: Vec<(i64, u64)>,
    fallback: Option<(i64, u64)>,
}

fn reference(p: &OracleParams, msgs: &[Msg]) -> Reference {
    let start = EXPIRY - i64::from(p.window_s());
    let tol = i64::from(p.slot_tolerance_s());
    let slots = (0..p.slots())
        .map(|i| {
            let b = start + i64::from(i) * i64::from(p.slot_s());
            let found: Vec<u64> = msgs
                .iter()
                .filter(|m| m.prev > 0 && m.prev < b && b <= m.publish && m.publish <= b + tol)
                .map(|m| m.price)
                .collect();
            // Uniqueness: at most one canonical print per boundary (same publish second with
            // the same prev is impossible on a tape).
            assert!(found.len() <= 1, "two canonical prints for slot {i}");
            found.first().copied()
        })
        .collect();
    let mut chain = Vec::new();
    let mut t = EXPIRY;
    while chain.len() < usize::from(p.k_next_print()) {
        let found: Vec<&Msg> = msgs
            .iter()
            .filter(|m| {
                if chain.is_empty() {
                    m.prev > 0 && m.prev <= t && t < m.publish
                } else {
                    m.prev == t && m.publish > t
                }
            })
            .collect();
        assert!(
            found.len() <= 1,
            "two candidates for chain link {}",
            chain.len()
        );
        let Some(m) = found.first() else { break };
        chain.push((m.publish, m.price));
        t = m.publish;
    }
    let earliest = EXPIRY - i64::from(p.fallback_lookback_s());
    let fallback = msgs
        .iter()
        .filter(|m| {
            m.prev > 0 && m.prev < m.publish && m.publish >= earliest && m.publish <= EXPIRY
        })
        .max_by_key(|m| m.publish)
        .map(|m| (m.publish, m.price));
    Reference {
        slots,
        chain,
        fallback,
    }
}

/// Deterministic permutation and subset from a seed (splitmix64).
fn shuffle<T: Copy>(items: &[T], seed: u64, keep_all: bool) -> Vec<T> {
    let mut x = seed;
    let mut next = || {
        x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = x;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    let mut v: Vec<T> = items
        .iter()
        .copied()
        .filter(|_| keep_all || next() % 3 != 0)
        .collect();
    for i in (1..v.len()).rev() {
        let j = (next() % (i as u64 + 1)) as usize;
        v.swap(i, j);
    }
    v
}

/// Posts `msgs` in the given order, then again until nothing changes: a chain link posted
/// before its predecessor is rejected and must be posted again later, as a keeper would.
fn post_all(p: &OracleParams, msgs: &[Msg]) -> SettlementState {
    let mut s = SettlementState::default();
    let mut n = 0i64;
    loop {
        let before = s;
        for m in msgs {
            if let Ok(print) = Print::try_new(m.price, m.publish, m.prev) {
                let _ = s.add_print(p, UnixTimestamp::new(EXPIRY), print, n);
            }
            n += 1;
        }
        if s == before {
            return s;
        }
    }
}

fn steps() -> impl Strategy<Value = (bool, Vec<Step>)> {
    (
        any::<bool>(),
        prop::collection::vec(
            (any::<u8>(), prop::bool::weighted(0.1), 1..=10_000_000u64),
            1_000..2_500,
        ),
    )
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn test_whole_tape_in_any_order_gives_the_reference(steps in steps(), seed in any::<u64>()) {
        let p = params();
        let msgs = tape(&steps);
        let r = reference(&p, &msgs);
        let s = post_all(&p, &shuffle(&msgs, seed, true)).parts();

        for (i, slot) in r.slots.iter().enumerate() {
            prop_assert_eq!(s.filled_slots & (1 << i) != 0, slot.is_some(), "slot {}", i);
        }
        let sum: u128 = r.slots.iter().flatten().map(|&x| u128::from(x)).sum();
        prop_assert_eq!(s.sample_sum, sum);

        prop_assert_eq!(usize::from(s.next_len), r.chain.len());
        for (j, &(t, px)) in r.chain.iter().enumerate() {
            prop_assert_eq!((s.next_times[j], s.next_prices[j]), (t, px));
        }
        prop_assert_eq!(
            (s.fallback_time, s.fallback_price),
            r.fallback.unwrap_or((0, 0))
        );
    }

    #[test]
    fn test_any_subset_in_any_order_never_stores_a_non_canonical_price(
        steps in steps(),
        seed in any::<u64>(),
    ) {
        let p = params();
        let msgs = tape(&steps);
        let r = reference(&p, &msgs);
        let s = post_all(&p, &shuffle(&msgs, seed, false)).parts();

        let mut sum: u128 = 0;
        for (i, slot) in r.slots.iter().enumerate() {
            if s.filled_slots & (1 << i) != 0 {
                let px = slot.expect("a slot without a canonical print was filled");
                sum += u128::from(px);
            }
        }
        prop_assert_eq!(s.sample_sum, sum);
        prop_assert!(usize::from(s.next_len) <= r.chain.len());
        prop_assert!(usize::from(s.next_len) <= MAX_NEXT_PRINTS);
        for j in 0..usize::from(s.next_len) {
            // Chain links can only be stored in order, so the stored chain is a prefix.
            prop_assert_eq!((s.next_times[j], s.next_prices[j]), r.chain[j]);
        }
        if s.fallback_time != 0 {
            let (rt, _) = r.fallback.expect("a fallback without a reference fallback");
            prop_assert!(s.fallback_time <= rt);
            let m = msgs.iter().find(|m| m.publish == s.fallback_time && m.prev < m.publish).unwrap();
            prop_assert_eq!(s.fallback_price, m.price);
        }
    }
}

/// The tapes exercise every candidate: full and partial windows, complete and stalled
/// chains, and fallbacks. Guards against the properties above passing vacuously.
#[test]
fn test_synthetic_tapes_cover_every_candidate() {
    use proptest::strategy::ValueTree;
    let mut runner = proptest::test_runner::TestRunner::new(config());
    let p = params();
    let (mut full_window, mut partial_window, mut empty_window) = (0, 0, 0);
    let (mut full_chain, mut short_chain, mut fallback) = (0, 0, 0);
    for _ in 0..500 {
        let r = reference(&p, &tape(&steps().new_tree(&mut runner).unwrap().current()));
        match r.slots.iter().flatten().count() {
            30 => full_window += 1,
            0 => empty_window += 1,
            _ => partial_window += 1,
        }
        if r.chain.len() == 5 {
            full_chain += 1;
        } else {
            short_chain += 1;
        }
        fallback += usize::from(r.fallback.is_some());
    }
    println!(
        "windows full {full_window} partial {partial_window} empty {empty_window}; \
         chains full {full_chain} short {short_chain}; fallbacks {fallback}"
    );
    assert!(full_window >= 50 && partial_window >= 50, "window coverage");
    assert!(
        full_chain >= 50 && fallback >= 50,
        "chain and fallback coverage"
    );
}
