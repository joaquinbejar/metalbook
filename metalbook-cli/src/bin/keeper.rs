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
//! When Hermes rate limits a pass, the next pass resumes at the series where it stopped, so
//! every series progresses under a recurring limit.

use std::process::ExitCode;
use std::time::Duration;

use clap::Parser;
use metalbook_cli::keeper_chain::RpcChain;
use metalbook_cli::params::parse_address;
use metalbook_cli::rpc::Cluster;
use metalbook_cli::{read_keypair, CliError};
use metalbook_keeper::{
    Chain, HermesConfig, HermesSource, Keeper, SeriesView, UreqHttp, SERIES_LEN,
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
    // At most 6 Hermes requests a minute: the timestamp endpoint allows about 10 per key.
    let source =
        HermesSource::new(hermes, UreqHttp::new()).with_min_interval(Duration::from_secs(10));
    let fixed: Vec<Address> = args
        .series
        .iter()
        .map(|s| parse_address(s))
        .collect::<Result<_, _>>()?;
    let _lock = PayerLock::acquire(&payer_address).map_err(|e| CliError::Usage(e.to_string()))?;
    let cluster = Cluster::new(&args.url);
    let mut chain = RpcChain::new(RpcTransport::new(&args.url), &source, program_id, payer);

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
    let mut leftovers = LeftoverScanner::new();
    loop {
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
        let mut all: Vec<Address> = if fixed.is_empty() {
            cluster.accounts_of_size(&program_id, series_len)?
        } else {
            fixed.clone()
        };
        // Resume where the last rate-limited pass stopped.
        resume_at(&mut all, resume.take());
        let now = chain.now().map_err(|e| CliError::Rpc(e.to_string()))?;
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
            if now < window_start {
                continue;
            }
            match keeper.tick(&mut chain, &source, series) {
                Ok(r) => {
                    if r.slots > 0 || r.links > 0 || r.fallback || r.settled || r.skipped > 0 {
                        println!(
                            "{series}: slots +{} links +{} fallback {} skipped {} settled {}",
                            r.slots, r.links, r.fallback, r.skipped, r.settled
                        );
                    }
                }
                Err(e) => {
                    eprintln!("{series}: {e}");
                    if is_rate_limit(&e.to_string()) {
                        // Every later request of this pass would fail too: stop here and
                        // start the next pass with this series.
                        resume = Some(*series);
                        break;
                    }
                }
            }
        }
        if args.once {
            return Ok(());
        }
        std::thread::sleep(Duration::from_secs(args.interval_s));
    }
}

/// Puts `resume` (the series a rate-limited pass stopped at) first, keeping the cyclic order;
/// unchanged when it is `None` or no longer listed.
fn resume_at(series: &mut [Address], resume: Option<Address>) {
    if let Some(i) = resume.and_then(|r| series.iter().position(|a| *a == r)) {
        series.rotate_left(i);
    }
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
