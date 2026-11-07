//! Keeper heartbeats and chain-only alerts (`docs/PLAN-improvements.md`, P2).
//!
//! The watcher (`metalbook-watch`) reads chain state and the keepers' remote heartbeats, never a
//! keeper's memory, so it alerts when every keeper is down. It cannot see when an off-chain
//! Pyth print first existed (an absent print leaves no chain evidence), so lag is measured
//! against the **schedule**: a window slot still empty `slot_tolerance_s + grace` after its
//! boundary is overdue, and its oracle availability is `unknown`. Everything here is pure, so
//! the controls run on a scripted clock.
//!
//! Transport (coordinator decision, bus 44325): the ACS channel `metalbook-keepers`. Keepers post
//! a heartbeat on any state change and otherwise every 10 minutes; alerts go to the channel and
//! to the SOLANA-PROJECTS coordinator by DM, through the `ai-crew-sync client` CLI and its local
//! profile (no credential is read or printed here).

use std::collections::BTreeMap;

use metalbook_keeper::SeriesView;
use metalbook_sdk::core::SeriesState;
use solana_address::Address;

/// Grace after a slot's tolerance before it is overdue, seconds.
pub const SLOT_GRACE_S: i64 = 120;
/// How long after `window_end + D` an unsettled series is overdue, seconds.
pub const SETTLE_GRACE_S: i64 = 600;
/// A keeper with no heartbeat for this long is reported missing, seconds: three of the
/// 10-minute idle heartbeats.
pub const HEARTBEAT_TIMEOUT_S: i64 = 1_800;
/// Idle heartbeat period of a keeper, seconds.
pub const HEARTBEAT_PERIOD_S: i64 = 600;
/// Default payer balance alarm, lamports (0.5 SOL).
pub const LOW_BALANCE_LAMPORTS: u64 = 500_000_000;
/// The heartbeat channel.
pub const CHANNEL: &str = "metalbook-keepers";

/// `NoNextSeries`: a metal needs a live series expiring at least this far ahead, seconds.
pub const NEXT_SERIES_AHEAD_S: i64 = 86_400;
/// `CalendarLow`: fewer future calendar expiries than this.
pub const CALENDAR_MIN_FUTURE: usize = 2;

/// Prefix of a heartbeat message body.
const HEARTBEAT_PREFIX: &str = "heartbeat keeper=";

/// What one alert is about. Each alert names its runbook.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Alert {
    /// Window slots still empty past their boundary, tolerance and grace.
    SlotsOverdue {
        /// The series.
        series: Address,
        /// How many slots are overdue.
        slots: u32,
        /// The earliest overdue slot's boundary, Unix seconds.
        since: i64,
    },
    /// Not settled by `window_end + D + SETTLE_GRACE_S`.
    SettleOverdue {
        /// The series.
        series: Address,
        /// When it became overdue.
        due: i64,
    },
    /// Past `hard_deadline + D` and neither settled nor voided.
    VoidPending {
        /// The series.
        series: Address,
        /// When it became voidable.
        due: i64,
    },
    /// A keeper payer under the balance alarm.
    LowBalance {
        /// The payer.
        payer: Address,
        /// Its balance, lamports.
        lamports: u64,
    },
    /// Leftover encoded VAAs of a payer (unchanged for 300 blocks).
    Leftovers {
        /// The payer.
        payer: Address,
        /// How many.
        count: usize,
    },
    /// A keeper's heartbeat is older than [`HEARTBEAT_TIMEOUT_S`] (or never seen).
    KeeperSilent {
        /// The keeper's name.
        keeper: String,
        /// Its last heartbeat, Unix seconds.
        last: Option<i64>,
    },
    /// The heartbeat channel could not be read: keeper liveness is unknown, not dead.
    HeartbeatUnknown,
    /// While the market is open, a metal has no live series of the roller's size expiring at
    /// least [`NEXT_SERIES_AHEAD_S`] ahead (`docs/PLAN-phase2.md`, R1).
    NoNextSeries {
        /// The metal (`Underlying` discriminant).
        underlying: u8,
    },
    /// A metal's calendar has fewer than [`CALENDAR_MIN_FUTURE`] future expiries: the roller
    /// runs out unless the admin extends it.
    CalendarLow {
        /// The metal.
        underlying: u8,
        /// Its future expiries.
        future: usize,
    },
}

/// `xau` or `xag`, for alert text.
fn metal(underlying: u8) -> &'static str {
    match underlying {
        0 => "xau",
        1 => "xag",
        _ => "unknown",
    }
}

impl Alert {
    /// A stable key: an alert is announced when its key appears and resolved when it goes.
    #[must_use]
    pub fn key(&self) -> String {
        match self {
            Self::SlotsOverdue { series, .. } => format!("slots:{series}"),
            Self::SettleOverdue { series, .. } => format!("settle:{series}"),
            Self::VoidPending { series, .. } => format!("void:{series}"),
            Self::LowBalance { payer, .. } => format!("balance:{payer}"),
            Self::Leftovers { payer, .. } => format!("leftovers:{payer}"),
            Self::KeeperSilent { keeper, .. } => format!("silent:{keeper}"),
            Self::HeartbeatUnknown => "heartbeat-unknown".into(),
            Self::NoNextSeries { underlying } => format!("next-series:{}", metal(*underlying)),
            Self::CalendarLow { underlying, .. } => format!("calendar:{}", metal(*underlying)),
        }
    }

    /// The runbook that applies (`docs/runbooks/`).
    #[must_use]
    pub fn runbook(&self) -> &'static str {
        match self {
            Self::SlotsOverdue { .. } | Self::KeeperSilent { .. } => {
                "docs/runbooks/keeper-restart.md"
            }
            Self::SettleOverdue { .. } => "docs/runbooks/stuck-settlement.md",
            Self::VoidPending { .. } => "docs/runbooks/void.md",
            Self::LowBalance { .. } => "docs/runbooks/low-balance.md",
            Self::Leftovers { .. } => "docs/runbooks/orphaned-vaas.md",
            Self::HeartbeatUnknown => "docs/runbooks/keeper-restart.md (failure modes)",
            Self::NoNextSeries { .. } | Self::CalendarLow { .. } => "docs/runbooks/series-roll.md",
        }
    }

    /// One line for the channel and the DM.
    #[must_use]
    pub fn text(&self) -> String {
        let what = match self {
            Self::SlotsOverdue {
                series,
                slots,
                since,
            } => format!(
                "{series}: {slots} window slot(s) empty past tolerance and grace since {since} \
                 (oracle availability unknown)"
            ),
            Self::SettleOverdue { series, due } => {
                format!("{series}: not settled, overdue since {due}")
            }
            Self::VoidPending { series, due } => {
                format!("{series}: past hard_deadline + D since {due}, neither settled nor voided")
            }
            Self::LowBalance { payer, lamports } => {
                format!("payer {payer}: {lamports} lamports, under the alarm")
            }
            Self::Leftovers { payer, count } => {
                format!("payer {payer}: {count} leftover encoded VAA(s)")
            }
            Self::KeeperSilent { keeper, last } => match last {
                Some(t) => format!("keeper {keeper}: no heartbeat since {t}"),
                None => format!("keeper {keeper}: no heartbeat seen"),
            },
            Self::HeartbeatUnknown => {
                "heartbeat channel unreadable: keeper liveness unknown".into()
            }
            Self::NoNextSeries { underlying } => format!(
                "{}: no live series expiring at least {NEXT_SERIES_AHEAD_S} s ahead while the \
                 market is open",
                metal(*underlying)
            ),
            Self::CalendarLow { underlying, future } => format!(
                "{}: {future} future calendar expiry(ies), under {CALENDAR_MIN_FUTURE}",
                metal(*underlying)
            ),
        };
        format!("ALERT {what}. Runbook: {}", self.runbook())
    }
}

/// The schedule alerts of one series at `now`.
#[must_use]
pub fn series_alerts(series: &Address, view: &SeriesView, now: i64) -> Vec<Alert> {
    if matches!(view.state, SeriesState::Settled | SeriesState::Voided) {
        return Vec::new();
    }
    let p = view.params;
    let mut out = Vec::new();
    let window_s = i64::from(p.window_s());
    let slot_s = i64::from(p.slot_s());
    let tolerance = i64::from(p.slot_tolerance_s());
    let d = i64::from(p.challenge_delay_s());
    let window_start = view.expiry.saturating_sub(window_s);
    let filled = view.candidates.parts().filled_slots;
    let mut overdue: u32 = 0;
    let mut since: Option<i64> = None;
    for i in 0..p.slots() {
        if filled.checked_shr(i).unwrap_or(0) & 1 == 1 {
            continue;
        }
        let boundary = window_start.saturating_add(i64::from(i).saturating_mul(slot_s));
        if now
            >= boundary
                .saturating_add(tolerance)
                .saturating_add(SLOT_GRACE_S)
        {
            overdue = overdue.saturating_add(1);
            since = Some(since.map_or(boundary, |s| s.min(boundary)));
        }
    }
    if let Some(since) = since {
        out.push(Alert::SlotsOverdue {
            series: *series,
            slots: overdue,
            since,
        });
    }
    let settle_due = view.expiry.saturating_add(d).saturating_add(SETTLE_GRACE_S);
    let void_due = view
        .expiry
        .saturating_add(i64::from(p.hard_deadline_s()))
        .saturating_add(d);
    if now >= void_due {
        out.push(Alert::VoidPending {
            series: *series,
            due: void_due,
        });
    } else if now >= settle_due {
        out.push(Alert::SettleOverdue {
            series: *series,
            due: settle_due,
        });
    }
    out
}

/// A listed series as the listing alerts see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Listed {
    /// The metal.
    pub underlying: u8,
    /// Expiry, Unix seconds.
    pub expiry: i64,
    /// Multiplier, milli-ounces.
    pub multiplier: u32,
    /// Still active (not expired, settled or voided).
    pub active: bool,
}

/// The listing alerts at `now`: `CalendarLow` per metal with fewer than [`CALENDAR_MIN_FUTURE`]
/// future expiries in `calendars` (indexed by metal), and, only while `market_open`,
/// `NoNextSeries` per metal with no active `multiplier` series expiring at least
/// [`NEXT_SERIES_AHEAD_S`] ahead.
#[must_use]
pub fn listing_alerts(
    listed: &[Listed],
    calendars: &[Vec<i64>],
    multiplier: u32,
    now: i64,
    market_open: bool,
) -> Vec<Alert> {
    let mut out = Vec::new();
    for (i, calendar) in calendars.iter().enumerate() {
        let Ok(underlying) = u8::try_from(i) else {
            continue;
        };
        let future = calendar.iter().filter(|e| **e > now).count();
        if future < CALENDAR_MIN_FUTURE {
            out.push(Alert::CalendarLow { underlying, future });
        }
        let ahead = now.saturating_add(NEXT_SERIES_AHEAD_S);
        let live = listed.iter().any(|s| {
            s.underlying == underlying
                && s.multiplier == multiplier
                && s.active
                && s.expiry >= ahead
        });
        if market_open && !live {
            out.push(Alert::NoNextSeries { underlying });
        }
    }
    out
}

/// Days since 1970-01-01 of a proleptic Gregorian date (H. Hinnant's `days_from_civil`).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 {
        year.saturating_sub(1)
    } else {
        year
    };
    let era = y.div_euclid(400);
    let yoe = y.saturating_sub(era.saturating_mul(400));
    let m = if month > 2 {
        month.saturating_sub(3)
    } else {
        month.saturating_add(9)
    };
    let doy = m
        .saturating_mul(153)
        .saturating_add(2)
        .checked_div(5)
        .unwrap_or(0)
        .saturating_add(day)
        .saturating_sub(1);
    let doe = yoe
        .saturating_mul(365)
        .saturating_add(yoe.checked_div(4).unwrap_or(0))
        .saturating_sub(yoe.checked_div(100).unwrap_or(0))
        .saturating_add(doy);
    era.saturating_mul(146_097)
        .saturating_add(doe)
        .saturating_sub(719_468)
}

/// Day of the week of a day number, 0 for Sunday (1970-01-01 was a Thursday).
fn weekday(days: i64) -> i64 {
    days.saturating_add(4).rem_euclid(7)
}

/// The `n`-th Sunday (1-based) of `month` in `year`, as a day number.
fn nth_sunday(year: i64, month: i64, n: i64) -> i64 {
    let first = days_from_civil(year, month, 1);
    let to_sunday = 7_i64.saturating_sub(weekday(first)).rem_euclid(7);
    first
        .saturating_add(to_sunday)
        .saturating_add(n.saturating_sub(1).saturating_mul(7))
}

/// Whether New York is on daylight time at `now` (US rule since 2007: from the second Sunday of
/// March at 2:00 EST, 07:00Z, to the first Sunday of November at 2:00 EDT, 06:00Z).
fn new_york_dst(now: i64) -> bool {
    let days = now.div_euclid(86_400);
    // The year of `days`, from an estimate corrected both ways.
    let mut year = days.checked_div(365).unwrap_or(0).saturating_add(1970);
    while days_from_civil(year, 1, 1) > days {
        year = year.saturating_sub(1);
    }
    while days_from_civil(year.saturating_add(1), 1, 1) <= days {
        year = year.saturating_add(1);
    }
    let start = nth_sunday(year, 3, 2)
        .saturating_mul(86_400)
        .saturating_add(7 * 3_600);
    let end = nth_sunday(year, 11, 1)
        .saturating_mul(86_400)
        .saturating_add(6 * 3_600);
    now >= start && now < end
}

/// Whether the metals trade at `now` on Pyth's schedule (`README.md`, Market Hours): New York
/// time, Sunday 18:00 to Friday 17:00, with a daily pause from 17:00 to 18:00. Holidays are not
/// known here; on a holiday the `NoNextSeries` alert can fire although the market is closed.
#[must_use]
pub fn metals_market_open(now: i64) -> bool {
    let offset: i64 = if new_york_dst(now) { -4 } else { -5 };
    let local = now.saturating_add(offset.saturating_mul(3_600));
    let days = local.div_euclid(86_400);
    let tod = local.rem_euclid(86_400);
    let (pause_start, pause_end) = (17 * 3_600, 18 * 3_600);
    match weekday(days) {
        0 => tod >= pause_end,
        1..=4 => !(pause_start..pause_end).contains(&tod),
        5 => tod < pause_start,
        _ => false,
    }
}

/// A heartbeat message body: `heartbeat keeper=<name> <fields>`.
#[must_use]
pub fn heartbeat_body(keeper: &str, fields: &str) -> String {
    format!("{HEARTBEAT_PREFIX}{keeper} {fields}")
}

/// The keeper a heartbeat body is from, if it is one.
#[must_use]
pub fn heartbeat_keeper(body: &str) -> Option<&str> {
    body.strip_prefix(HEARTBEAT_PREFIX)?
        .split_whitespace()
        .next()
        .filter(|k| !k.is_empty())
}

/// Liveness alerts: `heartbeats` maps each keeper to its latest heartbeat time, or is `None`
/// when the channel could not be read.
#[must_use]
pub fn liveness_alerts(
    expected: &[String],
    heartbeats: Option<&BTreeMap<String, i64>>,
    now: i64,
) -> Vec<Alert> {
    let Some(seen) = heartbeats else {
        return vec![Alert::HeartbeatUnknown];
    };
    expected
        .iter()
        .filter_map(|k| {
            let last = seen.get(k).copied();
            let stale = last.is_none_or(|t| now.saturating_sub(t) > HEARTBEAT_TIMEOUT_S);
            stale.then(|| Alert::KeeperSilent {
                keeper: k.clone(),
                last,
            })
        })
        .collect()
}

/// Whether a keeper should post a heartbeat: its state digest changed, or the idle period
/// passed since the last one.
#[must_use]
pub fn heartbeat_due(last: Option<(i64, u64)>, digest: u64, now: i64) -> bool {
    match last {
        None => true,
        Some((at, previous)) => previous != digest || now.saturating_sub(at) >= HEARTBEAT_PERIOD_S,
    }
}

/// Announces alerts and their resolution, with delivery tracked per destination.
///
/// - An alert counts as announced to a destination only once its send succeeded; failed sends
///   are retried on every step until they succeed.
/// - A key resolves only when it was **observed** absent: a key matching an `unobserved`
///   prefix of the step (its balance, scan or heartbeat read failed) is kept, never resolved
///   on a failed observation.
/// - A resolution notice is retried until sent.
#[derive(Debug, Clone, Default)]
pub struct Announcer {
    /// Active keys: the last alert text, and whether it reached the channel and the DM.
    active: BTreeMap<String, Delivery>,
    /// Resolved keys whose notice has not been sent yet.
    resolving: std::collections::BTreeSet<String>,
}

/// One active alert's delivery state.
#[derive(Debug, Clone, Default)]
struct Delivery {
    /// The alert's last observed text.
    text: String,
    /// Whether the current condition is unobserved this step (shown as last known).
    unobserved: bool,
    channel: bool,
    direct: bool,
}

impl Announcer {
    /// A fresh announcer.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The active keys.
    #[must_use]
    pub fn active(&self) -> Vec<String> {
        self.active.keys().cloned().collect()
    }

    /// One step: announce `current`, resolve what was observed gone. Returns what it sent
    /// and what failed, one line each, for the log.
    pub fn step<N: Notify>(
        &mut self,
        notify: &N,
        channel: &str,
        direct: &str,
        current: &[Alert],
        unobserved: &[String],
    ) -> Vec<String> {
        let mut log = Vec::new();
        let now_keys: std::collections::BTreeSet<String> = current.iter().map(Alert::key).collect();
        // Resolve: previously active, now absent, and observed.
        let gone: Vec<String> = self
            .active
            .keys()
            .filter(|k| !now_keys.contains(*k))
            .filter(|k| !unobserved.iter().any(|p| k.starts_with(p.as_str())))
            .cloned()
            .collect();
        for k in gone {
            self.active.remove(&k);
            self.resolving.insert(k);
        }
        // A key that came back before its resolution was sent is simply active again.
        self.resolving.retain(|k| !now_keys.contains(k));
        let pending: Vec<String> = self.resolving.iter().cloned().collect();
        for k in pending {
            let text = format!("RESOLVED {k}");
            match notify.send(&Target::Channel(channel.into()), &text) {
                Ok(()) => {
                    self.resolving.remove(&k);
                    log.push(text);
                }
                Err(e) => log.push(format!("resolution of {k} not sent (will retry): {e}")),
            }
        }
        // Refresh the active set from this step's alerts; keys kept only because they are
        // unobserved keep their last text, marked as last known.
        for alert in current {
            let entry = self.active.entry(alert.key()).or_default();
            entry.text = alert.text();
            entry.unobserved = false;
        }
        for (k, entry) in &mut self.active {
            if !now_keys.contains(k) {
                entry.unobserved = true;
            }
        }
        // Announce: every active key, every destination not yet delivered.
        for entry in self.active.values_mut() {
            let text = if entry.unobserved {
                format!("{} (last known; currently unobserved)", entry.text)
            } else {
                entry.text.clone()
            };
            if !entry.channel {
                match notify.send(&Target::Channel(channel.into()), &text) {
                    Ok(()) => {
                        entry.channel = true;
                        log.push(text.clone());
                    }
                    Err(e) => log.push(format!("alert to the channel not sent (will retry): {e}")),
                }
            }
            if !entry.direct {
                match notify.send(&Target::Direct(direct.into()), &text) {
                    Ok(()) => entry.direct = true,
                    Err(e) => log.push(format!("alert by DM not sent (will retry): {e}")),
                }
            }
        }
        log
    }
}

/// Where a message goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// The heartbeat channel.
    Channel(String),
    /// A direct message to this address (`agent/session`).
    Direct(String),
}

/// Sends messages to the bus.
pub trait Notify {
    /// Sends `body` to `target`.
    ///
    /// # Errors
    ///
    /// A description of the failure (never a credential).
    fn send(&self, target: &Target, body: &str) -> Result<(), String>;

    /// The messages of the channel, newest last, as (created_at Unix seconds, body).
    ///
    /// # Errors
    ///
    /// A description of the failure.
    fn read_channel(&self, channel: &str) -> Result<Vec<(i64, String)>, String>;
}

/// [`Notify`] through the `ai-crew-sync client` CLI and its local profile or environment
/// (`BUS_URL`, `BUS_TOKEN`); this process never reads or prints the credential.
#[derive(Debug, Clone)]
pub struct AcsCli {
    /// The CLI binary.
    pub program: String,
    /// The `--session` working context.
    pub session: String,
}

impl Notify for AcsCli {
    fn send(&self, target: &Target, body: &str) -> Result<(), String> {
        let (flag, value) = match target {
            Target::Channel(c) => ("--channel", c.as_str()),
            Target::Direct(to) => ("--to", to.as_str()),
        };
        let out = std::process::Command::new(&self.program)
            .args(["client", "--session", &self.session, "send", flag, value])
            .args(["--body", body])
            .output()
            .map_err(|e| format!("{}: {e}", self.program))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(format!("{} exited with {}", self.program, out.status))
        }
    }

    fn read_channel(&self, channel: &str) -> Result<Vec<(i64, String)>, String> {
        let out = std::process::Command::new(&self.program)
            .args(["client", "--session", &self.session, "--json", "read"])
            .args(["--scope", channel, "--history", "--limit", "200"])
            .output()
            .map_err(|e| format!("{}: {e}", self.program))?;
        if !out.status.success() {
            return Err(format!("{} exited with {}", self.program, out.status));
        }
        parse_messages(&out.stdout)
    }
}

/// The messages of a `client read --json` answer as (created_at, body).
///
/// # Errors
///
/// A description of what is malformed.
pub fn parse_messages(json: &[u8]) -> Result<Vec<(i64, String)>, String> {
    let v: serde_json::Value =
        serde_json::from_slice(json).map_err(|_| "malformed read answer".to_string())?;
    let messages = v
        .get("messages")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "read answer without messages".to_string())?;
    let mut out = Vec::new();
    for m in messages {
        let (Some(at), Some(body)) = (
            m.get("created_at").and_then(serde_json::Value::as_str),
            m.get("body").and_then(serde_json::Value::as_str),
        ) else {
            continue;
        };
        if let Some(t) = parse_rfc3339_utc(at) {
            out.push((t, body.to_string()));
        }
    }
    Ok(out)
}

/// The latest heartbeat time of each keeper among `messages`.
#[must_use]
pub fn latest_heartbeats(messages: &[(i64, String)]) -> BTreeMap<String, i64> {
    let mut out: BTreeMap<String, i64> = BTreeMap::new();
    for (at, body) in messages {
        if let Some(k) = heartbeat_keeper(body) {
            let e = out.entry(k.to_string()).or_insert(*at);
            *e = (*e).max(*at);
        }
    }
    out
}

/// `YYYY-MM-DDTHH:MM:SSZ` as Unix seconds (the bus's `created_at`); `None` otherwise.
#[must_use]
pub fn parse_rfc3339_utc(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() != 20 || b.get(19) != Some(&b'Z') {
        return None;
    }
    let num = |from: usize, to: usize| -> Option<i64> { s.get(from..to)?.parse().ok() };
    let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (h, mi, se) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || se > 60 {
        return None;
    }
    // Days from civil (Howard Hinnant's algorithm), proleptic Gregorian. Inputs are bounded
    // above (4-digit year), so every step is checked only for form.
    let y = if mo <= 2 { y.checked_sub(1)? } else { y };
    let era = y.div_euclid(400);
    let yoe = y.checked_sub(era.checked_mul(400)?)?;
    let mp = mo.checked_add(9)?.checked_rem(12)?;
    let doy = mp
        .checked_mul(153)?
        .checked_add(2)?
        .checked_div(5)?
        .checked_add(d)?
        .checked_sub(1)?;
    let doe = yoe
        .checked_mul(365)?
        .checked_add(yoe.checked_div(4)?)?
        .checked_sub(yoe.checked_div(100)?)?
        .checked_add(doy)?;
    let days = era
        .checked_mul(146_097)?
        .checked_add(doe)?
        .checked_sub(719_468)?;
    let secs = h
        .checked_mul(3_600)?
        .checked_add(mi.checked_mul(60)?)?
        .checked_add(se)?;
    days.checked_mul(86_400)?.checked_add(secs)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::disallowed_macros,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;

    #[test]
    fn test_every_alert_names_an_existing_runbook() {
        let a = Address::new_from_array([1; 32]);
        let alerts = [
            Alert::SlotsOverdue {
                series: a,
                slots: 1,
                since: 0,
            },
            Alert::SettleOverdue { series: a, due: 0 },
            Alert::VoidPending { series: a, due: 0 },
            Alert::LowBalance {
                payer: a,
                lamports: 0,
            },
            Alert::Leftovers { payer: a, count: 1 },
            Alert::KeeperSilent {
                keeper: "primary".into(),
                last: None,
            },
            Alert::HeartbeatUnknown,
        ];
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        for alert in alerts {
            let path = alert.runbook().split(' ').next().unwrap();
            assert!(root.join(path).is_file(), "{alert:?}: {path} is missing");
            assert!(alert
                .text()
                .ends_with(&format!("Runbook: {}", alert.runbook())));
        }
    }

    #[test]
    fn test_parse_rfc3339_utc() {
        assert_eq!(parse_rfc3339_utc("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            parse_rfc3339_utc("2026-10-30T20:00:00Z"),
            Some(1_793_390_400)
        );
        assert_eq!(
            parse_rfc3339_utc("2026-11-06T21:00:00Z"),
            Some(1_793_998_800)
        );
        assert_eq!(
            parse_rfc3339_utc("2024-02-29T12:00:00Z"),
            Some(1_709_208_000)
        );
        assert_eq!(parse_rfc3339_utc("2026-11-06T21:00:00+00:00"), None);
        assert_eq!(parse_rfc3339_utc("2026-13-06T21:00:00Z"), None);
    }

    #[test]
    fn test_heartbeat_round_trip_and_latest() {
        let b = heartbeat_body("primary", "pass=3 live=26");
        assert_eq!(heartbeat_keeper(&b), Some("primary"));
        assert_eq!(heartbeat_keeper("hello"), None);
        assert_eq!(heartbeat_keeper("heartbeat keeper="), None);
        let msgs = vec![
            (100, heartbeat_body("primary", "")),
            (300, heartbeat_body("secondary", "")),
            (200, heartbeat_body("primary", "")),
            (400, "ALERT something".to_string()),
        ];
        let latest = latest_heartbeats(&msgs);
        assert_eq!(latest.get("primary"), Some(&200));
        assert_eq!(latest.get("secondary"), Some(&300));
        assert_eq!(latest.len(), 2);
    }

    #[test]
    fn test_parse_messages() {
        let json = br#"{"cursor":1,"messages":[{"created_at":"2026-11-06T21:00:00Z","body":"heartbeat keeper=primary pass=1"},{"created_at":"bad","body":"x"}]}"#;
        let m = parse_messages(json).unwrap();
        assert_eq!(
            m,
            vec![(1_793_998_800, "heartbeat keeper=primary pass=1".into())]
        );
        assert!(parse_messages(b"not json").is_err());
    }

    #[test]
    fn test_heartbeat_due() {
        assert!(heartbeat_due(None, 1, 0));
        assert!(!heartbeat_due(Some((0, 1)), 1, HEARTBEAT_PERIOD_S - 1));
        assert!(heartbeat_due(Some((0, 1)), 1, HEARTBEAT_PERIOD_S));
        assert!(heartbeat_due(Some((0, 1)), 2, 1));
    }

    #[test]
    fn test_liveness_controls() {
        let keepers = vec!["primary".to_string(), "secondary".to_string()];
        let mut seen = BTreeMap::new();
        seen.insert("primary".to_string(), 10_000);
        seen.insert("secondary".to_string(), 10_000);
        // Both alive.
        assert!(liveness_alerts(&keepers, Some(&seen), 10_000 + HEARTBEAT_TIMEOUT_S).is_empty());
        // Keeper dead: silent past the timeout, the other one still fine.
        seen.insert("secondary".to_string(), 11_000);
        let a = liveness_alerts(&keepers, Some(&seen), 10_001 + HEARTBEAT_TIMEOUT_S);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].key(), "silent:primary");
        // Transport lost: unknown, not a false "keeper dead".
        assert_eq!(
            liveness_alerts(&keepers, None, 99_999),
            vec![Alert::HeartbeatUnknown]
        );
        // Never seen.
        let none = BTreeMap::new();
        assert_eq!(liveness_alerts(&keepers, Some(&none), 0).len(), 2);
    }

    /// Records every send; fails while `down` (one destination or all).
    #[derive(Default)]
    struct Recorder {
        sent: std::cell::RefCell<Vec<(Target, String)>>,
        down: std::cell::Cell<bool>,
    }

    impl Notify for Recorder {
        fn send(&self, target: &Target, body: &str) -> Result<(), String> {
            if self.down.get() {
                return Err("bus unreachable".into());
            }
            self.sent
                .borrow_mut()
                .push((target.clone(), body.to_string()));
            Ok(())
        }
        fn read_channel(&self, _: &str) -> Result<Vec<(i64, String)>, String> {
            Ok(Vec::new())
        }
    }

    fn low(lamports: u64) -> Alert {
        Alert::LowBalance {
            payer: Address::new_from_array([7; 32]),
            lamports,
        }
    }

    #[test]
    fn test_a_failed_delivery_is_retried_until_it_succeeds_and_sent_once() {
        let n = Recorder::default();
        let mut a = Announcer::new();
        n.down.set(true);
        a.step(&n, CHANNEL, "c/s", &[low(1)], &[]);
        assert!(n.sent.borrow().is_empty());
        // Recovered: the persistent alert goes out to both destinations, once.
        n.down.set(false);
        a.step(&n, CHANNEL, "c/s", &[low(1)], &[]);
        a.step(&n, CHANNEL, "c/s", &[low(1)], &[]);
        assert_eq!(n.sent.borrow().len(), 2);
    }

    #[test]
    fn test_an_undelivered_alert_is_retried_while_unobserved_then_resolved() {
        // Codex R2: send fails, the resource becomes unobserved, the bus recovers (the last
        // known alert is delivered), then it is observed gone (resolved once).
        let n = Recorder::default();
        let mut a = Announcer::new();
        let key = low(1).key();
        n.down.set(true);
        a.step(&n, CHANNEL, "c/s", &[low(1)], &[]);
        n.down.set(false);
        a.step(&n, CHANNEL, "c/s", &[], std::slice::from_ref(&key));
        {
            let sent = n.sent.borrow();
            assert_eq!(sent.len(), 2, "{sent:?}");
            assert!(sent.iter().all(|(_, b)| b.contains("last known")));
        }
        a.step(&n, CHANNEL, "c/s", &[], &[]);
        let sent = n.sent.borrow();
        assert_eq!(sent.len(), 3, "{sent:?}");
        assert_eq!(sent[2].1, format!("RESOLVED {key}"));
    }

    #[test]
    fn test_an_unobserved_condition_is_never_resolved_until_observed_gone() {
        let n = Recorder::default();
        let mut a = Announcer::new();
        let payer_key = low(1).key();
        a.step(&n, CHANNEL, "c/s", &[low(1)], &[]);
        // The balance read fails: no alert computed, but no RESOLVED either.
        a.step(&n, CHANNEL, "c/s", &[], std::slice::from_ref(&payer_key));
        assert_eq!(a.active(), vec![payer_key.clone()]);
        assert!(n
            .sent
            .borrow()
            .iter()
            .all(|(_, b)| !b.starts_with("RESOLVED")));
        // Observed and fine: resolved, once; a failed resolution notice is retried.
        n.down.set(true);
        a.step(&n, CHANNEL, "c/s", &[], &[]);
        assert!(a.active().is_empty());
        n.down.set(false);
        a.step(&n, CHANNEL, "c/s", &[], &[]);
        a.step(&n, CHANNEL, "c/s", &[], &[]);
        let resolved = n
            .sent
            .borrow()
            .iter()
            .filter(|(_, b)| b == &format!("RESOLVED {payer_key}"))
            .count();
        assert_eq!(resolved, 1);
    }

    // 2026: DST until Sunday Nov 1, 06:00Z; Oct 30 is EDT, Nov 13 is EST.
    const FRI_NOV13_21Z: i64 = 1_794_603_600;
    const FRI_OCT30_20Z: i64 = 1_793_390_400;

    #[test]
    fn test_market_hours_new_york() {
        // Friday Nov 13, EST: open at 16:00 (21:00Z) and 16:59, closed from 17:00 (22:00Z).
        assert!(metals_market_open(FRI_NOV13_21Z));
        assert!(metals_market_open(FRI_NOV13_21Z + 3_599));
        assert!(!metals_market_open(FRI_NOV13_21Z + 3_600));
        // Saturday, and Sunday before 18:00 (23:00Z): closed; Sunday 18:00: open.
        assert!(!metals_market_open(FRI_NOV13_21Z + 86_400));
        let sun_18 = FRI_NOV13_21Z + 2 * 86_400 + 2 * 3_600;
        assert!(!metals_market_open(sun_18 - 1));
        assert!(metals_market_open(sun_18));
        // Wednesday's daily pause, 17:00 to 18:00 EST (22:00Z to 23:00Z).
        let wed_17 = FRI_NOV13_21Z - 2 * 86_400 + 3_600;
        assert!(metals_market_open(wed_17 - 1));
        assert!(!metals_market_open(wed_17));
        assert!(!metals_market_open(wed_17 + 3_599));
        assert!(metals_market_open(wed_17 + 3_600));
        // Friday Oct 30, EDT: 16:00 is 20:00Z, the close is 21:00Z.
        assert!(metals_market_open(FRI_OCT30_20Z + 3_599));
        assert!(!metals_market_open(FRI_OCT30_20Z + 3_600));
    }

    #[test]
    fn test_new_york_dst_edges() {
        // 2026: from Sunday Mar 8, 07:00Z, to Sunday Nov 1, 06:00Z.
        let mar8_07z = days_from_civil(2026, 3, 8) * 86_400 + 7 * 3_600;
        let nov1_06z = days_from_civil(2026, 11, 1) * 86_400 + 6 * 3_600;
        assert!(!new_york_dst(mar8_07z - 1));
        assert!(new_york_dst(mar8_07z));
        assert!(new_york_dst(nov1_06z - 1));
        assert!(!new_york_dst(nov1_06z));
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(
            days_from_civil(2026, 11, 13) * 86_400 + 21 * 3_600,
            FRI_NOV13_21Z
        );
        assert_eq!(weekday(days_from_civil(2026, 11, 13)), 5);
    }

    fn listed(underlying: u8, expiry: i64, multiplier: u32, active: bool) -> Listed {
        Listed {
            underlying,
            expiry,
            multiplier,
            active,
        }
    }

    #[test]
    fn test_listing_alerts() {
        let now = FRI_NOV13_21Z - 3 * 86_400;
        let next = FRI_NOV13_21Z;
        let cal = vec![vec![next, next + 7 * 86_400], vec![next]];
        // XAU has a live 1 oz series; XAG only a 50 oz one, and its calendar is low.
        let series = [listed(0, next, 1_000, true), listed(1, next, 50_000, true)];
        let a = listing_alerts(&series, &cal, 1_000, now, true);
        assert_eq!(
            a,
            vec![
                Alert::CalendarLow {
                    underlying: 1,
                    future: 1
                },
                Alert::NoNextSeries { underlying: 1 }
            ]
        );
        assert_eq!(a[1].key(), "next-series:xag");
        assert!(a[0].text().contains("docs/runbooks/series-roll.md"));
        // Market closed: no NoNextSeries, the calendar alert stays.
        assert_eq!(listing_alerts(&series, &cal, 1_000, now, false).len(), 1);
        // A series expiring in less than a day, or no longer active, does not count.
        let late = now + NEXT_SERIES_AHEAD_S - 1;
        let soon = [listed(0, late, 1_000, true), listed(1, next, 1_000, false)];
        let a = listing_alerts(&soon, &cal, 1_000, now, true);
        assert!(a.contains(&Alert::NoNextSeries { underlying: 0 }));
        assert!(a.contains(&Alert::NoNextSeries { underlying: 1 }));
        // Two future expiries are enough; one already past does not count.
        let cal = vec![vec![now - 1, next, next + 1], vec![next, next + 1]];
        let ok = [listed(0, next, 1_000, true), listed(1, next, 1_000, true)];
        assert!(listing_alerts(&ok, &cal, 1_000, now, true).is_empty());
    }
}
