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

/// Codex's case on #13: the bid must round the exact value down. 699999999999.99999999999999999
/// USD is 699_999_999_999_999_999.99999999999 atoms, less than one tick of 7e17 atoms, so the
/// bid is 0 ticks. A Decimal quotient rounded before the floor gave 1 tick.
#[test]
fn test_quote_rounding_regression_large_tick() {
    let value: Decimal = "699999999999.99999999999999999".parse().unwrap();
    let m = a_mark(value, Decimal::ZERO, dec!(700000000000));
    let q = quote_atoms(&m, Decimal::ZERO, 700_000_000_000_000_000).unwrap();
    assert_eq!((q.bid, q.ask), (0, 700_000_000_000_000_000));
}

/// Codex's cases at ordinary scale (#13): a bid just below one tick, an ask just above one
/// tick, and a maximum payout just below two ticks (the ask's ceiling). Each rounded a
/// Decimal quotient across the tick before.
#[test]
fn test_quote_rounding_regressions_ordinary_scale() {
    let tick = 7_000_000;
    let v: Decimal = "6.9999999999999999999999999999".parse().unwrap();
    let q = quote_atoms(&a_mark(v, Decimal::ZERO, dec!(7)), Decimal::ZERO, tick).unwrap();
    assert_eq!((q.bid, q.ask), (0, 7_000_000), "bid side");
    let v: Decimal = "7.0000000000000000000000000001".parse().unwrap();
    let q = quote_atoms(&a_mark(v, Decimal::ZERO, dec!(14)), Decimal::ZERO, tick).unwrap();
    assert_eq!((q.bid, q.ask), (7_000_000, 14_000_000), "ask side");
    // 28 significant digits (a 29th would not fit the 96-bit mantissa and parse as 14).
    let max: Decimal = "13.99999999999999999999999999".parse().unwrap();
    assert!(max < dec!(14));
    let q = quote_atoms(&a_mark(dec!(13), Decimal::ZERO, max), Decimal::ZERO, tick).unwrap();
    assert_eq!((q.bid, q.ask), (7_000_000, 7_000_000), "cap floor");
}

/// Exact rational reference: `value = mantissa / 10^scale` USD, so in atoms
/// `mantissa × 10^6 / 10^scale`; floor and ceiling by integer division only.
/// A denominator too large for a `u128` exceeds every numerator here, so the quotient is 0.
fn reference_ticks(value: Decimal, tick: u64, up: bool) -> Option<u128> {
    let m = u128::try_from(value.mantissa()).ok()?;
    let num = m.checked_mul(1_000_000)?;
    let den = 10u128
        .checked_pow(value.scale())
        .and_then(|p| p.checked_mul(u128::from(tick)));
    let (q, exact) = match den {
        Some(den) => (num / den, num % den == 0),
        None => (0, num == 0),
    };
    (if up && !exact { q + 1 } else { q }).checked_mul(u128::from(tick))
}

proptest! {
    #![proptest_config(config())]

    /// Over arbitrary decimals (96-bit mantissas, up to 28 fractional digits) and arbitrary
    /// non-zero u64 ticks (small ones half the time),
    /// with zero error and spread, the bid is exactly the floor and the ask exactly the
    /// ceiling of the value on the tick grid (the ask capped at the max payout's floor).
    #[test]
    fn test_quote_rounding_matches_exact_rational_reference(
        mantissa in prop_oneof![0u128..=1_000_000_000_000u128, 0u128..=(1u128 << 96) - 1], scale in 0u32..=28, tick in prop_oneof![1u64..=1_000_000, 1u64..=u64::MAX],
    ) {
        let value = Decimal::from_i128_with_scale(mantissa as i128, scale);
        let max = value.ceil() + Decimal::ONE;
        let m = a_mark(value, Decimal::ZERO, max);
        match quote_atoms(&m, Decimal::ZERO, tick) {
            Ok(q) => {
                let bid = reference_ticks(value, tick, false).unwrap();
                let ask = reference_ticks(value, tick, true).unwrap();
                let ceiling = reference_ticks(max, tick, false).unwrap();
                prop_assert_eq!(u128::from(q.ask), ask.min(ceiling));
                prop_assert_eq!(u128::from(q.bid), bid.min(u128::from(q.ask)));
            }
            Err(MakerError::Overflow) => {
                // Only when an amount on the grid does not fit a u64 of atoms.
                let ask = reference_ticks(value, tick, true);
                let ceiling = reference_ticks(max, tick, false);
                prop_assert!(ask.is_none_or(|a| a > u128::from(u64::MAX)) || ceiling.is_none_or(|c| c > u128::from(u64::MAX)));
            }
            Err(e) => prop_assert!(false, "unexpected {e:?}"),
        }
    }
}

// optfix, the production pricer. OptionStratLib checks the wiring (kinds, units, greek
// conventions: OSL reports vega per vol point and theta per day, optfix per 1.00 of volatility
// and per calendar year), within OSL's own measured deviation: it computes in f64 (values off
// by up to about 2.3e-11 × max(S, K) over the grid below) and returns a gamma of 0 deep out of
// the money. Accuracy is pinned separately against 50-digit references.

fn grid() -> Vec<(VanillaKind, VanillaInputs)> {
    let mut out = Vec::new();
    for kind in [VanillaKind::Call, VanillaKind::Put] {
        for (spot, strike) in [
            (dec!(2400), dec!(2400)),
            (dec!(2400), dec!(2450)),
            (dec!(2400), dec!(2100)),
            (dec!(31.25), dec!(30)),
            (dec!(31.25), dec!(36.5)),
        ] {
            for days in [1, 7, 30, 91, 365] {
                for (vol, rate, carry) in [
                    (dec!(0.18), dec!(0.04), dec!(0)),
                    (dec!(0.32), dec!(0.05), dec!(0.01)),
                    (dec!(0.10), dec!(0), dec!(0)),
                ] {
                    let years = Decimal::from(days * 86_400) / Decimal::from(SECONDS_PER_YEAR);
                    out.push((
                        kind,
                        VanillaInputs {
                            spot,
                            strike,
                            years,
                            vol,
                            rate,
                            carry,
                        },
                    ));
                }
            }
        }
    }
    out
}

#[test]
fn test_optfix_matches_optionstratlib_within_its_deviation() {
    let points = grid();
    assert_eq!(points.len(), 150);
    for (kind, i) in points {
        let a = metalbook_maker::OptfixPricer.price(kind, &i).unwrap();
        let b = Osl.price(kind, &i).unwrap();
        let scale = i.spot.max(i.strike);
        // OSL's measured deviations over this grid, with a 2x to 4x margin.
        assert!(
            (a.value - b.value).abs() <= a.error_bound + dec!(0.0000000001) * scale,
            "{kind:?} {i:?}: {a:?} {b:?}"
        );
        assert!(
            (a.delta - b.delta).abs() <= dec!(0.0000000001),
            "{kind:?} {i:?}"
        );
        assert!(
            (a.gamma - b.gamma).abs() * scale <= dec!(0.00001),
            "{kind:?} {i:?}"
        );
        assert!(
            (a.vega / dec!(100) - b.vega).abs() <= dec!(0.000000001) * scale,
            "{kind:?} {i:?}"
        );
        assert!(
            (a.theta / dec!(365) - b.theta).abs() <= dec!(0.000000001) * scale,
            "{kind:?} {i:?}"
        );
        // At metal prices the bound is optfix's envelope plus noise: far below a USDC atom.
        assert!(a.error_bound > Decimal::ZERO && a.error_bound < dec!(0.0000001));
    }
}

/// 50-digit Black-Scholes-Merton references (mpmath, `mp.dps = 50`, rounded to 20 significant
/// digits): value, delta, gamma, vega per 1.00 of volatility, theta `dV/dt` per year, with
/// `T = days × 86_400 / 31_536_000`, `d1 = (ln(S/K) + (r − q + σ²/2) T) / (σ√T)`.
#[allow(clippy::type_complexity)]
const REFERENCES: [(
    VanillaKind,
    Decimal,
    Decimal,
    i64,
    Decimal,
    Decimal,
    Decimal,
    [Decimal; 5],
); 4] = [
    (
        VanillaKind::Call,
        dec!(2400),
        dec!(2450),
        7,
        dec!(0.18),
        dec!(0.04),
        dec!(0.0),
        [
            dec!(7.309022630464420632),
            dec!(0.21653791693961158376),
            dec!(0.0049042613377056468508),
            dec!(97.515526258993157765),
            dec!(-478.12171306497345416),
        ],
    ),
    (
        VanillaKind::Put,
        dec!(2400),
        dec!(2100),
        30,
        dec!(0.32),
        dec!(0.05),
        dec!(0.01),
        [
            dec!(6.1360422655087394197),
            dec!(-0.062067413380536141498),
            dec!(0.00055544617061396253244),
            dec!(84.147812192519649846),
            dec!(-157.54246727029801181),
        ],
    ),
    (
        VanillaKind::Call,
        dec!(31.25),
        dec!(36.5),
        91,
        dec!(0.1),
        dec!(0.0),
        dec!(0.0),
        [
            dec!(0.00043432001118649478186),
            dec!(0.0010172293988871140481),
            dec!(0.0021919897907549722097),
            dec!(0.05336875828304325532),
            dec!(-0.010703075150170762743),
        ],
    ),
    (
        VanillaKind::Put,
        dec!(31.25),
        dec!(30),
        365,
        dec!(0.18),
        dec!(0.04),
        dec!(0.0),
        [
            dec!(1.1537083313105301015),
            dec!(-0.29493960441297275795),
            dec!(0.061333692514934873754),
            dec!(10.781313137390895777),
            dec!(-0.55549534359654346845),
        ],
    ),
];

#[test]
fn test_optfix_is_within_its_bound_of_high_precision_references() {
    for (kind, spot, strike, days, vol, rate, carry, want) in REFERENCES {
        let i = VanillaInputs {
            spot,
            strike,
            years: Decimal::from(days * 86_400) / Decimal::from(SECONDS_PER_YEAR),
            vol,
            rate,
            carry,
        };
        let got = metalbook_maker::OptfixPricer.price(kind, &i).unwrap();
        // The reference is rounded to 20 digits: allow one unit there on top of the bound.
        let value_tol =
            got.error_bound + dec!(0.0000000000000000001) * want[0].abs().max(Decimal::ONE);
        assert!(
            (got.value - want[0]).abs() <= value_tol,
            "{kind:?} {days}: {got:?}"
        );
        // Greeks: optfix's envelopes are about 1e-13 relative; 1e-12 relative plus 1e-17.
        for (g, w) in [got.delta, got.gamma, got.vega, got.theta]
            .into_iter()
            .zip(&want[1..])
        {
            let tol = dec!(0.000000000001) * w.abs() + dec!(0.00000000000000001);
            assert!((g - w).abs() <= tol, "{kind:?} {days}: {g} vs {w}");
        }
    }
}

#[test]
fn test_optfix_marks_match_the_reference_marks() {
    for (t, spot, days) in [
        (terms(OptionKind::Put, 4_000, 0, 50_000), dec!(3950), 30),
        (terms(OptionKind::Call, 4_000, 4_400, 1_000), dec!(4100), 14),
    ] {
        let m = market(spot, days);
        let a = mark(&metalbook_maker::OptfixPricer, &t, &m, WINDOW_S).unwrap();
        let b = mark(&Osl, &t, &m, WINDOW_S).unwrap();
        // OSL's deviation (about 2.3e-11 × max(S, K) per ounce) times the ounces, with margin.
        let ounces = Decimal::from(t.multiplier().get()) / dec!(1000);
        let tol = a.error_bound + dec!(0.0000000001) * dec!(4400) * ounces;
        assert!((a.value - b.value).abs() <= tol, "{a:?} {b:?}");
        assert!((a.delta - b.delta).abs() <= dec!(0.0000001));
        assert_eq!(a.max_payout, b.max_payout);
    }
}

#[test]
fn test_optfix_near_expiry_and_out_of_domain() {
    let base = VanillaInputs {
        spot: dec!(2400),
        strike: dec!(2350),
        years: Decimal::ZERO,
        vol: dec!(0.18),
        rate: dec!(0.04),
        carry: dec!(0),
    };
    let p = metalbook_maker::OptfixPricer;
    // T = 0 is the intrinsic value, up to the output rounding.
    let call = p.price(VanillaKind::Call, &base).unwrap();
    assert!((call.value - dec!(50)).abs() <= call.error_bound);
    assert_eq!(
        p.price(VanillaKind::Put, &base).unwrap().value,
        Decimal::ZERO
    );
    // One second before expiry: the √ε form of the time term keeps the bound honest and small.
    let one_second = VanillaInputs {
        years: Decimal::ONE / Decimal::from(SECONDS_PER_YEAR),
        ..base
    };
    let v = p.price(VanillaKind::Call, &one_second).unwrap();
    assert!(v.value >= dec!(50) - v.error_bound && v.error_bound < dec!(0.000001));
    // optfix's domain: S/K in [0.05, 20], volatility in [0.005, 5], rates in [-0.1, 0.5].
    for bad in [
        VanillaInputs {
            strike: dec!(100000),
            ..one_second
        },
        VanillaInputs {
            vol: dec!(0.001),
            ..one_second
        },
        VanillaInputs {
            vol: dec!(6),
            ..one_second
        },
        VanillaInputs {
            rate: dec!(0.6),
            ..one_second
        },
        VanillaInputs {
            carry: dec!(-0.2),
            ..one_second
        },
        VanillaInputs {
            years: dec!(31),
            ..one_second
        },
        VanillaInputs {
            spot: dec!(-1),
            ..one_second
        },
    ] {
        assert!(
            matches!(p.price(VanillaKind::Put, &bad), Err(MakerError::Pricer(_))),
            "{bad:?}"
        );
    }
}
