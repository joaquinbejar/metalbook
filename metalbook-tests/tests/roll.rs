//! The series roller (`metalbook_cli::roll`, `docs/PLAN-phase2.md` R1) against the program in
//! LiteSVM, with scripted Pyth prints: what a run lists, that a second run sends nothing, that
//! a run failing halfway is finished by the next one, and every skip and refusal. Run
//! `make program` first.

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

use std::collections::VecDeque;
use std::time::Duration;

use common::*;
use metalbook_cli::roll::{
    roll, LatestPrints, MetalOutcome, PrintRead, RollOptions, RollReport, HERMES_SPACING_S,
};
use metalbook_cli::{create_series_ix, read_config, AccountInfo, CliError, Rpc};

use metalbook_keeper::PrintMsg;
use solana_address::Address;
use solana_keypair::Keypair;
use solana_transaction::Transaction;

/// [`Rpc`] over the LiteSVM of an [`Env`]; with `fail_after`, every send after that many
/// confirmed ones fails (a run cut halfway).
struct Svm<'a> {
    env: &'a mut Env,
    sent: usize,
    fail_after: Option<usize>,
}

impl<'a> Svm<'a> {
    fn new(env: &'a mut Env) -> Self {
        Self {
            env,
            sent: 0,
            fail_after: None,
        }
    }
}

impl Rpc for Svm<'_> {
    fn latest_blockhash(&mut self) -> Result<solana_hash::Hash, CliError> {
        self.env.svm.expire_blockhash();
        Ok(self.env.svm.latest_blockhash())
    }

    fn account(&mut self, address: &Address) -> Result<Option<AccountInfo>, CliError> {
        Ok(self.env.svm.get_account(address).map(|a| AccountInfo {
            owner: a.owner,
            data: a.data,
        }))
    }

    fn send(&mut self, tx: &Transaction) -> Result<(), CliError> {
        if self.fail_after.is_some_and(|n| self.sent >= n) {
            return Err(CliError::Rpc("node unavailable (scripted)".into()));
        }
        self.env
            .svm
            .send_transaction(tx.clone())
            .map(|_| ())
            .map_err(|e| CliError::Rpc(format!("{:?} {:?}", e.err, e.meta.logs)))?;
        self.sent += 1;
        Ok(())
    }
}

/// Scripted `/latest` answers, in order, counting the reads.
struct Script {
    reads: VecDeque<Result<PrintRead, CliError>>,
    asked: usize,
}

impl Script {
    fn new(reads: Vec<Result<PrintRead, CliError>>) -> Self {
        Self {
            reads: reads.into(),
            asked: 0,
        }
    }
}

impl LatestPrints for Script {
    fn latest(&mut self, _feed: &[u8; 32]) -> Result<PrintRead, CliError> {
        self.asked += 1;
        self.reads
            .pop_front()
            .unwrap_or_else(|| Err(CliError::Rpc("no scripted read left".into())))
    }
}

/// XAU at 4012.345 and XAG at 50.12345, published 10 s before `NOW`.
fn xau() -> Result<PrintRead, CliError> {
    Ok(PrintRead::Print(print(4_012_345, EXPO_XAU, NOW - 10)))
}
fn xag() -> Result<PrintRead, CliError> {
    Ok(PrintRead::Print(print(5_012_345, EXPO_XAG, NOW - 10)))
}
fn print(price: i64, expo: i8, publish_time: i64) -> PrintMsg {
    PrintMsg {
        price,
        conf: 0,
        expo: i32::from(expo),
        publish_time,
        prev_publish_time: publish_time - 1,
    }
}

/// The program with both metals at 1 oz, as on devnet since Nov 6.
fn env_1oz() -> Env {
    let mut p = Params::default_params();
    p.multiplier = [1_000, 1_000];
    Env::with_params(p)
}

/// The program's series of `metal` at `expiry` within ±20% of the scripted spot: a superset of
/// any ±5% plan, enough to count what a run created.
fn existing(rpc: &mut Svm, metal: u8, expiry: i64) -> Result<usize, CliError> {
    let config = read_config(rpc, &PROGRAM_ID)?;
    let (price, expo) = if metal == XAU {
        (4_012_345, EXPO_XAU)
    } else {
        (5_012_345, EXPO_XAG)
    };
    let spot = metalbook_cli::listing::spot_atoms(price, i32::from(expo))?;
    let wide = metalbook_cli::listing::week_series(
        metal,
        expiry,
        spot,
        config.strike_tick[usize::from(metal)],
        2_000,
        config.cap_bps[0],
    )?;
    let payer = Address::new_unique();
    let mut n = 0;
    for args in &wide {
        let (_, series) = create_series_ix(rpc, &PROGRAM_ID, &payer, args)?;
        if rpc.account(&series)?.is_some() {
            n += 1;
        }
    }
    Ok(n)
}

fn run(
    rpc: &mut Svm,
    prints: &mut Script,
    signer: &Keypair,
    now: i64,
    opts: &RollOptions,
    sleeps: &mut Vec<Duration>,
) -> Result<RollReport, CliError> {
    roll(
        rpc,
        prints,
        &PROGRAM_ID,
        signer,
        &mut || Ok(now),
        opts,
        &mut |rpc: &mut Svm, m, e| existing(rpc, m, e),
        &mut |d| sleeps.push(d),
    )
}

fn execute() -> RollOptions {
    RollOptions {
        execute: true,
        ..RollOptions::default()
    }
}

fn new_of(report: &RollReport, metal: u8) -> Vec<Address> {
    match &report.metals.iter().find(|(m, _)| *m == metal).unwrap().1 {
        MetalOutcome::Listed { new, .. } => new.clone(),
        other => panic!("{metal}: {other:?}"),
    }
}

#[test]
fn test_a_run_lists_the_week_and_a_second_run_sends_nothing() {
    let mut env = env_1oz();
    let lister = env.funded();
    let mut sleeps = Vec::new();
    let mut prints = Script::new(vec![xau(), xag()]);
    let mut rpc = Svm::new(&mut env);
    let first = run(&mut rpc, &mut prints, &lister, NOW, &execute(), &mut sleeps).unwrap();
    assert!(!first.failed(), "{:?}", first.lines);
    let (x, g) = (new_of(&first, XAU), new_of(&first, XAG));
    // XAU 4012.345 ±5% on a 5 USD grid: 3815..4210, 80 strikes, each a put and a call; the run
    // bound of 28 cuts it; XAG 50.12345 ±5% on 0.25: 47.75..52.5, 20 strikes.
    assert_eq!(x.len(), 18, "--max-per-expiry 18 bounds XAU");
    assert_eq!(g.len(), 10, "the run's --max 28 leaves 10 for XAG");
    assert_eq!(rpc.sent, 28);
    for s in x.iter().chain(&g) {
        assert!(rpc.account(s).unwrap().is_some());
    }
    // The two Hermes requests were spaced.
    assert_eq!(sleeps, vec![Duration::from_secs(HERMES_SPACING_S)]);
    // Every series is at the first expiry three days ahead, 1 oz.
    for s in &x {
        let v = metalbook_cli::read_series(&mut rpc, &PROGRAM_ID, s).unwrap();
        assert_eq!((v.expiry, v.multiplier), (EXPIRY, 1_000));
    }

    // Second run, same spot: XAU is at its per-expiry bound, XAG gets the rest of its band
    // (a top-up), and a third run sends nothing at all.
    let mut prints = Script::new(vec![xau(), xag()]);
    let second = run(&mut rpc, &mut prints, &lister, NOW, &execute(), &mut sleeps).unwrap();
    assert!(new_of(&second, XAU).is_empty());
    assert_eq!(new_of(&second, XAG).len(), 18 - 10);
    let mut prints = Script::new(vec![xau(), xag()]);
    let before = rpc.sent;
    let third = run(&mut rpc, &mut prints, &lister, NOW, &execute(), &mut sleeps).unwrap();
    assert!(new_of(&third, XAU).is_empty() && new_of(&third, XAG).is_empty());
    assert_eq!(rpc.sent, before, "nothing sent");
}

#[test]
fn test_a_run_failing_halfway_is_finished_by_the_next() {
    let mut env = env_1oz();
    let lister = env.funded();
    let mut sleeps = Vec::new();
    let opts = RollOptions {
        max: 1_000,
        max_per_expiry: 1_000,
        ..execute()
    };
    let mut rpc = Svm::new(&mut env);
    rpc.fail_after = Some(5);
    let mut prints = Script::new(vec![xau(), xag()]);
    let cut = run(&mut rpc, &mut prints, &lister, NOW, &opts, &mut sleeps).unwrap();
    assert!(cut.failed());
    assert!(matches!(cut.metals[0].1, MetalOutcome::Failed(_)));
    assert_eq!(rpc.sent, 5);
    rpc.fail_after = None;
    let mut prints = Script::new(vec![xau(), xag()]);
    let next = run(&mut rpc, &mut prints, &lister, NOW, &opts, &mut sleeps).unwrap();
    assert!(!next.failed(), "{:?}", next.lines);
    // XAU: 160 in the band, 5 listed by the cut run.
    assert_eq!(new_of(&next, XAU).len(), 160 - 5);
    assert_eq!(new_of(&next, XAG).len(), 40);
    let mut prints = Script::new(vec![xau(), xag()]);
    let done = run(&mut rpc, &mut prints, &lister, NOW, &opts, &mut sleeps).unwrap();
    assert!(new_of(&done, XAU).is_empty() && new_of(&done, XAG).is_empty());
}

#[test]
fn test_a_stale_print_and_a_busy_budget_are_skips() {
    let mut env = env_1oz();
    let lister = env.funded();
    let mut sleeps = Vec::new();
    let weekend = Ok(PrintRead::Print(print(
        4_012_345,
        EXPO_XAU,
        NOW - 2 * 86_400,
    )));
    let mut prints = Script::new(vec![weekend, Ok(PrintRead::Busy)]);
    let mut rpc = Svm::new(&mut env);
    let r = run(&mut rpc, &mut prints, &lister, NOW, &execute(), &mut sleeps).unwrap();
    assert!(!r.failed(), "skips are not failures");
    assert!(matches!(r.metals[0].1, MetalOutcome::Stale(_)));
    assert_eq!(r.metals[1].1, MetalOutcome::Busy);
    assert!(r.lines.iter().any(|l| l.contains("hermes budget busy")));
    assert_eq!(rpc.sent, 0);
}

#[test]
fn test_a_hermes_429_or_a_bad_print_fails_that_metal() {
    let mut env = env_1oz();
    let lister = env.funded();
    let mut sleeps = Vec::new();
    let limited = Err(CliError::Rpc(
        "price source failed: Hermes returned HTTP 429 (rate limited); cooling down".into(),
    ));
    let wrong_expo = Ok(PrintRead::Print(print(5_012_345, EXPO_XAU, NOW - 10)));
    let mut prints = Script::new(vec![limited, wrong_expo]);
    let mut rpc = Svm::new(&mut env);
    let r = run(&mut rpc, &mut prints, &lister, NOW, &execute(), &mut sleeps).unwrap();
    assert!(r.failed());
    assert!(matches!(r.metals[0].1, MetalOutcome::Failed(_)));
    assert!(matches!(r.metals[1].1, MetalOutcome::Failed(_)));
    assert_eq!(rpc.sent, 0, "nothing sent for a failed metal");
}

#[test]
fn test_the_quiet_period_reads_nothing_from_hermes() {
    let mut env = env_1oz();
    let lister = env.funded();
    let mut sleeps = Vec::new();
    for now in [EXPIRY - 7_200, EXPIRY + 7_200] {
        let mut prints = Script::new(vec![xau(), xag()]);
        let mut rpc = Svm::new(&mut env);
        let r = run(&mut rpc, &mut prints, &lister, now, &execute(), &mut sleeps).unwrap();
        assert_eq!(r.quiet, Some(EXPIRY));
        assert_eq!(prints.asked, 0);
        assert_eq!(rpc.sent, 0);
    }
    // One second outside: the run reads Hermes again (and lists for EXPIRY_2, the next one).
    let mut prints = Script::new(vec![xau(), xag()]);
    let mut rpc = Svm::new(&mut env);
    let r = run(
        &mut rpc,
        &mut prints,
        &lister,
        EXPIRY - 7_201,
        &RollOptions::default(),
        &mut sleeps,
    )
    .unwrap();
    assert_eq!(r.quiet, None);
    assert!(prints.asked > 0);
}

#[test]
fn test_a_multiplier_mismatch_refuses_before_hermes() {
    // The test program's default config lists XAG at 50 oz.
    let mut env = Env::new();
    let lister = env.funded();
    let mut sleeps = Vec::new();
    let mut prints = Script::new(vec![xau(), xag()]);
    let mut rpc = Svm::new(&mut env);
    let e = run(&mut rpc, &mut prints, &lister, NOW, &execute(), &mut sleeps).unwrap_err();
    assert!(e.to_string().contains("multiplier"), "{e}");
    assert_eq!(prints.asked, 0);
    assert_eq!(rpc.sent, 0);
}

#[test]
fn test_no_expiry_closer_than_min_listing_s_is_attempted() {
    // min_listing_s of 4 h, wider than the 2 h quiet period, and a zero lead: 3 h before EXPIRY
    // the run is outside the quiet period but EXPIRY is too close to list. XAU moves to
    // EXPIRY_2; XAG, whose calendar has EXPIRY only, has no expiry and reads nothing.
    let mut p = Params::default_params();
    p.multiplier = [1_000, 1_000];
    p.min_listing_s = 14_400;
    let mut env = Env::with_params(p);
    let lister = env.funded();
    let mut sleeps = Vec::new();
    let opts = RollOptions {
        min_lead_s: 0,
        max: 1_000,
        max_per_expiry: 1_000,
        ..execute()
    };
    let now = EXPIRY - 10_800;
    env.set_time(now);
    let fresh = Ok(PrintRead::Print(print(4_012_345, EXPO_XAU, now - 10)));
    let mut prints = Script::new(vec![fresh]);
    let mut rpc = Svm::new(&mut env);
    let r = run(&mut rpc, &mut prints, &lister, now, &opts, &mut sleeps).unwrap();
    assert_eq!(r.quiet, None);
    assert!(!r.failed(), "{:?}", r.lines);
    match &r.metals[0].1 {
        MetalOutcome::Listed { expiry, new, .. } => {
            assert_eq!(*expiry, EXPIRY_2);
            assert_eq!(new.len(), 160);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(r.metals[1].1, MetalOutcome::NoExpiry);
    assert_eq!(prints.asked, 1);
    assert_eq!(existing(&mut rpc, XAU, EXPIRY).unwrap(), 0);
    assert_eq!(existing(&mut rpc, XAG, EXPIRY).unwrap(), 0);
}

#[test]
fn test_a_dry_run_sends_nothing() {
    let mut env = env_1oz();
    let lister = env.funded();
    let mut sleeps = Vec::new();
    let mut prints = Script::new(vec![xau(), xag()]);
    let mut rpc = Svm::new(&mut env);
    let r = run(
        &mut rpc,
        &mut prints,
        &lister,
        NOW,
        &RollOptions::default(),
        &mut sleeps,
    )
    .unwrap();
    assert_eq!(new_of(&r, XAU).len(), 18);
    assert_eq!(rpc.sent, 0);
    assert!(r.lines.iter().any(|l| l.contains("dry run")));
}

/// The run behind bus 65319: the first metal's sends took 84 s, and the second metal's fresh
/// print was refused as "in the future" against the clock read when the run started.
#[test]
fn test_the_clock_advancing_between_metals_still_lists_the_second() {
    let mut env = env_1oz();
    let lister = env.funded();
    let mut rpc = Svm::new(&mut env);
    // Past the 5 s skew limit by far, and inside the 300 s age limit.
    const SENDS_TOOK: i64 = 90;
    let late_xag = Ok(PrintRead::Print(print(
        5_012_345,
        EXPO_XAG,
        NOW + SENDS_TOOK - 1,
    )));
    let mut prints = Script::new(vec![xau(), late_xag]);
    // Run start, then XAU before and after its print; everything later is after its sends.
    let mut reads = 0;
    let mut clock = || {
        reads += 1;
        Ok(if reads <= 3 { NOW } else { NOW + SENDS_TOOK })
    };
    let first = roll(
        &mut rpc,
        &mut prints,
        &PROGRAM_ID,
        &lister,
        &mut clock,
        &execute(),
        &mut |rpc: &mut Svm, m, e| existing(rpc, m, e),
        &mut |_| {},
    )
    .unwrap();
    assert!(!first.failed(), "{:?}", first.lines);
    assert_eq!(new_of(&first, XAU).len(), 18);
    assert_eq!(new_of(&first, XAG).len(), 10, "the run's --max leaves 10");

    // A rerun: XAU has its 18 and sends nothing, XAG only tops up.
    let mut prints = Script::new(vec![xau(), xag()]);
    let mut sleeps = Vec::new();
    let sent = rpc.sent;
    let second = run(&mut rpc, &mut prints, &lister, NOW, &execute(), &mut sleeps).unwrap();
    assert!(new_of(&second, XAU).is_empty(), "{:?}", second.lines);
    assert_eq!(rpc.sent, sent + new_of(&second, XAG).len());
}

/// A run that reaches a quiet period while sending stops before the next metal's Hermes read.
#[test]
fn test_a_run_entering_the_quiet_period_stops_before_the_next_metal() {
    let mut env = env_1oz();
    let lister = env.funded();
    let mut rpc = Svm::new(&mut env);
    let mut prints = Script::new(vec![xau(), xag()]);
    let mut reads = 0;
    let mut clock = || {
        reads += 1;
        Ok(if reads <= 3 { NOW } else { EXPIRY - 7_200 })
    };
    let r = roll(
        &mut rpc,
        &mut prints,
        &PROGRAM_ID,
        &lister,
        &mut clock,
        &execute(),
        &mut |rpc: &mut Svm, m, e| existing(rpc, m, e),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(r.quiet, Some(EXPIRY));
    assert_eq!(prints.asked, 1, "XAG was not read");
    assert!(!r.failed(), "{:?}", r.lines);
}
