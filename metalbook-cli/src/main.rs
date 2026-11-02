//! `metalbook-admin`: see the crate documentation and `docs/devnet.md`.

use std::process::ExitCode;

use clap::{Parser, Subcommand};
use metalbook_cli::listing;
use metalbook_cli::params::{parse_address, parse_kind, parse_underlying, parse_usd, ParamsFile};
use metalbook_cli::rpc::Cluster;
use metalbook_cli::Rpc;
use metalbook_cli::{
    calendar_json, claim_writer_ix, close_series_ixs, config_json, create_series_ix, describe,
    init_config_ix, mint_ixs, read_calendar, read_config, read_keypair, read_series, redeem_ix,
    series_json, set_calendar_ix, set_config_ix, settle_ix, submit, CliError, SeriesArgs,
};
use metalbook_sdk::pda::{self, SeriesSpec};
use solana_instruction::Instruction;
use solana_keypair::Keypair;
use solana_signer::Signer;

/// Admin and listing commands for the metalbook program.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// JSON-RPC URL. Never printed, so a provider key in it stays private.
    #[arg(long, env = "SOLANA_URL", global = true, hide_env_values = true)]
    url: Option<String>,
    /// The metalbook program id.
    #[arg(long, env = "METALBOOK_PROGRAM_ID", global = true)]
    program_id: Option<String>,
    /// Keypair file of the signer (upgrade authority, admin or payer).
    #[arg(long, env = "METALBOOK_KEYPAIR", global = true)]
    keypair: Option<String>,
    /// Print the instruction instead of signing and sending it.
    #[arg(long, global = true)]
    dry_run: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// `init_config`: signed by the program's upgrade authority, who becomes admin.
    InitConfig {
        /// The USDC mint.
        #[arg(long)]
        usdc_mint: String,
        /// The parameters file (JSON, the shape `show-config` prints under `params`).
        #[arg(long)]
        params: String,
    },
    /// `set_config`: signed by the admin.
    SetConfig {
        /// The parameters file.
        #[arg(long)]
        params: String,
        /// Hand admin to this key (default: keep the signer).
        #[arg(long)]
        new_admin: Option<String>,
    },
    /// `set_calendar`: replaces an underlying's expiries; signed by the admin.
    SetCalendar {
        /// `xau` or `xag`.
        #[arg(long)]
        underlying: String,
        /// Strictly increasing Unix seconds, comma separated.
        #[arg(long, value_delimiter = ',', allow_negative_numbers = true)]
        expiries: Vec<i64>,
    },
    /// `create_series`: lists a series; the signer pays its rent.
    CreateSeries {
        #[command(flatten)]
        terms: Terms,
    },
    /// The series address of some terms; needs `--series-multiplier`, reads nothing.
    SeriesAddress {
        #[command(flatten)]
        terms: Terms,
    },
    /// Prints the config as JSON.
    ShowConfig,
    /// Prints a calendar as JSON.
    ShowCalendar {
        /// `xau` or `xag`.
        #[arg(long)]
        underlying: String,
    },
    /// `mint`: the signer writes `contracts` of a series (collateral from its USDC account) and
    /// receives as many long tokens.
    Mint {
        /// The series address.
        #[arg(long)]
        series: String,
        /// Number of contracts.
        #[arg(long)]
        contracts: u64,
    },
    /// `redeem`: the signer redeems long tokens of a settled or voided series.
    Redeem {
        /// The series address.
        #[arg(long)]
        series: String,
        /// Number of long tokens.
        #[arg(long)]
        contracts: u64,
    },
    /// `claim_writer`: the signer takes its residual collateral of a settled or voided series.
    ClaimWriter {
        /// The series address.
        #[arg(long)]
        series: String,
    },
    /// `settle`: fixes the settlement price, or voids the series, once the cascade allows it.
    Settle {
        /// The series address.
        #[arg(long)]
        series: String,
    },
    /// `close_series`: closes a settled or voided series that owes nothing; rent and any
    /// donation go to the series' rent payer (its USDC account is created first if missing,
    /// paid by the signer).
    CloseSeries {
        /// The series address.
        #[arg(long)]
        series: String,
    },
    /// Lists one week's series of an underlying: a put and a capped call (the config's first
    /// cap offset) at every grid strike within ±5% of a fresh Pyth print (Hermes `/latest`,
    /// key from `METALBOOK_HERMES_API_KEY`). Prints the plan and sends nothing without
    /// `--execute`; series that already exist are skipped.
    ListWeek {
        /// `xau` or `xag`.
        #[arg(long)]
        underlying: String,
        /// One of the calendar's expiries (default: the first at least `min_listing_s` ahead).
        #[arg(long)]
        expiry: Option<i64>,
        /// Half-width of the strike band around spot, in basis points.
        #[arg(long, default_value_t = metalbook_cli::listing::LISTING_BAND_BPS)]
        band_bps: u16,
        /// Refuse a Pyth print older than this, in seconds (market closed or feed stalled).
        #[arg(long, default_value_t = metalbook_cli::listing::MAX_PRINT_AGE_S)]
        max_age_s: i64,
        /// Refuse to list more than this many new series in one run.
        #[arg(long, default_value_t = 16)]
        max: usize,
        /// Send the transactions (default: print the plan only).
        #[arg(long)]
        execute: bool,
    },
    /// Closes every settled or voided series of the program that owes nothing
    /// (`close-series`), returning its rent to the payer recorded at listing. Prints the plan
    /// and sends nothing without `--execute`.
    Reclaim {
        /// Send the transactions (default: print the plan only).
        #[arg(long)]
        execute: bool,
    },
    /// Prints a series as JSON.
    ShowSeries {
        /// The series address.
        address: String,
    },
}

/// Series terms in dollars.
#[derive(clap::Args)]
struct Terms {
    /// `xau` or `xag`.
    #[arg(long)]
    underlying: String,
    /// `call` or `put`.
    #[arg(long)]
    kind: String,
    /// Strike in dollars per ounce, at most 6 decimals.
    #[arg(long)]
    strike: String,
    /// Expiry, Unix seconds (one of the calendar's).
    #[arg(long)]
    expiry: i64,
    /// Cap in dollars per ounce, calls only.
    #[arg(long)]
    cap: Option<String>,
    /// Multiplier in milli-ounces (default: the config's).
    #[arg(long = "series-multiplier")]
    multiplier: Option<u32>,
}

impl Terms {
    fn args(&self) -> Result<SeriesArgs, CliError> {
        Ok(SeriesArgs {
            underlying: parse_underlying(&self.underlying)?,
            kind: parse_kind(&self.kind)?,
            strike: parse_usd(&self.strike)?,
            expiry: self.expiry,
            cap: self.cap.as_deref().map(parse_usd).transpose()?.unwrap_or(0),
            multiplier: self.multiplier,
        })
    }
}

fn required<'a>(value: &'a Option<String>, what: &str) -> Result<&'a str, CliError> {
    value
        .as_deref()
        .ok_or_else(|| CliError::Usage(format!("{what} is required")))
}

fn read_params(path: &str) -> Result<ParamsFile, CliError> {
    let text =
        std::fs::read_to_string(path).map_err(|e| CliError::ParamsFile(format!("{path}: {e}")))?;
    ParamsFile::from_json(&text)
}

fn print_json(value: &serde_json::Value) -> Result<(), CliError> {
    // `{:#}` is serde_json's pretty printer, which cannot fail on a `Value`.
    println!("{value:#}");
    Ok(())
}

/// Signs and sends `instruction`, or prints it with `--dry-run`.
fn finish(
    rpc: &mut Cluster,
    instruction: Instruction,
    signer: &Keypair,
    dry_run: bool,
) -> Result<(), CliError> {
    if dry_run {
        print!("{}", describe(&instruction));
    } else {
        let signature = submit(rpc, &[instruction], signer)?;
        println!("confirmed {signature}");
    }
    Ok(())
}

fn run(cli: Cli) -> Result<(), CliError> {
    let program_id = parse_address(required(&cli.program_id, "--program-id")?)?;
    if let Command::SeriesAddress { terms } = &cli.command {
        let a = terms.args()?;
        let multiplier = a.multiplier.ok_or_else(|| {
            CliError::Usage("--series-multiplier is required (no config is read)".into())
        })?;
        let spec = SeriesSpec {
            underlying: a.underlying,
            kind: a.kind,
            strike: a.strike,
            expiry: a.expiry,
            cap: a.cap,
            multiplier,
        };
        println!("{}", pda::series(&program_id, &spec).0);
        return Ok(());
    }
    let mut rpc = Cluster::new(required(&cli.url, "--url")?);
    let signer = || read_keypair(required(&cli.keypair, "--keypair")?);
    match &cli.command {
        Command::ShowConfig => {
            let config = read_config(&mut rpc, &program_id)?;
            print_json(&config_json(&program_id, &config))
        }
        Command::ShowCalendar { underlying } => {
            let calendar = read_calendar(&mut rpc, &program_id, parse_underlying(underlying)?)?;
            print_json(&calendar_json(&program_id, &calendar))
        }
        Command::Mint { series, contracts } => {
            let signer = signer()?;
            let ixs = mint_ixs(
                &mut rpc,
                &program_id,
                &signer.pubkey(),
                &parse_address(series)?,
                *contracts,
            )?;
            if cli.dry_run {
                for i in &ixs {
                    print!("{}", describe(i));
                }
                Ok(())
            } else {
                let signature = submit(&mut rpc, &ixs, &signer)?;
                println!("confirmed {signature}");
                Ok(())
            }
        }
        Command::Settle { series } => {
            let signer = signer()?;
            let instruction = settle_ix(&program_id, &parse_address(series)?);
            finish(&mut rpc, instruction, &signer, cli.dry_run)
        }
        Command::CloseSeries { series } => {
            let signer = signer()?;
            let ixs = close_series_ixs(
                &mut rpc,
                &program_id,
                &signer.pubkey(),
                &parse_address(series)?,
            )?;
            if cli.dry_run {
                for i in &ixs {
                    print!("{}", describe(i));
                }
                Ok(())
            } else {
                let signature = submit(&mut rpc, &ixs, &signer)?;
                println!("confirmed {signature}");
                Ok(())
            }
        }
        Command::Redeem { series, contracts } => {
            let signer = signer()?;
            let instruction = redeem_ix(
                &mut rpc,
                &program_id,
                &signer.pubkey(),
                &parse_address(series)?,
                *contracts,
            )?;
            finish(&mut rpc, instruction, &signer, cli.dry_run)
        }
        Command::ClaimWriter { series } => {
            let signer = signer()?;
            let instruction = claim_writer_ix(
                &mut rpc,
                &program_id,
                &signer.pubkey(),
                &parse_address(series)?,
            )?;
            finish(&mut rpc, instruction, &signer, cli.dry_run)
        }
        Command::ShowSeries { address } => {
            let address = parse_address(address)?;
            let series = read_series(&mut rpc, &program_id, &address)?;
            print_json(&series_json(&address, &series))
        }
        Command::InitConfig { usdc_mint, params } => {
            let signer = signer()?;
            let instruction = init_config_ix(
                &program_id,
                &signer.pubkey(),
                &parse_address(usdc_mint)?,
                &read_params(params)?.to_params()?,
            );
            finish(&mut rpc, instruction, &signer, cli.dry_run)
        }
        Command::SetConfig { params, new_admin } => {
            let signer = signer()?;
            let new_admin = new_admin.as_deref().map(parse_address).transpose()?;
            let instruction = set_config_ix(
                &program_id,
                &signer.pubkey(),
                new_admin.as_ref(),
                &read_params(params)?.to_params()?,
            );
            finish(&mut rpc, instruction, &signer, cli.dry_run)
        }
        Command::SetCalendar {
            underlying,
            expiries,
        } => {
            let signer = signer()?;
            let instruction = set_calendar_ix(
                &program_id,
                &signer.pubkey(),
                parse_underlying(underlying)?,
                expiries,
            )?;
            finish(&mut rpc, instruction, &signer, cli.dry_run)
        }
        Command::CreateSeries { terms } => {
            let signer = signer()?;
            let (instruction, series) =
                create_series_ix(&mut rpc, &program_id, &signer.pubkey(), &terms.args()?)?;
            println!("series {series}");
            finish(&mut rpc, instruction, &signer, cli.dry_run)
        }
        Command::SeriesAddress { .. } => Ok(()),
        Command::ListWeek {
            underlying,
            expiry,
            band_bps,
            max_age_s,
            max,
            execute,
        } => {
            let underlying = parse_underlying(underlying)?;
            let index = usize::from(underlying);
            let config = read_config(&mut rpc, &program_id)?;
            let calendar = read_calendar(&mut rpc, &program_id, underlying)?;
            let expiries: Vec<i64> = calendar
                .expiries
                .iter()
                .take(usize::from(calendar.count))
                .copied()
                .collect();
            let now = unix_now()?;
            let expiry = listing::listing_expiry(&expiries, now, config.min_listing_s, *expiry)?;
            let feed = *config
                .feed_id
                .get(index)
                .ok_or_else(|| CliError::Usage("unknown underlying".into()))?;
            let expo = i32::from(
                *config
                    .expo
                    .get(index)
                    .ok_or_else(|| CliError::Usage("unknown underlying".into()))?,
            );
            let hermes = metalbook_keeper::HermesConfig::from_env()
                .map_err(|e| CliError::Usage(e.to_string()))?;
            let source =
                metalbook_keeper::HermesSource::new(hermes, metalbook_keeper::UreqHttp::new());
            let print = source
                .latest(&feed)
                .map_err(|e| CliError::Rpc(e.to_string()))?
                .ok_or_else(|| CliError::Usage("Hermes has no latest print".into()))?;
            listing::fresh_print(&print, now, *max_age_s, expo)?;
            let spot = listing::spot_atoms(print.price, print.expo)?;
            let tick = *config
                .strike_tick
                .get(index)
                .ok_or_else(|| CliError::Usage("unknown underlying".into()))?;
            let cap_bps = config.cap_bps.first().copied().unwrap_or(0);
            let plan = listing::week_series(underlying, expiry, spot, tick, *band_bps, cap_bps)?;
            println!(
                "spot {} USD (published {}, {} s old); expiry {expiry}; {} series in the band",
                listing::usd(spot),
                print.publish_time,
                now.saturating_sub(print.publish_time),
                plan.len()
            );
            let signer = signer()?;
            let mut new = Vec::new();
            for args in &plan {
                let (instruction, series) =
                    create_series_ix(&mut rpc, &program_id, &signer.pubkey(), args)?;
                let exists = rpc.account(&series)?.is_some();
                let (kind, cu) = if args.kind == 0 {
                    (
                        format!("call cap {}", listing::usd(args.cap)),
                        listing::CREATE_CALL_CU,
                    )
                } else {
                    ("put".to_string(), listing::CREATE_PUT_CU)
                };
                println!(
                    "{series} {kind} strike {} {}",
                    listing::usd(args.strike),
                    if exists {
                        "exists, skipped".to_string()
                    } else {
                        format!("new, {cu} CU")
                    }
                );
                if !exists {
                    new.push(instruction);
                }
            }
            if new.len() > *max {
                return Err(CliError::Usage(format!(
                    "{} new series exceed --max {max}; nothing sent",
                    new.len()
                )));
            }
            if !*execute {
                println!(
                    "{} new series; dry run, nothing sent (add --execute)",
                    new.len()
                );
                return Ok(());
            }
            for instruction in new {
                let signature = submit(&mut rpc, &[instruction], &signer)?;
                println!("confirmed {signature}");
            }
            Ok(())
        }
        Command::Reclaim { execute } => {
            let signer = signer()?;
            let len = u64::try_from(metalbook_keeper::SERIES_LEN)
                .map_err(|_| CliError::Usage("series size".into()))?;
            let mut closable = Vec::new();
            for address in rpc.accounts_of_size(&program_id, len)? {
                let Ok(s) = read_series(&mut rpc, &program_id, &address) else {
                    continue;
                };
                if listing::reclaimable(&s) {
                    println!(
                        "{address} {} owes nothing: closable",
                        if s.state == 3 { "voided" } else { "settled" }
                    );
                    closable.push(address);
                }
            }
            if !*execute {
                println!(
                    "{} closable; dry run, nothing sent (add --execute)",
                    closable.len()
                );
                return Ok(());
            }
            for series in closable {
                let ixs = close_series_ixs(&mut rpc, &program_id, &signer.pubkey(), &series)?;
                let signature = submit(&mut rpc, &ixs, &signer)?;
                println!("{series} closed {signature}");
            }
            Ok(())
        }
    }
}

/// Seconds since the Unix epoch, from the system clock.
fn unix_now() -> Result<i64, CliError> {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| CliError::Usage("system clock before 1970".into()))?;
    i64::try_from(d.as_secs()).map_err(|_| CliError::Usage("system clock out of range".into()))
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
