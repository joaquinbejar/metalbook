//! The watcher's schedule alerts (`metalbook_cli::watch::series_alerts`) on a real series in
//! LiteSVM at scripted times: a slot with no print is overdue exactly from its boundary plus
//! tolerance plus grace (oracle availability unknown); a posted slot is not; an unsettled
//! series is overdue from `window_end + D + 600` and void-pending from `hard + D`; a settled
//! series raises nothing. Run `make program` first.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::disallowed_macros,
    missing_docs
)]

mod common;

use common::*;
use metalbook_cli::watch::{series_alerts, Alert, SETTLE_GRACE_S, SLOT_GRACE_S};
use metalbook_keeper::SeriesView;
use solana_address::Address;

fn alerts(env: &Env, series: &Address, now: i64) -> Vec<Alert> {
    let view = SeriesView::decode(&env.svm.get_account(series).unwrap().data).unwrap();
    series_alerts(series, &view, now)
}

#[test]
fn test_a_missing_print_is_overdue_from_boundary_plus_tolerance_plus_grace() {
    let mut env = Env::new();
    let series = env.create_series(SeriesArgs::xau_put(4_000));
    let slot0 = EXPIRY - WINDOW_S;
    let due = slot0 + TOLERANCE_S + SLOT_GRACE_S;
    assert!(alerts(&env, &series, due - 1).is_empty());
    assert_eq!(
        alerts(&env, &series, due),
        vec![Alert::SlotsOverdue {
            series,
            slots: 1,
            since: slot0
        }]
    );
    assert!(alerts(&env, &series, due)
        .iter()
        .all(|a| a.text().contains("oracle availability unknown")));
    // A keeper posts slot 0: nothing overdue until slot 1 is.
    env.set_time(slot0 + 5);
    env.post(&series, 4_000_000, slot0 + 2, slot0 - 1).unwrap();
    assert!(alerts(&env, &series, due).is_empty());
    assert_eq!(
        alerts(&env, &series, due + SLOT_S),
        vec![Alert::SlotsOverdue {
            series,
            slots: 1,
            since: slot0 + SLOT_S
        }]
    );
}

#[test]
fn test_unsettled_series_turn_settle_overdue_then_void_pending_and_settled_ones_are_quiet() {
    let mut env = Env::new();
    let args = SeriesArgs::xau_put(4_000);
    let series = env.create_series(args);
    let settle_due = EXPIRY + D + SETTLE_GRACE_S;
    let void_due = EXPIRY + HARD_S + D;
    let kinds = |v: Vec<Alert>| -> Vec<String> { v.iter().map(|a| a.key()).collect() };
    let s = series.to_string();
    assert_eq!(
        kinds(alerts(&env, &series, settle_due - 1)),
        vec![format!("slots:{s}")]
    );
    assert_eq!(
        kinds(alerts(&env, &series, settle_due)),
        vec![format!("slots:{s}"), format!("settle:{s}")]
    );
    assert_eq!(
        kinds(alerts(&env, &series, void_due)),
        vec![format!("slots:{s}"), format!("void:{s}")]
    );
    // Settled on a full window: no alert at any time.
    env.settle_with_window(args, &[3_600_000; 30]);
    for t in [settle_due, void_due] {
        assert!(alerts(&env, &series, t).is_empty());
    }
}
