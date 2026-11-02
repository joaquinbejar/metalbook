//! `metalbook-watch`: chain-only alerts for metalbook keepers (`docs/PLAN-improvements.md`, P2).
//!
//! Every `--interval-s` seconds it reads every series of the program, each keeper payer's
//! balance and leftover encoded VAAs, and the keepers' heartbeats on the ACS channel
//! `metalbook-keepers`; it announces each new alert to the channel and to `--alert-to` by DM,
//! and each resolved one to the channel. Lag is measured against the schedule (a slot empty
//! past its tolerance and grace), never against a print it cannot see. It needs no Hermes key
//! and holds no signing key. The RPC URL is never printed.

use std::collections::BTreeSet;
use std::process::ExitCode;
use std::time::Duration;

use clap::Parser;
use metalbook_cli::params::parse_address;
use metalbook_cli::rpc::Cluster;
use metalbook_cli::watch::{
    diff, latest_heartbeats, liveness_alerts, series_alerts, AcsCli, Alert, Notify, Target,
    CHANNEL, LOW_BALANCE_LAMPORTS,
};
use metalbook_cli::CliError;
use metalbook_keeper::{SeriesView, SERIES_LEN};
use metalbook_sdk::Address;
use pyth_post::{LeftoverScanner, RpcTransport};

/// Chain-only alerts for metalbook keepers.
#[derive(Parser)]
#[command(version)]
struct Args {
    /// JSON-RPC URL. Never printed.
    #[arg(long, env = "SOLANA_URL", hide_env_values = true)]
    url: String,
    /// The metalbook program id.
    #[arg(long, env = "METALBOOK_PROGRAM_ID")]
    program_id: String,
    /// A keeper payer to watch (balance and leftovers); repeatable.
    #[arg(long)]
    payer: Vec<String>,
    /// A keeper name whose heartbeat is expected; repeatable.
    #[arg(long)]
    keeper: Vec<String>,
    /// Watch only these series (one `getMultipleAccounts` call per 100) instead of discovering
    /// every series with `getProgramAccounts`, which public RPCs rate-limit.
    #[arg(long)]
    series: Vec<String>,
    /// Never alert on these series (for example series left to void on purpose).
    #[arg(long)]
    exclude: Vec<String>,
    /// Who gets alerts by direct message (`agent/session`).
    #[arg(long, default_value = "joaquin/s-1836f1d6a089")]
    alert_to: String,
    /// The `ai-crew-sync` binary.
    #[arg(long, default_value = "ai-crew-sync")]
    acs: String,
    /// Payer balance alarm, lamports.
    #[arg(long, default_value_t = LOW_BALANCE_LAMPORTS)]
    low_balance: u64,
    /// Seconds between checks.
    #[arg(long, default_value_t = 60)]
    interval_s: u64,
    /// One check, print the alerts, send nothing.
    #[arg(long)]
    once: bool,
}

fn run(args: &Args) -> Result<(), CliError> {
    let program_id = parse_address(&args.program_id)?;
    let payers: Vec<Address> = args
        .payer
        .iter()
        .map(|s| parse_address(s))
        .collect::<Result<_, _>>()?;
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
    let cluster = Cluster::new(&args.url);
    let transport = RpcTransport::new(&args.url);
    let acs = AcsCli {
        program: args.acs.clone(),
        session: "metalbook-watch".into(),
    };
    let mut scanners: Vec<LeftoverScanner> =
        payers.iter().map(|_| LeftoverScanner::new()).collect();
    let mut announced: BTreeSet<String> = BTreeSet::new();
    let series_len =
        u64::try_from(SERIES_LEN).map_err(|_| CliError::Usage("series size".into()))?;
    loop {
        let mut alerts: Vec<Alert> = Vec::new();
        let now = match pyth_post::Transport::now(&transport) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("clock: {e}");
                std::thread::sleep(Duration::from_secs(args.interval_s));
                continue;
            }
        };
        // Every series with its data in one call (or the fixed list in one call per 100).
        let listed = if fixed.is_empty() {
            cluster.accounts_with_data_of_size(&program_id, series_len)
        } else {
            cluster.accounts_data(&fixed)
        };
        let listed = match listed {
            Ok(l) => l,
            Err(e) => {
                // An unreadable chain is not "no alerts": say so, then try again.
                eprintln!("series: {e}");
                if args.once {
                    return Err(e);
                }
                std::thread::sleep(Duration::from_secs(args.interval_s));
                continue;
            }
        };
        for (s, data) in listed {
            if excluded.contains(&s) {
                continue;
            }
            if let Ok(view) = SeriesView::decode(&data) {
                alerts.extend(series_alerts(&s, &view, now));
            }
        }
        for (payer, scanner) in payers.iter().zip(scanners.iter_mut()) {
            match cluster.balance(payer) {
                Ok(lamports) if lamports < args.low_balance => {
                    alerts.push(Alert::LowBalance {
                        payer: *payer,
                        lamports,
                    });
                }
                Ok(_) => {}
                Err(e) => eprintln!("balance of {payer}: {e}"),
            }
            match scanner.scan(&transport, payer) {
                Ok(found) if !found.is_empty() => alerts.push(Alert::Leftovers {
                    payer: *payer,
                    count: found.len(),
                }),
                Ok(_) => {}
                Err(e) => eprintln!("leftover scan of {payer}: {e}"),
            }
        }
        if !args.keeper.is_empty() {
            let seen = acs
                .read_channel(CHANNEL)
                .map(|m| latest_heartbeats(&m))
                .map_err(|e| eprintln!("heartbeats: {e}"))
                .ok();
            alerts.extend(liveness_alerts(&args.keeper, seen.as_ref(), now));
        }

        if args.once {
            for a in &alerts {
                println!("{}", a.text());
            }
            println!("{} alert(s) at {now}", alerts.len());
            return Ok(());
        }
        let (new, resolved) = diff(&announced, &alerts);
        for a in &new {
            let text = a.text();
            println!("{text}");
            for target in [
                Target::Channel(CHANNEL.into()),
                Target::Direct(args.alert_to.clone()),
            ] {
                if let Err(e) = acs.send(&target, &text) {
                    eprintln!("alert not sent: {e}");
                }
            }
        }
        for key in &resolved {
            let text = format!("RESOLVED {key}");
            println!("{text}");
            if let Err(e) = acs.send(&Target::Channel(CHANNEL.into()), &text) {
                eprintln!("resolution not sent: {e}");
            }
        }
        announced = alerts.iter().map(Alert::key).collect();
        std::thread::sleep(Duration::from_secs(args.interval_s));
    }
}

fn main() -> ExitCode {
    match run(&Args::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
