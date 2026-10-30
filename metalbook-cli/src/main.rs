//! `metalbook-admin`: see the crate documentation and `docs/devnet.md`.

use std::process::ExitCode;

use clap::{Parser, Subcommand};
use metalbook_cli::params::{parse_address, parse_kind, parse_underlying, parse_usd, ParamsFile};
use metalbook_cli::rpc::Cluster;
use metalbook_cli::{
    calendar_json, claim_writer_ix, config_json, create_series_ix, describe, init_config_ix,
    mint_ixs, read_calendar, read_config, read_keypair, read_series, redeem_ix, series_json,
    set_calendar_ix, set_config_ix, submit, CliError, SeriesArgs,
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
    }
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
