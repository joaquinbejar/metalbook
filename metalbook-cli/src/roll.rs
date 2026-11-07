//! The weekly series roller (`docs/PLAN-phase2.md`, R1): `metalbook-admin roll`.
//!
//! One idempotent run lists, for each metal, the week shape of `list-week` (a put and a capped
//! call at every grid strike within ±5% of a fresh Pyth print) for the first calendar expiry at
//! least `--min-lead-s` ahead, creating only the series that do not exist yet. A launchd agent
//! runs it every 6 hours (`scripts/series-roll.sh`), so a later run adds the strikes that have
//! entered the band since (a top-up) and a run failing halfway is finished by the next one.
//!
//! The decisions are pure and tested here ([`target_expiry`], [`quiet_expiry`],
//! [`check_multiplier`], [`classify_print`], [`bounded`]); [`roll`] adds the reads and the
//! sends over [`Rpc`] and a [`LatestPrints`] source, so the tests drive the whole run on LiteSVM
//! with scripted prints.
//!
//! Rules, in the order a run applies them:
//! 1. **Quiet period**: nothing at all, before any Hermes request, within [`QUIET_S`] of any
//!    calendar expiry, so the keepers keep the whole Hermes budget around their window.
//! 2. **Multiplier**: refuses (an error, nothing sent) unless every metal's configured multiplier
//!    is `--multiplier-milli-oz`, so a later `set_config` cannot silently list other sizes.
//! 3. **Target expiry**: the first calendar expiry at least `max(--min-lead-s, min_listing_s)`
//!    ahead and not before `--min-expiry`; none is a skip for that metal (the calendar ran out:
//!    `metalbook-watch` raises `CalendarLow` and `NoNextSeries`).
//! 4. **Hermes**: one `/latest` request per metal, at most two a run, spaced
//!    [`HERMES_SPACING_S`], each taken from the shared limiter without waiting: no free slot is a
//!    skip (`hermes budget busy`). A print older than `--max-age-s` is a skip too (the market is
//!    closed from Friday to Sunday); any other read error stops that metal with nothing sent and
//!    makes the run fail.
//! 5. **Bounds**: at most `--max` new series a run and `--max-per-expiry` series per metal and
//!    expiry, counting those already listed. Reaching a bound stops and says so; the strikes
//!    nearest spot go first.

use std::time::Duration;

use metalbook_keeper::PrintMsg;
use solana_address::Address;
use solana_keypair::Keypair;
use solana_signer::Signer;

use crate::listing::{self, MAX_CLOCK_SKEW_S};
use crate::{create_series_ix, read_calendar, read_config, submit, CliError, Rpc, SeriesArgs};

/// Default lead: the target expiry is at least 3 days ahead.
pub const DEFAULT_MIN_LEAD_S: i64 = 259_200;
/// No run within this long of any calendar expiry, on either side (2 hours).
pub const QUIET_S: i64 = 7_200;
/// The only contract size the roller lists by default: 1 oz.
pub const DEFAULT_MULTIPLIER_MILLI_OZ: u32 = 1_000;
/// Default bound on new series in one run.
pub const DEFAULT_MAX: usize = 28;
/// Default bound on series listed per metal and expiry.
pub const DEFAULT_MAX_PER_EXPIRY: usize = 18;
/// Seconds between the run's two Hermes requests.
pub const HERMES_SPACING_S: u64 = 12;

/// The metals, in the order a run takes them.
pub const METALS: [u8; 2] = [0, 1];

/// The run's options (the command line's flags).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RollOptions {
    /// Least time from now to the target expiry, seconds.
    pub min_lead_s: i64,
    /// No target expiry before this one.
    pub min_expiry: Option<i64>,
    /// The contract size the run accepts, milli-ounces.
    pub multiplier_milli_oz: u32,
    /// Half-width of the strike band, basis points.
    pub band_bps: u16,
    /// Oldest Pyth print accepted, seconds.
    pub max_age_s: i64,
    /// Most new series in one run.
    pub max: usize,
    /// Most series listed per metal and expiry.
    pub max_per_expiry: usize,
    /// Send (`--execute`); otherwise print the plan only.
    pub execute: bool,
}

impl Default for RollOptions {
    fn default() -> Self {
        Self {
            min_lead_s: DEFAULT_MIN_LEAD_S,
            min_expiry: None,
            multiplier_milli_oz: DEFAULT_MULTIPLIER_MILLI_OZ,
            band_bps: listing::LISTING_BAND_BPS,
            max_age_s: listing::MAX_PRINT_AGE_S,
            max: DEFAULT_MAX,
            max_per_expiry: DEFAULT_MAX_PER_EXPIRY,
            execute: false,
        }
    }
}

/// The first expiry of `calendar` at least `max(min_lead_s, min_listing_s)` after `now` and not
/// before `min_expiry`.
#[must_use]
pub fn target_expiry(
    calendar: &[i64],
    now: i64,
    min_lead_s: i64,
    min_listing_s: u32,
    min_expiry: Option<i64>,
) -> Option<i64> {
    let lead = min_lead_s.max(i64::from(min_listing_s));
    let earliest = now.checked_add(lead)?.max(min_expiry.unwrap_or(i64::MIN));
    calendar.iter().copied().filter(|e| *e >= earliest).min()
}

/// The calendar expiry within `quiet_s` of `now` (on either side, edges included), if any.
#[must_use]
pub fn quiet_expiry(calendars: &[Vec<i64>], now: i64, quiet_s: i64) -> Option<i64> {
    calendars
        .iter()
        .flatten()
        .copied()
        .find(|e| now.abs_diff(*e) <= quiet_s.unsigned_abs())
}

/// Refuses unless every metal's configured multiplier is `wanted`.
///
/// # Errors
///
/// [`CliError::Usage`] naming the metal and both sizes.
pub fn check_multiplier(config: &[u32], wanted: u32) -> Result<(), CliError> {
    for (i, m) in config.iter().enumerate() {
        if *m != wanted {
            return Err(CliError::Usage(format!(
                "{}: the config multiplier is {m} milli-oz, the roller lists {wanted}; nothing \
                 read or sent (pass --multiplier-milli-oz to accept it)",
                metal_name(u8::try_from(i).unwrap_or(u8::MAX))
            )));
        }
    }
    Ok(())
}

/// What a Pyth print is good for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrintCheck {
    /// Fresh, with the config's exponent: list around it.
    Fresh,
    /// Older than the limit: the market is closed or the feed stalled. A skip.
    Stale(String),
    /// Anything else (another exponent, a print from the future): a failure.
    Bad(String),
}

/// Classifies `print` at `now` for a metal whose config pins `expo`.
#[must_use]
pub fn classify_print(print: &PrintMsg, now: i64, max_age_s: i64, expo: i32) -> PrintCheck {
    let future = print.publish_time > now.saturating_add(MAX_CLOCK_SKEW_S);
    let age = now.saturating_sub(print.publish_time);
    if print.expo == expo && !future && age > max_age_s {
        return PrintCheck::Stale(format!(
            "the latest Pyth print is {age} s old (limit {max_age_s} s): market closed or feed \
             stalled"
        ));
    }
    match listing::fresh_print(print, now, max_age_s, expo) {
        Ok(()) => PrintCheck::Fresh,
        Err(e) => PrintCheck::Bad(e.to_string()),
    }
}

/// How many of `missing` new series may be listed: at most `run_left` in the run and
/// `max_per_expiry − existing` at the expiry. Returns the count and, when it cuts, why.
#[must_use]
pub fn bounded(
    missing: usize,
    existing: usize,
    run_left: usize,
    max_per_expiry: usize,
) -> (usize, Option<String>) {
    let expiry_left = max_per_expiry.saturating_sub(existing);
    let allowed = run_left.min(expiry_left);
    if missing <= allowed {
        (missing, None)
    } else if expiry_left <= run_left {
        (
            allowed,
            Some(format!(
                "--max-per-expiry {max_per_expiry} reached ({existing} listed)"
            )),
        )
    } else {
        (allowed, Some("--max reached for this run".to_string()))
    }
}

/// A Pyth `/latest` read, or the shared budget refusing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrintRead {
    /// The latest print.
    Print(PrintMsg),
    /// No Hermes slot was free; nothing was sent.
    Busy,
}

/// Where a run gets its prints (Hermes behind the limiter in production, a script in tests).
pub trait LatestPrints {
    /// The latest print of `feed`.
    ///
    /// # Errors
    ///
    /// [`CliError`] for any failure other than a busy budget (a 429, a transport error, no
    /// print at all).
    fn latest(&mut self, feed: &[u8; 32]) -> Result<PrintRead, CliError>;
}

/// [`LatestPrints`] over a Hermes source behind the shared limiter in
/// [`metalbook_keeper::Admission::Try`] mode: a busy budget is [`PrintRead::Busy`], nothing sent.
pub struct HermesPrints<'a, H: metalbook_keeper::HttpGet>(
    pub &'a metalbook_keeper::HermesSource<H>,
);

impl<H: metalbook_keeper::HttpGet> LatestPrints for HermesPrints<'_, H> {
    fn latest(&mut self, feed: &[u8; 32]) -> Result<PrintRead, CliError> {
        match self.0.latest(feed) {
            Ok(Some(p)) => Ok(PrintRead::Print(p)),
            Ok(None) => Err(CliError::Rpc("Hermes has no latest print".into())),
            Err(metalbook_keeper::KeeperError::BudgetBusy) => Ok(PrintRead::Busy),
            Err(e) => Err(CliError::Rpc(e.to_string())),
        }
    }
}

/// What a run did for one metal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetalOutcome {
    /// No calendar expiry far enough ahead.
    NoExpiry,
    /// The shared Hermes budget had no free slot.
    Busy,
    /// The print was stale (market closed).
    Stale(String),
    /// Planned (and, with `--execute`, sent).
    Listed {
        /// The target expiry.
        expiry: i64,
        /// Series in the band.
        planned: usize,
        /// Series of this metal already listed at the expiry.
        existing: usize,
        /// New series listed (or, in a dry run, to list).
        new: Vec<Address>,
        /// Why the run stopped short of the band, if it did.
        bound: Option<String>,
    },
    /// A read or a send failed: nothing more for this metal in this run.
    Failed(String),
}

/// A whole run.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RollReport {
    /// The expiry that made it a quiet run, if it was one (then nothing else happened).
    pub quiet: Option<i64>,
    /// Per metal, in [`METALS`] order.
    pub metals: Vec<(u8, MetalOutcome)>,
    /// One line per decision and per transaction, for the log.
    pub lines: Vec<String>,
}

impl RollReport {
    /// Whether any metal failed (the run exits non-zero).
    #[must_use]
    pub fn failed(&self) -> bool {
        self.metals
            .iter()
            .any(|(_, o)| matches!(o, MetalOutcome::Failed(_)))
    }
}

/// `xau` or `xag`.
#[must_use]
pub fn metal_name(underlying: u8) -> &'static str {
    match underlying {
        0 => "xau",
        1 => "xag",
        _ => "unknown",
    }
}

/// One run: see the module documentation. `existing(underlying, expiry)` counts the program's
/// series of a metal at an expiry (`getProgramAccounts` in production); `sleep` spaces the two
/// Hermes requests.
///
/// # Errors
///
/// [`CliError`] when the config or a calendar cannot be read, or the multiplier check refuses:
/// nothing is sent then. Per-metal failures are in the report instead.
#[allow(clippy::too_many_arguments)]
pub fn roll<R: Rpc, P: LatestPrints>(
    rpc: &mut R,
    prints: &mut P,
    program_id: &Address,
    signer: &Keypair,
    now: i64,
    opts: &RollOptions,
    existing: &mut dyn FnMut(&mut R, u8, i64) -> Result<usize, CliError>,
    sleep: &mut dyn FnMut(Duration),
) -> Result<RollReport, CliError> {
    let mut report = RollReport::default();
    let config = read_config(rpc, program_id)?;
    let mut calendars = Vec::new();
    for metal in METALS {
        let c = read_calendar(rpc, program_id, metal)?;
        calendars.push(
            c.expiries
                .iter()
                .take(usize::from(c.count))
                .copied()
                .collect::<Vec<i64>>(),
        );
    }
    if let Some(e) = quiet_expiry(&calendars, now, QUIET_S) {
        report.quiet = Some(e);
        report.lines.push(format!(
            "quiet period: now {now} is within {QUIET_S} s of expiry {e}; nothing read or sent"
        ));
        return Ok(report);
    }
    check_multiplier(&config.multiplier, opts.multiplier_milli_oz)?;
    let mut run_left = opts.max;
    let mut asked_hermes = false;
    for (metal, calendar) in METALS.iter().copied().zip(calendars.iter()) {
        let name = metal_name(metal);
        let index = usize::from(metal);
        let Some(expiry) = target_expiry(
            calendar,
            now,
            opts.min_lead_s,
            config.min_listing_s,
            opts.min_expiry,
        ) else {
            report.lines.push(format!(
                "{name}: no calendar expiry at least {} s ahead{}; nothing listed",
                opts.min_lead_s,
                opts.min_expiry
                    .map(|m| format!(" and from {m}"))
                    .unwrap_or_default()
            ));
            report.metals.push((metal, MetalOutcome::NoExpiry));
            continue;
        };
        let (Some(feed), Some(expo), Some(tick)) = (
            config.feed_id.get(index),
            config.expo.get(index),
            config.strike_tick.get(index),
        ) else {
            return Err(CliError::Usage("unknown underlying".into()));
        };
        if asked_hermes {
            sleep(Duration::from_secs(HERMES_SPACING_S));
        }
        asked_hermes = true;
        let print = match prints.latest(feed) {
            Ok(PrintRead::Print(p)) => p,
            Ok(PrintRead::Busy) => {
                report.lines.push(format!(
                    "{name}: hermes budget busy; skipped, next run retries"
                ));
                report.metals.push((metal, MetalOutcome::Busy));
                continue;
            }
            Err(e) => {
                report
                    .lines
                    .push(format!("{name}: Hermes read failed: {e}; nothing sent"));
                report
                    .metals
                    .push((metal, MetalOutcome::Failed(e.to_string())));
                continue;
            }
        };
        match classify_print(&print, now, opts.max_age_s, i32::from(*expo)) {
            PrintCheck::Fresh => {}
            PrintCheck::Stale(why) => {
                report.lines.push(format!("{name}: {why}; skipped"));
                report.metals.push((metal, MetalOutcome::Stale(why)));
                continue;
            }
            PrintCheck::Bad(why) => {
                report.lines.push(format!("{name}: {why}; nothing sent"));
                report.metals.push((metal, MetalOutcome::Failed(why)));
                continue;
            }
        }
        let outcome = list_metal(
            rpc,
            program_id,
            signer,
            now,
            opts,
            &config,
            metal,
            expiry,
            &print,
            *tick,
            &mut run_left,
            existing,
            &mut report.lines,
        );
        report.metals.push((metal, outcome));
    }
    Ok(report)
}

/// Plans and (with `--execute`) sends one metal's missing series.
#[allow(clippy::too_many_arguments)]
fn list_metal<R: Rpc>(
    rpc: &mut R,
    program_id: &Address,
    signer: &Keypair,
    now: i64,
    opts: &RollOptions,
    config: &metalbook_sdk::core::accounts::Config,
    metal: u8,
    expiry: i64,
    print: &PrintMsg,
    tick: u64,
    run_left: &mut usize,
    existing: &mut dyn FnMut(&mut R, u8, i64) -> Result<usize, CliError>,
    lines: &mut Vec<String>,
) -> MetalOutcome {
    let name = metal_name(metal);
    let fail = |lines: &mut Vec<String>, e: CliError| {
        lines.push(format!("{name}: {e}; stopped"));
        MetalOutcome::Failed(e.to_string())
    };
    // The program refuses an expiry closer than `min_listing_s`; never try one.
    if expiry < now.saturating_add(i64::from(config.min_listing_s)) {
        return fail(
            lines,
            CliError::Usage(format!("expiry {expiry} is closer than min_listing_s")),
        );
    }
    let spot = match listing::spot_atoms(print.price, print.expo) {
        Ok(s) => s,
        Err(e) => return fail(lines, e),
    };
    let cap_bps = config.cap_bps.first().copied().unwrap_or(0);
    let plan = match listing::week_series(metal, expiry, spot, tick, opts.band_bps, cap_bps) {
        Ok(p) => p,
        Err(e) => return fail(lines, e),
    };
    let mut missing: Vec<(SeriesArgs, Address, solana_instruction::Instruction)> = Vec::new();
    for args in &plan {
        let (ix, series) = match create_series_ix(rpc, program_id, &signer.pubkey(), args) {
            Ok(x) => x,
            Err(e) => return fail(lines, e),
        };
        match rpc.account(&series) {
            Ok(Some(_)) => {}
            Ok(None) => missing.push((*args, series, ix)),
            Err(e) => return fail(lines, e),
        }
    }
    let listed = match existing(rpc, metal, expiry) {
        Ok(n) => n,
        Err(e) => return fail(lines, e),
    };
    // Nearest spot first, so a bound keeps the strikes a maker quotes.
    missing.sort_by_key(|(a, _, _)| (a.strike.abs_diff(spot), a.kind));
    let (allowed, bound) = bounded(missing.len(), listed, *run_left, opts.max_per_expiry);
    missing.truncate(allowed);
    lines.push(format!(
        "{name}: spot {} USD ({} s old), expiry {expiry}: {} in the band, {listed} listed at the \
         expiry, {} new{}",
        listing::usd(spot),
        now.saturating_sub(print.publish_time),
        plan.len(),
        missing.len(),
        bound
            .as_ref()
            .map(|b| format!(" (bound: {b})"))
            .unwrap_or_default()
    ));
    let mut new = Vec::new();
    for (args, series, ix) in missing {
        let what = if args.kind == 0 {
            format!(
                "call {} cap {}",
                listing::usd(args.strike),
                listing::usd(args.cap)
            )
        } else {
            format!("put {}", listing::usd(args.strike))
        };
        if opts.execute {
            match submit(rpc, &[ix], signer) {
                Ok(sig) => lines.push(format!("{name}: {series} {what} confirmed {sig}")),
                Err(e) => {
                    lines.push(format!("{name}: {series} {what} failed: {e}; stopped"));
                    return MetalOutcome::Failed(e.to_string());
                }
            }
        } else {
            lines.push(format!("{name}: {series} {what} new (dry run)"));
        }
        *run_left = run_left.saturating_sub(1);
        new.push(series);
    }
    if !opts.execute {
        lines.push(format!("{name}: dry run, nothing sent (add --execute)"));
    }
    MetalOutcome::Listed {
        expiry,
        planned: plan.len(),
        existing: listed,
        new,
        bound,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::disallowed_macros)]
mod tests {
    use super::*;

    // The devnet calendar: Oct 30, Nov 6, 13, 20 (no Nov 27), Dec 4, 11, 18, 21:00Z (20:00Z on
    // Oct 30, before the US clock change).
    const OCT30: i64 = 1_793_390_400;
    const NOV6: i64 = 1_793_998_800;
    const NOV13: i64 = 1_794_603_600;
    const NOV20: i64 = 1_795_208_400;
    const DEC4: i64 = 1_796_418_000;
    const DEC11: i64 = 1_797_022_800;
    const DEC18: i64 = 1_797_627_600;
    const DAY: i64 = 86_400;

    fn cal() -> Vec<i64> {
        vec![OCT30, NOV6, NOV13, NOV20, DEC4, DEC11, DEC18]
    }

    #[test]
    fn test_target_expiry_across_the_nov_27_gap() {
        // Nov 24: Nov 27 is missing, so the next expiry 3 days ahead is Dec 4.
        let nov24 = NOV20 + 4 * DAY;
        assert_eq!(
            target_expiry(&cal(), nov24, DEFAULT_MIN_LEAD_S, 3_600, None),
            Some(DEC4)
        );
        // Nov 21: Dec 4 too (nothing between).
        assert_eq!(
            target_expiry(&cal(), NOV20 + DAY, DEFAULT_MIN_LEAD_S, 3_600, None),
            Some(DEC4)
        );
    }

    #[test]
    fn test_target_expiry_lead_boundary() {
        // Exactly 3 days before Nov 20: Nov 20 itself; one second later: Dec 4.
        let edge = NOV20 - DEFAULT_MIN_LEAD_S;
        assert_eq!(
            target_expiry(&cal(), edge, DEFAULT_MIN_LEAD_S, 3_600, None),
            Some(NOV20)
        );
        assert_eq!(
            target_expiry(&cal(), edge + 1, DEFAULT_MIN_LEAD_S, 3_600, None),
            Some(DEC4)
        );
    }

    #[test]
    fn test_target_expiry_past_the_end_and_min_expiry() {
        assert_eq!(
            target_expiry(&cal(), DEC18 - DAY, DEFAULT_MIN_LEAD_S, 3_600, None),
            None
        );
        // Nov 7, first install: Nov 13 is 6 days ahead, but --min-expiry Nov 20 skips it.
        let nov7 = NOV6 + DAY;
        assert_eq!(
            target_expiry(&cal(), nov7, DEFAULT_MIN_LEAD_S, 3_600, None),
            Some(NOV13)
        );
        assert_eq!(
            target_expiry(&cal(), nov7, DEFAULT_MIN_LEAD_S, 3_600, Some(NOV20)),
            Some(NOV20)
        );
    }

    #[test]
    fn test_min_listing_s_is_never_undercut() {
        // A zero lead still respects the program's min_listing_s.
        assert_eq!(
            target_expiry(&cal(), NOV20 - 3_599, 0, 3_600, None),
            Some(DEC4)
        );
        assert_eq!(
            target_expiry(&cal(), NOV20 - 3_600, 0, 3_600, None),
            Some(NOV20)
        );
    }

    #[test]
    fn test_quiet_period_edges() {
        let cals = vec![cal(), vec![NOV13]];
        assert_eq!(quiet_expiry(&cals, NOV13 - QUIET_S, QUIET_S), Some(NOV13));
        assert_eq!(quiet_expiry(&cals, NOV13 - QUIET_S - 1, QUIET_S), None);
        assert_eq!(quiet_expiry(&cals, NOV13 + QUIET_S, QUIET_S), Some(NOV13));
        assert_eq!(quiet_expiry(&cals, NOV13 + QUIET_S + 1, QUIET_S), None);
        assert_eq!(quiet_expiry(&cals, NOV13, QUIET_S), Some(NOV13));
    }

    #[test]
    fn test_multiplier_mismatch_refuses() {
        assert!(check_multiplier(&[1_000, 1_000], 1_000).is_ok());
        let e = check_multiplier(&[1_000, 50_000], 1_000).unwrap_err();
        assert!(e.to_string().contains("xag"), "{e}");
    }

    #[test]
    fn test_classify_print() {
        let p = PrintMsg {
            price: 1,
            conf: 0,
            expo: -3,
            publish_time: 1_000,
            prev_publish_time: 999,
        };
        assert_eq!(classify_print(&p, 1_300, 300, -3), PrintCheck::Fresh);
        assert!(matches!(
            classify_print(&p, 1_301, 300, -3),
            PrintCheck::Stale(_)
        ));
        // A weekend: days old is still only a skip.
        assert!(matches!(
            classify_print(&p, 1_000 + 2 * DAY, 300, -3),
            PrintCheck::Stale(_)
        ));
        assert!(matches!(
            classify_print(&p, 1_300, 300, -5),
            PrintCheck::Bad(_)
        ));
        assert!(matches!(
            classify_print(&p, 1_000 - MAX_CLOCK_SKEW_S - 1, 300, -3),
            PrintCheck::Bad(_)
        ));
    }

    #[test]
    fn test_bounds() {
        assert_eq!(bounded(10, 0, 28, 18), (10, None));
        // Per expiry: 14 listed, 18 allowed, 4 more.
        let (n, why) = bounded(10, 14, 28, 18);
        assert_eq!(n, 4);
        assert!(why.unwrap().contains("--max-per-expiry"));
        // Per run: 3 left in the run.
        let (n, why) = bounded(10, 0, 3, 18);
        assert_eq!(n, 3);
        assert!(why.unwrap().contains("--max"));
        assert_eq!(bounded(5, 20, 28, 18).0, 0);
    }
}
