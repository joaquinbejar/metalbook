//! Tests of the pricing adapter against an independent reference: OptionStratLib 0.23's
//! Black-Scholes value and greeks (dev-only), plus mock pricers for the error-bound and
//! consistency rules.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::integer_division,
    clippy::float_arithmetic,
    clippy::panic_in_result_fn,
    clippy::disallowed_macros,
    missing_docs
)]

use std::cell::Cell;

use metalbook_core::{
    OptionKind, OuncesPerContract, SeriesKey, SeriesTerms, StrikeUsd, Underlying, UnixTimestamp,
};
use metalbook_maker::{
    mark, quote_atoms, quote_terms, Listing, MakerError, Mark, MarketInputs, VanillaInputs,
    VanillaKind, VanillaPricer, VanillaValue, SECONDS_PER_YEAR,
};
use optionstratlib::greeks::{delta, gamma, theta, vega};
use optionstratlib::prelude::{ExpirationDate, OptionStyle, OptionType, Options, Positive, Side};
use optionstratlib::pricing::black_scholes_model::black_scholes;
use proptest::prelude::*;
use proptest::test_runner::{Config, RngSeed};
use quotebook_core::{Address, SeriesId, Side as QbSide};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

const EXPIRY: i64 = 1_800_000_000;
const WINDOW_S: u32 = 1_800;
const USD: u64 = 1_000_000;

/// The reference: OptionStratLib's European Black-Scholes, per ounce.
struct Osl;

fn pos(d: Decimal) -> Positive {
    Positive::new_decimal(d).unwrap()
}

impl Osl {
    fn option(kind: VanillaKind, i: &VanillaInputs) -> Options {
        Options::new(
            OptionType::European,
            Side::Long,
            "XAU".to_string(),
            pos(i.strike),
            ExpirationDate::Days(pos(i.years * dec!(365))),
            pos(i.vol),
            Positive::ONE,
            pos(i.spot),
            i.rate,
            match kind {
                VanillaKind::Call => OptionStyle::Call,
                VanillaKind::Put => OptionStyle::Put,
            },
            pos(i.carry),
            None,
        )
    }
}

impl VanillaPricer for Osl {
    fn price(&self, kind: VanillaKind, i: &VanillaInputs) -> Result<VanillaValue, MakerError> {
        let o = Self::option(kind, i);
        let e = |x: String| MakerError::Pricer(x);
        Ok(VanillaValue {
            value: black_scholes(&o).map_err(|x| e(format!("{x:?}")))?,
            delta: delta(&o).map_err(|x| e(format!("{x:?}")))?,
            gamma: gamma(&o).map_err(|x| e(format!("{x:?}")))?,
            vega: vega(&o).map_err(|x| e(format!("{x:?}")))?,
            theta: theta(&o).map_err(|x| e(format!("{x:?}")))?,
            error_bound: dec!(0.000000001),
        })
    }
}

fn terms(kind: OptionKind, strike_usd: u64, cap_usd: u64, milli_oz: u32) -> SeriesTerms {
    let key = SeriesKey::try_new(
        Underlying::Xau,
        kind,
        StrikeUsd::new(strike_usd * USD),
        UnixTimestamp::new(EXPIRY),
    )
    .unwrap();
    SeriesTerms::try_new(
        key,
        OuncesPerContract::try_new(milli_oz).unwrap(),
        StrikeUsd::new(cap_usd * USD),
    )
    .unwrap()
}

fn market(spot: Decimal, days_left: i64) -> MarketInputs {
    MarketInputs {
        spot,
        vol: dec!(0.18),
        rate: dec!(0.04),
        carry: dec!(0.0),
        now: EXPIRY - days_left * 86_400,
        market_open: true,
    }
}

fn inputs(m: &MarketInputs, strike: Decimal) -> VanillaInputs {
    VanillaInputs {
        spot: m.spot,
        strike,
        years: Decimal::from(EXPIRY - m.now) / Decimal::from(SECONDS_PER_YEAR),
        vol: m.vol,
        rate: m.rate,
        carry: m.carry,
    }
}

#[test]
fn test_put_mark_is_the_reference_put_scaled() {
    let t = terms(OptionKind::Put, 4_000, 0, 50_000); // 50 oz
    let m = market(dec!(3950), 30);
    let got = mark(&Osl, &t, &m, WINDOW_S).unwrap();
    let r = Osl
        .price(VanillaKind::Put, &inputs(&m, dec!(4000)))
        .unwrap();
    assert_eq!(got.value, r.value * dec!(50));
    assert_eq!(got.delta, r.delta * dec!(50));
    assert_eq!(got.vega, r.vega * dec!(50));
    assert_eq!(got.max_payout, dec!(200000));
    assert!(got.value > Decimal::ZERO && got.delta < Decimal::ZERO);
}

#[test]
fn test_capped_call_mark_is_the_reference_call_spread() {
    let t = terms(OptionKind::Call, 4_000, 4_400, 1_000);
    let m = market(dec!(4100), 14);
    let got = mark(&Osl, &t, &m, WINDOW_S).unwrap();
    let ck = Osl
        .price(VanillaKind::Call, &inputs(&m, dec!(4000)))
        .unwrap();
    let cc = Osl
        .price(VanillaKind::Call, &inputs(&m, dec!(4400)))
        .unwrap();
    assert_eq!(got.value, ck.value - cc.value);
    assert_eq!(got.delta, ck.delta - cc.delta);
    assert_eq!(got.gamma, ck.gamma - cc.gamma);
    assert_eq!(got.vega, ck.vega - cc.vega);
    assert_eq!(got.theta, ck.theta - cc.theta);
    // A vanilla call would be worth more than the capped one (the cap leg is positive).
    assert!(ck.value > got.value);
    assert_eq!(got.max_payout, dec!(400));
    assert_eq!(got.error_bound, dec!(0.000000002));
}

#[test]
fn test_capped_call_far_above_cap_is_near_discounted_max_and_never_above_max() {
    let t = terms(OptionKind::Call, 4_000, 4_400, 1_000);
    let m = market(dec!(9000), 30);
    let got = mark(&Osl, &t, &m, WINDOW_S).unwrap();
    let years = dec!(30) / dec!(365);
    // (cap − K) e^{−rT}, from the reference's own put-call bounds: within a cent.
    let discounted = dec!(400) * (Decimal::ONE - dec!(0.04) * years + dec!(0.0008) * years * years);
    assert!(got.value <= got.max_payout);
    assert!(
        (got.value - discounted).abs() < dec!(0.01),
        "{} vs {}",
        got.value,
        discounted
    );
}

/// A pricer returning fixed legs, for the error-bound and consistency rules.
struct Fixed {
    by_strike: Vec<(Decimal, Decimal, Decimal)>, // strike, value, error bound
    calls: Cell<u32>,
}

impl VanillaPricer for Fixed {
    fn price(&self, _: VanillaKind, i: &VanillaInputs) -> Result<VanillaValue, MakerError> {
        self.calls.set(self.calls.get() + 1);
        let (_, value, error_bound) = *self
            .by_strike
            .iter()
            .find(|(k, _, _)| *k == i.strike)
            .unwrap();
        Ok(VanillaValue {
            value,
            delta: Decimal::ZERO,
            gamma: Decimal::ZERO,
            vega: Decimal::ZERO,
            theta: Decimal::ZERO,
            error_bound,
        })
    }
}

#[test]
fn test_spread_error_bound_is_the_sum_even_when_errors_cancel() {
    // Both legs overstated by the same 0.30: the spread's actual error is 0, but the bound
    // must still be 0.25 + 0.35, never the difference or zero.
    let t = terms(OptionKind::Call, 4_000, 4_400, 2_000); // 2 oz
    let p = Fixed {
        by_strike: vec![
            (dec!(4000), dec!(150.30), dec!(0.25)),
            (dec!(4400), dec!(20.30), dec!(0.35)),
        ],
        calls: Cell::new(0),
    };
    let got = mark(&p, &t, &market(dec!(4100), 14), WINDOW_S).unwrap();
    assert_eq!(p.calls.get(), 2);
    assert_eq!(got.value, dec!(260.00)); // (150.30 − 20.30) × 2
    assert_eq!(got.error_bound, dec!(1.20)); // (0.25 + 0.35) × 2
}

#[test]
fn test_inconsistent_pricer_is_rejected_and_small_excess_is_clamped() {
    let t = terms(OptionKind::Call, 4_000, 4_400, 1_000);
    let m = market(dec!(4100), 14);
    // Spread 400.5 with bound 1: within the bound, clamped to the max payout 400.
    let ok = Fixed {
        by_strike: vec![
            (dec!(4000), dec!(400.5), dec!(0.5)),
            (dec!(4400), dec!(0), dec!(0.5)),
        ],
        calls: Cell::new(0),
    };
    assert_eq!(mark(&ok, &t, &m, WINDOW_S).unwrap().value, dec!(400));
    // Spread 402 with bound 1: a pricer bug.
    let bad = Fixed {
        by_strike: vec![
            (dec!(4000), dec!(402), dec!(0.5)),
            (dec!(4400), dec!(0), dec!(0.5)),
        ],
        calls: Cell::new(0),
    };
    assert_eq!(
        mark(&bad, &t, &m, WINDOW_S),
        Err(MakerError::PricerInconsistent)
    );
    // Negative beyond the bound.
    let neg = Fixed {
        by_strike: vec![
            (dec!(4000), dec!(1), dec!(0.1)),
            (dec!(4400), dec!(3), dec!(0.1)),
        ],
        calls: Cell::new(0),
    };
    assert_eq!(
        mark(&neg, &t, &m, WINDOW_S),
        Err(MakerError::PricerInconsistent)
    );
}

#[test]
fn test_no_quotes_when_closed_or_in_the_settlement_window() {
    let t = terms(OptionKind::Put, 4_000, 0, 1_000);
    let mut m = market(dec!(4000), 1);
    m.market_open = false;
    assert_eq!(mark(&Osl, &t, &m, WINDOW_S), Err(MakerError::MarketClosed));
    m.market_open = true;
    m.now = EXPIRY - i64::from(WINDOW_S);
    assert_eq!(
        mark(&Osl, &t, &m, WINDOW_S),
        Err(MakerError::SettlementWindow)
    );
    m.now = EXPIRY + 1;
    assert_eq!(
        mark(&Osl, &t, &m, WINDOW_S),
        Err(MakerError::SettlementWindow)
    );
    m.now = EXPIRY - i64::from(WINDOW_S) - 1;
    assert!(mark(&Osl, &t, &m, WINDOW_S).is_ok());
    m.spot = Decimal::ZERO;
    assert!(matches!(
        mark(&Osl, &t, &m, WINDOW_S),
        Err(MakerError::InvalidInput(_))
    ));
}

fn a_mark(value: Decimal, error: Decimal, max: Decimal) -> Mark {
    Mark {
        value,
        delta: Decimal::ZERO,
        gamma: Decimal::ZERO,
        vega: Decimal::ZERO,
        theta: Decimal::ZERO,
        error_bound: error,
        max_payout: max,
    }
}

#[test]
fn test_quotes_round_toward_the_maker_and_stay_in_bounds() {
    // value 123.4567891, error 0.0000009, half spread 0.5, tick 0.01 USD.
    let q = quote_atoms(
        &a_mark(dec!(123.4567891), dec!(0.0000009), dec!(400)),
        dec!(0.5),
        10_000,
    )
    .unwrap();
    assert_eq!(q.bid, 122_950_000); // 122.9567882 rounded down
    assert_eq!(q.ask, 123_960_000); // 123.9567900 rounded up
                                    // Never above the max payout, never below zero.
    let q = quote_atoms(&a_mark(dec!(399.9), dec!(0.1), dec!(400)), dec!(1), 10_000).unwrap();
    assert_eq!(q.ask, 400_000_000);
    let q = quote_atoms(&a_mark(dec!(0.2), dec!(0.1), dec!(400)), dec!(1), 10_000).unwrap();
    assert_eq!((q.bid, q.ask), (0, 1_300_000));
    assert!(matches!(
        quote_atoms(&a_mark(dec!(1), dec!(0), dec!(2)), dec!(0), 0),
        Err(MakerError::InvalidInput(_))
    ));
    assert!(matches!(
        quote_atoms(&a_mark(dec!(1), dec!(0), dec!(2)), dec!(-1), 1),
        Err(MakerError::InvalidInput(_))
    ));
}

#[test]
fn test_quote_terms_convert_atoms_to_lots_exactly() {
    let listing = Listing {
        program_id: Address::new_from_array([1; 32]),
        deployment: [2; 16],
        series: SeriesId::new(Address::new_from_array([3; 32])),
        quote_lot_size_atoms: 10_000,
    };
    let maker = Address::new_from_array([4; 32]);
    let taker = Address::new_from_array([0; 32]);
    let t = quote_terms(
        &listing,
        maker,
        taker,
        QbSide::Ask,
        123_960_000,
        3,
        100,
        160,
        7,
    )
    .unwrap();
    assert_eq!((t.price.get(), t.size.get(), t.nonce.get()), (12_396, 3, 7));
    assert_eq!(
        quote_terms(
            &listing,
            maker,
            taker,
            QbSide::Bid,
            123_960_001,
            3,
            100,
            160,
            7
        ),
        Err(MakerError::OffLotGrid)
    );
    assert!(matches!(
        quote_terms(&listing, maker, taker, QbSide::Bid, 10_000, 0, 100, 160, 7),
        Err(MakerError::Quote(_))
    ));
    assert!(matches!(
        quote_terms(&listing, maker, taker, QbSide::Bid, 0, 1, 100, 160, 7),
        Err(MakerError::Quote(_))
    ));
}

fn config() -> Config {
    let cases = std::env::var("METALBOOK_PROPTEST_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(500);
    Config {
        cases,
        rng_seed: RngSeed::Fixed(0x3E7A_1B00_C0DE_0005),
        failure_persistence: None,
        ..Config::default()
    }
}

proptest! {
    #![proptest_config(config())]

    /// Against the reference: the capped call is within [0, max payout], equals the reference
    /// spread, and is non-decreasing in spot; the put is non-increasing in spot.
    #[test]
    fn test_marks_respect_payoff_bounds_and_monotonicity(
        spot in 2_000u32..8_000, bump in 1u32..200, vol_bp in 500u32..8_000, days in 1i64..365,
        cap_pct in prop_oneof![Just(10u64), Just(25), Just(50)],
    ) {
        let call = terms(OptionKind::Call, 4_000, 4_000 + 40 * cap_pct, 1_000);
        let put = terms(OptionKind::Put, 4_000, 0, 1_000);
        let mut m = market(Decimal::from(spot), days);
        m.vol = Decimal::from(vol_bp) / dec!(10000);
        let mut up = m;
        up.spot = Decimal::from(spot + bump);
        let c = mark(&Osl, &call, &m, WINDOW_S).unwrap();
        let c_up = mark(&Osl, &call, &up, WINDOW_S).unwrap();
        prop_assert!(c.value >= Decimal::ZERO && c.value <= c.max_payout);
        prop_assert!(c_up.value + dec!(0.000001) >= c.value);
        let p = mark(&Osl, &put, &m, WINDOW_S).unwrap();
        let p_up = mark(&Osl, &put, &up, WINDOW_S).unwrap();
        prop_assert!(p.value >= Decimal::ZERO && p.value <= p.max_payout);
        prop_assert!(p_up.value <= p.value + dec!(0.000001));
        // Quotes bracket the mark and stay inside [0, max payout].
        let q = quote_atoms(&c, dec!(0.25), 10_000).unwrap();
        let v_atoms = c.value * dec!(1000000);
        prop_assert!(Decimal::from(q.bid) <= v_atoms && v_atoms <= Decimal::from(q.ask));
        prop_assert!(Decimal::from(q.ask) <= c.max_payout * dec!(1000000));
        prop_assert!(q.bid <= q.ask);
    }
}
