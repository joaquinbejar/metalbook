//! Property tests for the payoff and collateral formulas: the collateral is never below the
//! exact maximum payoff, the payout is never above the exact payoff, both are within one
//! atom of exact, and nothing panics, for puts and capped calls across the whole `u64`
//! domain.
//!
//! The reference is computed here in `u128` from the textbook payoff, independently of the
//! crate's per-ounce helpers.

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
    Contracts, CoreError, OptionKind, OuncesPerContract, SeriesKey, SeriesTerms, StrikeUsd,
    Underlying, UnixTimestamp, MILLI_OUNCES_PER_OUNCE,
};
use proptest::prelude::*;
use proptest::test_runner::{Config, RngSeed};

const MILLI: u128 = MILLI_OUNCES_PER_OUNCE as u128;

/// Cases per property. `METALBOOK_PROPTEST_CASES` overrides the default of 200_000.
fn config() -> Config {
    let cases = std::env::var("METALBOOK_PROPTEST_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200_000);
    Config {
        cases,
        rng_seed: RngSeed::Fixed(0x3E7A_1B00_C0DE_0001),
        failure_persistence: None,
        ..Config::default()
    }
}

/// Realistic prices (up to 10 million USD per ounce) half the time, any `u64` otherwise.
fn price() -> impl Strategy<Value = u64> {
    prop_oneof![0..=10_000_000_000_000u64, any::<u64>()]
}

/// Realistic multipliers (up to 1000 oz) half the time, any non-zero `u32` otherwise.
fn multiplier() -> impl Strategy<Value = u32> {
    prop_oneof![1..=1_000_000u32, 1..=u32::MAX]
}

fn kind() -> impl Strategy<Value = OptionKind> {
    prop_oneof![Just(OptionKind::Call), Just(OptionKind::Put)]
}

/// Exact maximum payoff per ounce: `K` for a put, `cap − K` for a call.
fn ref_bound(kind: OptionKind, k: u64, cap: u64) -> u128 {
    match kind {
        OptionKind::Put => u128::from(k),
        OptionKind::Call => u128::from(cap) - u128::from(k),
    }
}

/// Exact payoff per ounce at `s`.
fn ref_payoff(kind: OptionKind, k: u64, cap: u64, s: u64) -> u128 {
    let (k, cap, s) = (i128::from(k), i128::from(cap), i128::from(s));
    let v = match kind {
        OptionKind::Put => (k - s).max(0),
        OptionKind::Call => (s - k).max(0).min(cap - k),
    };
    v as u128
}

/// Builds terms; for a call, `cap = K + delta` (skipped if it overflows).
fn build(kind: OptionKind, k: u64, delta: u64, mult: u32) -> Option<(SeriesTerms, u64)> {
    let cap = match kind {
        OptionKind::Put => 0,
        OptionKind::Call => k.checked_add(delta.max(1))?,
    };
    let key = SeriesKey::try_new(
        Underlying::Xau,
        kind,
        StrikeUsd::new(k),
        UnixTimestamp::new(1_800_000_000),
    )
    .ok()?;
    let m = OuncesPerContract::try_new(mult).ok()?;
    SeriesTerms::try_new(key, m, StrikeUsd::new(cap))
        .ok()
        .map(|t| (t, cap))
}

proptest! {
    #![proptest_config(config())]

    /// `try_new` accepts exactly the terms whose rounded-up maximum payout fits a `u64`.
    #[test]
    fn test_terms_overflow_exactly_when_max_payout_does_not_fit(
        kind in kind(), k in 1..=u64::MAX, delta in price(), mult in multiplier(),
    ) {
        let cap = match kind {
            OptionKind::Put => 0,
            OptionKind::Call => match k.checked_add(delta.max(1)) {
                Some(c) => c,
                None => return Ok(()),
            },
        };
        let key = SeriesKey::try_new(Underlying::Xag, kind, StrikeUsd::new(k), UnixTimestamp::new(0))
            .unwrap();
        let m = OuncesPerContract::try_new(mult).unwrap();
        let exact = ref_bound(kind, k, cap) * u128::from(mult);
        let ceil = exact.div_ceil(MILLI);
        let got = SeriesTerms::try_new(key, m, StrikeUsd::new(cap));
        if ceil > u128::from(u64::MAX) {
            prop_assert_eq!(got, Err(CoreError::Overflow));
        } else {
            prop_assert!(got.is_ok());
        }
    }

    /// A call's cap must be above the strike; a put's cap must be zero.
    #[test]
    fn test_terms_cap_rule(kind in kind(), k in 1..=u64::MAX, cap in any::<u64>()) {
        let key = SeriesKey::try_new(Underlying::Xau, kind, StrikeUsd::new(k), UnixTimestamp::new(0))
            .unwrap();
        let got = SeriesTerms::try_new(key, OuncesPerContract::try_new(1).unwrap(), StrikeUsd::new(cap));
        let valid = match kind {
            OptionKind::Call => cap > k,
            OptionKind::Put => cap == 0,
        };
        if !valid {
            prop_assert_eq!(got, Err(CoreError::InvalidCap));
        } else {
            // With a 1 milli-ounce multiplier the maximum payout always fits.
            prop_assert!(got.is_ok());
        }
    }

    /// Collateral per contract is the exact maximum payoff rounded up; the payout is the exact
    /// payoff rounded down; payout + residual = collateral per contract; no panics.
    #[test]
    fn test_collateral_up_payout_down_within_one_atom(
        kind in kind(), k in price(), delta in price(), mult in multiplier(), s in price(),
    ) {
        let Some((t, cap)) = build(kind, k.max(1), delta, mult) else { return Ok(()); };
        let k = k.max(1);
        let m = u128::from(mult);

        let max = u128::from(t.max_payout_per_contract().unwrap().get());
        let exact_max = ref_bound(kind, k, cap) * m;
        prop_assert!(max * MILLI >= exact_max, "collateral below the exact maximum payoff");
        prop_assert!(max * MILLI - exact_max < MILLI, "collateral more than one atom above exact");

        let payout = u128::from(t.payout_per_contract(StrikeUsd::new(s)).unwrap().get());
        let exact_payoff = ref_payoff(kind, k, cap, s) * m;
        prop_assert!(payout * MILLI <= exact_payoff, "payout above the exact payoff");
        prop_assert!(exact_payoff - payout * MILLI < MILLI, "payout more than one atom below exact");

        prop_assert!(payout <= max, "payout above the collateral per contract");
        let residual = u128::from(t.writer_residual_per_contract(StrikeUsd::new(s)).unwrap().get());
        prop_assert_eq!(payout + residual, max);
    }

    /// For `n` contracts the collateral covers `n` exact maximum payoffs, hence every payout.
    #[test]
    fn test_collateral_for_n_covers_n_payouts(
        kind in kind(), k in price(), delta in price(), mult in multiplier(), s in price(),
        n in prop_oneof![1..=1_000_000u64, 1..=u64::MAX],
    ) {
        let Some((t, cap)) = build(kind, k.max(1), delta, mult) else { return Ok(()); };
        let k = k.max(1);
        let n_u = u128::from(n);
        let max = u128::from(t.max_payout_per_contract().unwrap().get());
        match t.collateral(Contracts::new(n)) {
            Ok(c) => {
                let c = u128::from(c.get());
                prop_assert_eq!(c, max * n_u);
                prop_assert!(c * MILLI >= ref_bound(kind, k, cap) * u128::from(mult) * n_u);
                let payout = u128::from(t.payout_per_contract(StrikeUsd::new(s)).unwrap().get());
                prop_assert!(payout * n_u <= c);
            }
            Err(e) => {
                prop_assert_eq!(e, CoreError::Overflow);
                prop_assert!(max * n_u > u128::from(u64::MAX));
            }
        }
    }

    /// A put's payout never rises with the price; a capped call's never falls.
    #[test]
    fn test_payout_is_monotone_in_price(
        kind in kind(), k in price(), delta in price(), mult in multiplier(),
        a in price(), b in price(),
    ) {
        let Some((t, _)) = build(kind, k.max(1), delta, mult) else { return Ok(()); };
        let (lo, hi) = (a.min(b), a.max(b));
        let p_lo = t.payout_per_contract(StrikeUsd::new(lo)).unwrap();
        let p_hi = t.payout_per_contract(StrikeUsd::new(hi)).unwrap();
        match kind {
            OptionKind::Put => prop_assert!(p_lo >= p_hi),
            OptionKind::Call => prop_assert!(p_lo <= p_hi),
        }
    }
}
