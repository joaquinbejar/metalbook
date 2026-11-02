//! Safe weekly listing and rent reclaim (`docs/PLAN-improvements.md`, P4).
//!
//! `list-week` lists, for one underlying and one calendar expiry, a put and a capped call at
//! every strike of the metal's grid within ±5% of a fresh Pyth print: the shape listed by hand
//! on Oct 30 (16 XAU and 10 XAG series). Everything here is pure, so the command only adds the
//! reads and the sends; the program checks the same rules again on `create_series`.

use metalbook_keeper::PrintMsg;

use crate::{CliError, SeriesArgs};

/// Half-width of the listed band around spot, in basis points (±5%).
pub const LISTING_BAND_BPS: u16 = 500;

/// The oldest Pyth print `list-week` accepts by default, in seconds: older means the market is
/// closed or the feed stalled, and strikes around a stale spot are not listed.
pub const MAX_PRINT_AGE_S: i64 = 300;

/// How far in the future a Pyth print's `publish_time` may be against the local clock, in
/// seconds: clock skew between this machine and Pythnet. Anything later is refused as
/// impossible rather than treated as fresh.
pub const MAX_CLOCK_SKEW_S: i64 = 5;

/// Whether `list-week` or `reclaim` sends: `--execute` sends, the global `--dry-run` never does,
/// and asking for both is refused before anything is read or sent.
///
/// # Errors
///
/// [`CliError::Usage`] when both are given.
pub fn sends(dry_run: bool, execute: bool) -> Result<bool, CliError> {
    if dry_run && execute {
        return Err(CliError::Usage(
            "--dry-run and --execute conflict: nothing read or sent".into(),
        ));
    }
    Ok(execute)
}

/// `create_series` CU from `docs/benchmarks.md`: a put and a capped call.
pub const CREATE_PUT_CU: u32 = 29_600;
/// See [`CREATE_PUT_CU`].
pub const CREATE_CALL_CU: u32 = 34_218;

/// USDC atoms per USD.
const ATOMS_PER_USD: u128 = 1_000_000;
/// Basis points in one.
const BPS: u128 = 10_000;

/// A Pyth price (`price × 10^expo` USD per ounce) in USDC atoms per ounce, rounded down.
///
/// # Errors
///
/// [`CliError::Usage`] for a non-positive price or an exponent out of range.
pub fn spot_atoms(price: i64, expo: i32) -> Result<u64, CliError> {
    let price = u128::try_from(price)
        .ok()
        .filter(|p| *p > 0)
        .ok_or_else(|| CliError::Usage("the Pyth price is not positive".into()))?;
    // atoms = price × 10^(6 + expo)
    let shift = expo
        .checked_add(6)
        .ok_or_else(|| CliError::Usage("exponent out of range".into()))?;
    let pow = |n: i32| -> Result<u128, CliError> {
        u32::try_from(n)
            .ok()
            .and_then(|n| 10_u128.checked_pow(n))
            .ok_or_else(|| CliError::Usage("exponent out of range".into()))
    };
    let atoms = if shift >= 0 {
        price.checked_mul(pow(shift)?)
    } else {
        let neg = shift
            .checked_neg()
            .ok_or_else(|| CliError::Usage("exponent out of range".into()))?;
        price.checked_div(pow(neg)?)
    };
    atoms
        .and_then(|a| u64::try_from(a).ok())
        .ok_or_else(|| CliError::Usage("price out of range".into()))
}

/// Every multiple of `tick` within `[spot × (1 − band), spot × (1 + band)]`, ascending. The
/// lower bound rounds up to the grid and the upper bound down, so no strike lies outside the
/// band.
///
/// # Errors
///
/// [`CliError::Usage`] for a zero tick, a band of 100% or more, or an overflow.
pub fn band_strikes(spot: u64, tick: u64, band_bps: u16) -> Result<Vec<u64>, CliError> {
    let band = u128::from(band_bps);
    if tick == 0 || band >= BPS {
        return Err(CliError::Usage("strike tick or band out of range".into()));
    }
    let overflow = || CliError::Usage("strike band overflow".into());
    let spot = u128::from(spot);
    let tick = u128::from(tick);
    let low = spot
        .checked_mul(BPS.checked_sub(band).ok_or_else(overflow)?)
        .ok_or_else(overflow)?;
    let high = spot
        .checked_mul(BPS.checked_add(band).ok_or_else(overflow)?)
        .ok_or_else(overflow)?;
    // First multiple of tick with k × tick × BPS >= low: k = ceil(low / (tick × BPS)).
    let unit = tick.checked_mul(BPS).ok_or_else(overflow)?;
    let first = low.div_ceil(unit);
    let last = high.checked_div(unit).ok_or_else(overflow)?;
    let mut strikes = Vec::new();
    let mut k = first.max(1);
    while k <= last {
        let strike = k.checked_mul(tick).ok_or_else(overflow)?;
        strikes.push(u64::try_from(strike).map_err(|_| overflow())?);
        k = k.checked_add(1).ok_or_else(overflow)?;
    }
    Ok(strikes)
}

/// The cap of a call at `strike` for `cap_bps`, if it is exact (the program requires
/// `cap × 10_000 == strike × (10_000 + cap_bps)`).
#[must_use]
pub fn exact_cap(strike: u64, cap_bps: u16) -> Option<u64> {
    let num = u128::from(strike).checked_mul(BPS.checked_add(u128::from(cap_bps))?)?;
    if num.checked_rem(BPS)? != 0 {
        return None;
    }
    u64::try_from(num.checked_div(BPS)?).ok()
}

/// The expiry to list: `wanted` if given, else the first calendar expiry at least
/// `min_listing_s` after `now`. A wanted expiry must be in the calendar and at least
/// `min_listing_s` away: the program refuses anything else, and the CLI does not try.
///
/// # Errors
///
/// [`CliError::Usage`] when no such expiry exists or `wanted` breaks either rule.
pub fn listing_expiry(
    calendar: &[i64],
    now: i64,
    min_listing_s: u32,
    wanted: Option<i64>,
) -> Result<i64, CliError> {
    let earliest = now
        .checked_add(i64::from(min_listing_s))
        .ok_or_else(|| CliError::Usage("time overflow".into()))?;
    match wanted {
        Some(e) if !calendar.contains(&e) => Err(CliError::Usage(format!(
            "expiry {e} is not in the calendar"
        ))),
        Some(e) if e < earliest => Err(CliError::Usage(format!(
            "expiry {e} is closer than min_listing_s ({min_listing_s} s) from now"
        ))),
        Some(e) => Ok(e),
        None => calendar
            .iter()
            .copied()
            .filter(|e| *e >= earliest)
            .min()
            .ok_or_else(|| CliError::Usage("no calendar expiry far enough ahead".into())),
    }
}

/// Refuses a print older than `max_age_s` at `now`, or one with another exponent than the
/// config's for the metal.
///
/// # Errors
///
/// [`CliError::Usage`] naming the reason.
pub fn fresh_print(print: &PrintMsg, now: i64, max_age_s: i64, expo: i32) -> Result<(), CliError> {
    if print.expo != expo {
        return Err(CliError::Usage(format!(
            "the Pyth exponent is {}, the config pins {expo}",
            print.expo
        )));
    }
    if print.publish_time > now.saturating_add(MAX_CLOCK_SKEW_S) {
        return Err(CliError::Usage(format!(
            "the latest Pyth print is published {} s in the future (clock skew limit {MAX_CLOCK_SKEW_S} s): check the local clock; nothing listed",
            print.publish_time.saturating_sub(now)
        )));
    }
    let age = now.saturating_sub(print.publish_time);
    if age > max_age_s {
        return Err(CliError::Usage(format!(
            "the latest Pyth print is {age} s old (limit {max_age_s} s): market closed or feed \
             stalled; nothing listed"
        )));
    }
    Ok(())
}

/// The week's series for one underlying: a put and a capped call (first cap offset of the
/// config) at every band strike. Calls whose cap is not exact on the grid are left out.
///
/// # Errors
///
/// As [`band_strikes`].
pub fn week_series(
    underlying: u8,
    expiry: i64,
    spot: u64,
    tick: u64,
    band_bps: u16,
    cap_bps: u16,
) -> Result<Vec<SeriesArgs>, CliError> {
    let mut out = Vec::new();
    for strike in band_strikes(spot, tick, band_bps)? {
        out.push(SeriesArgs {
            underlying,
            kind: 1,
            strike,
            expiry,
            cap: 0,
            multiplier: None,
        });
        if cap_bps > 0 {
            if let Some(cap) = exact_cap(strike, cap_bps) {
                out.push(SeriesArgs {
                    underlying,
                    kind: 0,
                    strike,
                    expiry,
                    cap,
                    multiplier: None,
                });
            }
        }
    }
    Ok(out)
}

/// Whether a series owes nothing and can be closed: settled or voided, every long redeemed,
/// every writer claimed, nothing owed by the vault (the program's own `close_series` rule).
#[must_use]
pub fn reclaimable(s: &metalbook_sdk::core::accounts::Series) -> bool {
    matches!(s.state, 2 | 3)
        && s.longs_outstanding == 0
        && s.writers_unclaimed == 0
        && s.vault_owed == 0
}

/// USD of `atoms`, for display.
#[must_use]
pub fn usd(atoms: u64) -> String {
    let whole = u128::from(atoms).checked_div(ATOMS_PER_USD).unwrap_or(0);
    let frac = u128::from(atoms).checked_rem(ATOMS_PER_USD).unwrap_or(0);
    if frac == 0 {
        format!("{whole}")
    } else {
        let s = format!("{whole}.{frac:06}");
        s.trim_end_matches('0').to_string()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::disallowed_macros)]
mod tests {
    use super::*;

    const USD: u64 = 1_000_000;

    #[test]
    fn test_spot_atoms() {
        // XAU 3968.15 at expo -3; XAG 50.357 at expo -5.
        assert_eq!(spot_atoms(3_968_150, -3).unwrap(), 3_968_150_000);
        assert_eq!(spot_atoms(5_035_700, -5).unwrap(), 50_357_000);
        assert_eq!(spot_atoms(5, -8).unwrap(), 0); // rounds down
        assert!(spot_atoms(0, -3).is_err());
        assert!(spot_atoms(-1, -3).is_err());
        assert!(spot_atoms(i64::MAX, 20).is_err());
    }

    #[test]
    fn test_band_strikes_reproduce_the_oct_30_listing() {
        let xau = band_strikes(3_968_150_000, 50 * USD, LISTING_BAND_BPS).unwrap();
        assert_eq!(
            xau,
            (0..8).map(|i| (3_800 + 50 * i) * USD).collect::<Vec<_>>()
        );
        let xag = band_strikes(50_357_000, USD, LISTING_BAND_BPS).unwrap();
        assert_eq!(xag, (48..=52).map(|k| k * USD).collect::<Vec<_>>());
        assert!(band_strikes(1, 0, 500).is_err());
        assert!(band_strikes(1, 1, 10_000).is_err());
    }

    #[test]
    fn test_band_edges_are_inclusive_and_never_outside() {
        // Spot 100, band 5%: [95, 105] on a grid of 5 includes both ends.
        assert_eq!(band_strikes(100, 5, 500).unwrap(), vec![95, 100, 105]);
        // One atom less: 94.05..103.95 on a grid of 5 gives 95 and 100 only.
        assert_eq!(band_strikes(99, 5, 500).unwrap(), vec![95, 100]);
    }

    #[test]
    fn test_exact_cap() {
        assert_eq!(exact_cap(48 * USD, 1_000), Some(52_800_000));
        assert_eq!(exact_cap(3_800 * USD, 1_000), Some(4_180 * USD));
        assert_eq!(exact_cap(1, 1_000), None); // 1.1 atoms is not exact
    }

    #[test]
    fn test_week_series_shape() {
        let s = week_series(1, 1_794_603_600, 50_357_000, USD, 500, 1_000).unwrap();
        assert_eq!(s.len(), 10);
        assert_eq!(s.iter().filter(|a| a.kind == 1).count(), 5);
        assert!(s
            .iter()
            .filter(|a| a.kind == 0)
            .all(|a| a.cap * 10 == a.strike * 11));
        let x = week_series(0, 1_794_603_600, 3_968_150_000, 50 * USD, 500, 1_000).unwrap();
        assert_eq!(x.len(), 16);
    }

    #[test]
    fn test_listing_expiry_rules() {
        let cal = [1_000, 5_000, 9_000];
        // Default: the first expiry at least min_listing_s ahead.
        assert_eq!(listing_expiry(&cal, 1_000, 3_600, None).unwrap(), 5_000);
        assert_eq!(listing_expiry(&cal, 1_400, 3_600, None).unwrap(), 5_000);
        assert_eq!(listing_expiry(&cal, 1_401, 3_600, None).unwrap(), 9_000);
        assert!(listing_expiry(&cal, 9_000, 3_600, None).is_err());
        // Wanted: in the calendar and far enough, or refused before any send.
        assert_eq!(listing_expiry(&cal, 0, 3_600, Some(9_000)).unwrap(), 9_000);
        assert!(listing_expiry(&cal, 0, 3_600, Some(9_001)).is_err());
        assert!(listing_expiry(&cal, 2_000, 3_600, Some(5_000)).is_err());
    }

    #[test]
    fn test_fresh_print() {
        let p = PrintMsg {
            price: 1,
            conf: 0,
            expo: -3,
            publish_time: 1_000,
            prev_publish_time: 999,
        };
        assert!(fresh_print(&p, 1_300, 300, -3).is_ok());
        // Future prints: within the clock skew they pass, beyond it they are refused.
        assert!(fresh_print(&p, 1_000 - MAX_CLOCK_SKEW_S, 300, -3).is_ok());
        assert!(fresh_print(&p, 1_000 - MAX_CLOCK_SKEW_S - 1, 300, -3).is_err());
        assert!(fresh_print(&p, 1_301, 300, -3).is_err());
        assert!(fresh_print(&p, 1_000, 300, -5).is_err());
    }

    #[test]
    fn test_dry_run_dominates_execute() {
        assert!(!sends(false, false).unwrap());
        assert!(sends(false, true).unwrap());
        assert!(!sends(true, false).unwrap());
        assert!(sends(true, true).is_err());
    }

    #[test]
    fn test_usd() {
        assert_eq!(usd(52_800_000), "52.8");
        assert_eq!(usd(4_180 * USD), "4180");
        assert_eq!(usd(1), "0.000001");
    }
}
