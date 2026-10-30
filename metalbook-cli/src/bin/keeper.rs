//! `metalbook-keeper`: runs the settlement keeper (`metalbook_keeper::Keeper::tick`) over every
//! live series of a deployment, through Hermes (key from `METALBOOK_HERMES_API_KEY` only) and
//! the Pyth receiver, every `--interval-s` seconds (`docs/devnet.md`, step 5).
//!
//! A series is ticked from the start of its averaging window until it is settled or voided:
//! before the window the keeper would only refresh the fallback candidate each pass. Errors are
//! printed per series and the loop goes on; neither the RPC URL nor the key is ever printed.

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
    let source = HermesSource::new(hermes, UreqHttp::new());
    let fixed: Vec<Address> = args
        .series
        .iter()
        .map(|s| parse_address(s))
        .collect::<Result<_, _>>()?;
    let mut chain = RpcChain::new(Cluster::new(&args.url), &source, program_id, payer);
    // Encoded VAAs left by earlier runs whose posts failed after verification.
    let scan = chain.rpc().encoded_vaas_of(&payer_address);
    match scan {
        Ok(found) if !found.is_empty() => {
            println!("closing {} orphaned encoded VAA(s)", found.len());
            chain.adopt_orphans(found);
        }
        Ok(_) => {}
        Err(e) => eprintln!("orphan scan skipped: {e}"),
    }
    chain.close_orphans();
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
    loop {
        let all = if fixed.is_empty() {
            chain.rpc().accounts_of_size(&program_id, series_len)?
        } else {
            fixed.clone()
        };
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
                Err(e) => eprintln!("{series}: {e}"),
            }
        }
        if args.once {
            return Ok(());
        }
        std::thread::sleep(Duration::from_secs(args.interval_s));
    }
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
