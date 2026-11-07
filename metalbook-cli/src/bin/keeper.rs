//! `metalbook-keeper`: runs the settlement keeper (`metalbook_keeper::Keeper::tick`) over every
//! live series of a deployment, through Hermes (key from `METALBOOK_HERMES_API_KEY` only) and
//! the Pyth receiver, every `--interval-s` seconds (`docs/devnet.md`, step 5).
//!
//! A series is ticked from the start of its averaging window until it is settled or voided:
//! before the window the keeper would only refresh the fallback candidate each pass. Errors are
//! printed per series and the loop goes on; neither the RPC URL nor the key is ever printed.
//!
//! One process per payer, enforced with `pyth_post::PayerLock` (shared with every other
//! pyth-post poster on this machine, varswap's keeper included): a second poster's orphan scan
//! could take this one's in-flight encoded VAAs for leftovers and close them (seen on devnet,
//! 2026-10-30). Orphans are found with `pyth_post::LeftoverScanner`, which only reports an
//! encoded VAA that consecutive scans have seen unchanged for `LEFTOVER_MIN_AGE` blocks.
//! By default each pass is batched (`Keeper::pass`, `docs/PLAN-improvements.md` P1): every
//! series in its window is planned, the intents are grouped by signed print, and each print
//! is posted once for as many series as one transaction carries. `--no-batch` ticks each
//! series on its own, as before. When Hermes rate limits a pass, the next pass resumes at the
//! first series it could not plan, so every series progresses under a recurring limit.
//!
//! Hermes budget (`docs/PLAN-improvements.md`, P2): the key is shared with varswap, and
//! metalbook's processes together send at most 7 requests a minute. Every request of this
//! process is paced at `--hermes-interval-s` (default and floor 12 s, 5 a minute) and taken
//! from metalbook's shared limiter (`metalbook_keeper::HermesLimiter`, `docs/PLAN-phase2.md`
//! R1 step 3a), which bounds the sum over every metalbook process on the host, waiting for a
//! slot when the others hold them all.
//!
//! After every pass that sent anything, one line reports the pass's transaction attempts
//! (confirmed, failed, attempted bytes, the pass's largest) and Hermes requests by status, for
//! the batching baseline (P3, P1).

use std::process::ExitCode;
use std::time::Duration;

use clap::Parser;
use metalbook_cli::keeper_chain::RpcChain;
use metalbook_cli::metrics::{
    hermes_interval, CountingHttp, HttpCounts, Metered, TxCounts, MIN_HERMES_INTERVAL_S,
};
use metalbook_cli::params::parse_address;
use metalbook_cli::rpc::Cluster;
use metalbook_cli::watch::{heartbeat_body, heartbeat_due, AcsCli, Notify, Target, CHANNEL};
use metalbook_cli::{read_keypair, CliError};
use metalbook_keeper::{
    Admission, Chain, HermesConfig, HermesSource, Keeper, LimitedHttp, SeriesView, UreqHttp,
    SERIES_LEN,
};
use metalbook_sdk::core::SeriesState;
use metalbook_sdk::Address;
use pyth_post::{LeftoverScanner, PayerLock, RpcTransport};

/// Passes between two scans for orphaned encoded VAAs (about 10 minutes at the default
/// interval, well over `pyth_post::LEFTOVER_MIN_AGE` blocks: a leftover is reported by the
/// second scan that sees it).
const ORPHAN_SCAN_PASSES: u64 = 10;

/// Settlement keeper for metalbook series.
#[derive(Parser)]
#[command(version)]
struct Args {
    /// JSON-RPC URL. Never printed.
    #[arg(long, env = "SOLANA_URL", hide_env_values = true)]
    url: String,
    /// The metalbook program id.
    #[arg(long, env = "METALBOOK_PROGRAM_ID")]
    program_id: String,
    /// Keypair file of the fee payer.
    #[arg(long, env = "METALBOOK_KEYPAIR")]
    keypair: String,
    /// Only these series (default: every series of the program).
    #[arg(long)]
    series: Vec<String>,
    /// Never tick these series (for example series left to void on purpose).
    #[arg(long)]
    exclude: Vec<String>,
    /// Post a heartbeat to the ACS channel `metalbook-keepers` (through the `ai-crew-sync`
    /// CLI and its local profile) on any state change and otherwise every 10 minutes.
    #[arg(long)]
    heartbeat: bool,
    /// This keeper's name in heartbeats (`primary`, `secondary`).
    #[arg(long, default_value = "primary")]
    keeper_name: String,
    /// The `ai-crew-sync` binary for heartbeats.
    #[arg(long, default_value = "ai-crew-sync")]
    acs: String,
    /// Tick each series on its own (one post per series and candidate), as before batching.
    #[arg(long)]
    no_batch: bool,
    /// Seconds between two Hermes requests of this process; at least 12. The sum over every
    /// metalbook process sharing the key must stay at or under 7 requests a minute.
    #[arg(long, default_value_t = MIN_HERMES_INTERVAL_S)]
    hermes_interval_s: u64,
    /// Seconds between passes; keep it well under the challenge delay `D`.
    #[arg(long, default_value_t = 60)]
    interval_s: u64,
    /// One pass, then exit.
    #[arg(long)]
    once: bool,
    /// Check the posting path end to end and exit: post the feed's latest print to this series
    /// (stored as its fallback candidate when within the lookback), through Hermes' `/latest`
    /// only.
    #[arg(long)]
    check_post: Option<String>,
}

fn run(args: Args) -> Result<(), CliError> {
    let program_id = parse_address(&args.program_id)?;
    let payer = read_keypair(&args.keypair)?;
    let payer_address = solana_signer::Signer::pubkey(&payer);
    let hermes = HermesConfig::from_env().map_err(|e| CliError::Usage(e.to_string()))?;
    let interval = hermes_interval(args.hermes_interval_s)?;
    let limiter = metalbook_cli::metrics::hermes_limiter(&hermes)?;
    let http = CountingHttp::new(LimitedHttp::new(UreqHttp::new(), limiter, Admission::Wait));
    let source = HermesSource::new(hermes, &http).with_min_interval(interval);
    let fixed: Vec<Address> = args
        .series
        .iter()
        .map(|s| parse_address(s))
        .collect::<Result<_, _>>()?;
    let excluded: Vec<Address> = args
        .exclude
        .iter()
        .map(|s| parse_address(s))
        .collect::<Result<_, _>>()?;
    let _lock = PayerLock::acquire(&payer_address).map_err(|e| CliError::Usage(e.to_string()))?;
    let cluster = Cluster::new(&args.url);
    let mut chain = RpcChain::new(
        Metered::new(RpcTransport::new(&args.url)),
        &source,
        program_id,
        payer,
    );

    if let Some(series) = &args.check_post {
        let series = parse_address(series)?;
        let data = chain
            .account_data(&series)
            .map_err(|e| CliError::Rpc(e.to_string()))?
            .ok_or_else(|| CliError::AccountNotFound(series.to_string()))?;
        let view = SeriesView::decode(&data).map_err(|e| CliError::Usage(e.to_string()))?;
        let print = source
            .latest(&view.feed_id)
            .map_err(|e| CliError::Rpc(e.to_string()))?
            .ok_or_else(|| CliError::Usage("Hermes has no latest print".into()))?;
        chain
            .post(&series, &view.feed_id, &print)
            .map_err(|e| CliError::Rpc(e.to_string()))?;
        println!(
            "{series}: posted price {} expo {} published {}",
            print.price, print.expo, print.publish_time
        );
        return Ok(());
    }
    let keeper = Keeper;
    let series_len =
        u64::try_from(SERIES_LEN).map_err(|_| CliError::Usage("series size".into()))?;
    let mut pass: u64 = 0;
    let mut resume: Option<Address> = None;
    let acs = AcsCli {
        program: args.acs.clone(),
        session: format!("metalbook-keeper-{}", args.keeper_name),
    };
    let mut last_heartbeat: Option<(i64, u64)> = None;
    let mut leftovers = LeftoverScanner::new();
    loop {
        let started = std::time::Instant::now();
        let mut errors: u32 = 0;
        let http_before = http.counts();
        // Encoded VAAs this payer left open (a post that failed after verification, or one
        // whose creation landed late): scanned at startup and every ORPHAN_SCAN_PASSES passes,
        // closed once two scans LEFTOVER_MIN_AGE blocks apart have seen them unchanged.
        if pass.checked_rem(ORPHAN_SCAN_PASSES) == Some(0) {
            let scan = leftovers.scan(&*chain.transport(), &payer_address);
            match scan {
                Ok(found) if !found.is_empty() => {
                    println!("closing {} orphaned encoded VAA(s)", found.len());
                    chain.adopt_orphans(found);
                }
                Ok(_) => {}
                Err(e) => eprintln!("orphan scan skipped: {e}"),
            }
            chain.close_orphans();
        }
        pass = pass.wrapping_add(1);
        // The series list and the clock: an RPC error here (a rate-limited getProgramAccounts,
        // a timeout) skips this pass and retries at the next one instead of ending the
        // process, which would leave every window unattended. `--once` reports it and exits.
        let listed = if fixed.is_empty() {
            cluster.accounts_of_size(&program_id, series_len)
        } else {
            Ok(fixed.clone())
        };
        let read = listed.and_then(|all| {
            let now = chain.now().map_err(|e| CliError::Rpc(e.to_string()))?;
            Ok((all, now))
        });
        let (mut all, now) = match read {
            Ok(read) => read,
            Err(e) => {
                eprintln!("pass {pass}: skipped, the chain could not be read: {e}");
                if args.once {
                    return Err(e);
                }
                std::thread::sleep(Duration::from_secs(args.interval_s));
                continue;
            }
        };
        all.retain(|s| !excluded.contains(s));
        // Resume where the last rate-limited pass stopped.
        resume_at(&mut all, resume.take());
        // The series in their averaging window (or past it) and not settled.
        let mut live = Vec::new();
        for series in &all {
            let Ok(Some(data)) = chain.account_data(series) else {
                continue;
            };
            let Ok(view) = SeriesView::decode(&data) else {
                continue;
            };
            if matches!(view.state, SeriesState::Settled | SeriesState::Voided) {
                continue;
            }
            let window_start = view
                .expiry
                .saturating_sub(i64::from(view.params.window_s()));
            if now >= window_start {
                live.push(*series);
            }
        }
        if args.no_batch {
            for series in &live {
                match keeper.tick(&mut chain, &source, series) {
                    Ok(r) => print_tick(series, &r),
                    Err(e) => {
                        eprintln!("{series}: {e}");
                        errors = errors.saturating_add(1);
                        if is_rate_limit(&e.to_string()) {
                            // Every later request of this pass would fail too: stop here and
                            // start the next pass with this series.
                            resume = Some(*series);
                            break;
                        }
                    }
                }
            }
        } else if !live.is_empty() {
            match keeper.pass(&mut chain, &source, &live) {
                Ok(report) => {
                    for (series, outcome) in &report.series {
                        match outcome {
                            Ok(r) => print_tick(series, r),
                            Err(e) => {
                                eprintln!("{series}: {e}");
                                errors = errors.saturating_add(1);
                                if resume.is_none() && is_rate_limit(&e.to_string()) {
                                    resume = Some(*series);
                                }
                            }
                        }
                    }
                    if report.posts > 0 {
                        println!(
                            "pass {pass}: {} prints, {} posts for {} series",
                            report.groups,
                            report.posts,
                            live.len()
                        );
                    }
                }
                Err(e) => eprintln!("pass {pass}: {e}"),
            }
        }
        let tx = chain.transport().take();
        let http_pass = http.counts().since(&http_before);
        report_pass(pass, started.elapsed(), &tx, &http_pass);
        if args.heartbeat {
            // State digest: every live series' account bytes. A change (a new candidate, a
            // settle) posts a heartbeat at once; otherwise every HEARTBEAT_PERIOD_S.
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            let mut settled = 0_u32;
            for series in &live {
                if let Ok(Some(data)) = chain.account_data(series) {
                    std::hash::Hash::hash(&data, &mut hasher);
                    if SeriesView::decode(&data).is_ok_and(|v| {
                        matches!(v.state, SeriesState::Settled | SeriesState::Voided)
                    }) {
                        settled = settled.saturating_add(1);
                    }
                }
            }
            let digest = std::hash::Hasher::finish(&hasher);
            let now_s = chain.now().unwrap_or(0);
            if heartbeat_due(last_heartbeat, digest, now_s) {
                let body = heartbeat_body(
                    &args.keeper_name,
                    &format!(
                        "payer={payer_address} pass={pass} live={} settled={settled} errors={errors} tx={} hermes={}",
                        live.len(),
                        tx.attempts(),
                        http_pass.total()
                    ),
                );
                match acs.send(&Target::Channel(CHANNEL.into()), &body) {
                    Ok(()) => last_heartbeat = Some((now_s, digest)),
                    Err(e) => eprintln!("heartbeat not sent: {e}"),
                }
            }
        }
        if args.once {
            return Ok(());
        }
        std::thread::sleep(Duration::from_secs(args.interval_s));
    }
}

/// One line for a series that did anything.
fn print_tick(series: &Address, r: &metalbook_keeper::TickReport) {
    if r.slots > 0 || r.links > 0 || r.fallback || r.settled || r.skipped > 0 {
        println!(
            "{series}: slots +{} links +{} fallback {} skipped {} settled {}",
            r.slots, r.links, r.fallback, r.skipped, r.settled
        );
    }
}

/// Puts `resume` (the series a rate-limited pass stopped at) first, keeping the cyclic order;
/// unchanged when it is `None` or no longer listed.
fn resume_at(series: &mut [Address], resume: Option<Address>) {
    if let Some(i) = resume.and_then(|r| series.iter().position(|a| *a == r)) {
        series.rotate_left(i);
    }
}

/// One line per pass that sent anything: transactions and Hermes requests.
fn report_pass(pass: u64, took: Duration, tx: &TxCounts, http: &HttpCounts) {
    if tx.ok == 0 && tx.failed == 0 && http.total() == 0 {
        return;
    }
    println!(
        "pass {pass}: {:.1} s; tx attempts {} (ok {}, failed {}), attempted bytes {} max {}; hermes {} (200 {}, 404 {}, 429 {}, other {})",
        took.as_secs_f64(),
        tx.attempts(),
        tx.ok,
        tx.failed,
        tx.bytes,
        tx.max_bytes,
        http.total(),
        http.ok,
        http.not_found,
        http.rate_limited,
        http.other
    );
}

/// Whether a tick failed on Hermes' rate limit (`HermesSource`'s 429 and cooldown errors).
fn is_rate_limit(error: &str) -> bool {
    error.contains("HTTP 429") || error.contains("rate limit")
}

fn main() -> ExitCode {
    match run(Args::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::disallowed_macros)]
mod tests {
    use super::*;

    #[test]
    fn a_rate_limited_pass_resumes_at_its_series() {
        let a = |n: u8| Address::new_from_array([n; 32]);
        let mut list = vec![a(1), a(2), a(3), a(4)];
        resume_at(&mut list, Some(a(3)));
        assert_eq!(list, vec![a(3), a(4), a(1), a(2)]);
        resume_at(&mut list, None);
        assert_eq!(list, vec![a(3), a(4), a(1), a(2)]);
        resume_at(&mut list, Some(a(9)));
        assert_eq!(list, vec![a(3), a(4), a(1), a(2)]);
    }

    #[test]
    fn rate_limit_errors_are_recognised() {
        assert!(is_rate_limit(
            "price source failed: Hermes returned HTTP 429 (rate limited); cooling down"
        ));
        assert!(is_rate_limit(
            "price source failed: Hermes rate limit: cooling down, request not sent"
        ));
        assert!(!is_rate_limit(
            "chain operation failed: RPC: blockhash not found"
        ));
    }
}
