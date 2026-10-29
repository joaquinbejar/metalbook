//! Property tests for the vault accounting: random writers mint and close, the series settles
//! at a random price, then longs redeem in random chunks and writers claim, all in a random
//! order. The vault never goes below zero, the accounting identity holds after every step,
//! and the vault ends at exactly zero.

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

use metalbook_core::{
    Contracts, OptionKind, OuncesPerContract, SeriesKey, SeriesLedger, SeriesState, SeriesTerms,
    StrikeUsd, Underlying, UnixTimestamp, UsdcAtoms,
};
use proptest::prelude::*;
use proptest::test_runner::{Config, RngSeed};

/// Cases per property. `METALBOOK_PROPTEST_CASES` overrides the default of 20_000.
fn config() -> Config {
    let cases = std::env::var("METALBOOK_PROPTEST_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20_000);
    Config {
        cases,
        rng_seed: RngSeed::Fixed(0x3E7A_1B00_C0DE_0002),
        failure_persistence: None,
        ..Config::default()
    }
}

/// Deterministic generator for the claim order (splitmix64).
struct SplitMix(u64);

impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

#[derive(Debug, Clone, Copy)]
enum Event {
    Redeem(u64),
    Claim(usize),
}

fn terms(kind: OptionKind, k: u64, delta: u64, mult: u32) -> SeriesTerms {
    let cap = match kind {
        OptionKind::Put => 0,
        OptionKind::Call => k + delta,
    };
    let key = SeriesKey::try_new(
        Underlying::Xag,
        kind,
        StrikeUsd::new(k),
        UnixTimestamp::new(1_800_000_000),
    )
    .unwrap();
    SeriesTerms::try_new(
        key,
        OuncesPerContract::try_new(mult).unwrap(),
        StrikeUsd::new(cap),
    )
    .unwrap()
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn test_vault_conserves_and_empties_exactly(
        kind in prop_oneof![Just(OptionKind::Call), Just(OptionKind::Put)],
        // Up to 10_000 USD per ounce, multipliers up to 100 oz: no overflow by construction.
        k in 1..=10_000_000_000u64,
        delta in 1..=10_000_000_000u64,
        mult in 1..=100_000u32,
        writers in 1..=6usize,
        ops in prop::collection::vec((0..6usize, any::<bool>(), 1..=1_000u64), 0..40),
        settle_ratio in 0..=3_000u64,
        seed in any::<u64>(),
    ) {
        let t = terms(kind, k, delta, mult);
        let max = t.max_payout_per_contract().unwrap();
        let mut ledger = SeriesLedger::new(t);
        let mut written = vec![0u64; writers];
        let mut deposited: u128 = 0;
        let mut withdrawn: u128 = 0;

        for (w, is_close, n) in ops {
            let w = w % writers;
            if is_close {
                let n = n.min(written[w]);
                if n == 0 {
                    continue;
                }
                let out = ledger.close(Contracts::new(n)).unwrap();
                prop_assert_eq!(out.get(), max.get() * n);
                written[w] -= n;
                withdrawn += u128::from(out.get());
            } else {
                let dep = ledger.mint(Contracts::new(n)).unwrap();
                prop_assert_eq!(dep.get(), max.get() * n);
                written[w] += n;
                deposited += u128::from(dep.get());
            }
            ledger.check_invariants().unwrap();
            prop_assert_eq!(u128::from(ledger.vault().get()), deposited - withdrawn);
        }

        let total: u64 = written.iter().sum();
        prop_assert_eq!(ledger.open_contracts().get(), total);
        ledger.expire().unwrap();

        // Settlement anywhere from 0 to 3 times the larger of strike and cap.
        let top = k.max(t.cap().get());
        let s = top / 1_000 * settle_ratio;
        ledger.settle(StrikeUsd::new(s)).unwrap();
        prop_assert_eq!(ledger.state(), SeriesState::Settled);
        ledger.check_invariants().unwrap();
        let (payout, residual) = ledger.settlement_amounts().unwrap();
        prop_assert!(payout <= max);

        // Longs redeem in random chunks; each writer claims once; random order.
        let mut rng = SplitMix(seed);
        let mut events = Vec::new();
        let mut left = total;
        while left > 0 {
            let chunk = 1 + rng.below(left);
            events.push(Event::Redeem(chunk));
            left -= chunk;
        }
        for (w, &n) in written.iter().enumerate() {
            if n > 0 {
                events.push(Event::Claim(w));
            }
        }
        for i in (1..events.len()).rev() {
            let j = rng.below(i as u64 + 1) as usize;
            events.swap(i, j);
        }

        let mut paid: u128 = 0;
        for e in events {
            let out = match e {
                Event::Redeem(n) => {
                    let out = ledger.redeem(Contracts::new(n)).unwrap();
                    prop_assert_eq!(out.get(), payout.get() * n);
                    out
                }
                Event::Claim(w) => {
                    let out = ledger.claim_writer(Contracts::new(written[w])).unwrap();
                    prop_assert_eq!(out.get(), residual.get() * written[w]);
                    out
                }
            };
            paid += u128::from(out.get());
            ledger.check_invariants().unwrap();
        }

        prop_assert_eq!(ledger.vault(), UsdcAtoms::ZERO);
        prop_assert_eq!(ledger.longs_outstanding(), Contracts::ZERO);
        prop_assert_eq!(ledger.writers_unclaimed(), Contracts::ZERO);
        prop_assert_eq!(paid, deposited - withdrawn);
    }
}
