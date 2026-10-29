//! Thin admin CLI for metalbook (`metalbook-admin`): the admin and listing instructions
//! (`init_config`, `set_config`, `set_calendar`, `create_series`) built by `metalbook-sdk`,
//! signed with a keypair file and sent over JSON-RPC, plus decoded reads of the config,
//! calendars and series.
//!
//! The commands are written against the [`Rpc`] trait: [`rpc::JsonRpc`] is the network
//! implementation, and the LiteSVM tests drive the same commands through an in-process one.

pub mod error;
pub mod params;
pub mod rpc;

use metalbook_sdk::core::accounts::{Calendar, Config, Series};
use metalbook_sdk::ix::ConfigParams;
use metalbook_sdk::pda::{self, SeriesSpec};
use metalbook_sdk::{decode, ix, Address};
use serde_json::{json, Value};
use solana_hash::Hash;
use solana_instruction::Instruction;
use solana_keypair::Keypair;
use solana_signer::Signer;
use solana_transaction::Transaction;

pub use error::CliError;
use params::{format_usd, kind_name, underlying_name, ParamsFile};

/// Most expiries a calendar holds (`metalbook_core::accounts::CALENDAR_CAPACITY`).
pub const CALENDAR_CAPACITY: usize = 64;

/// An account as the RPC returns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountInfo {
    /// The owning program.
    pub owner: Address,
    /// The account data.
    pub data: Vec<u8>,
}

/// What the commands need from a Solana node.
pub trait Rpc {
    /// A recent blockhash.
    ///
    /// # Errors
    ///
    /// [`CliError`] from the node or the transport.
    fn latest_blockhash(&mut self) -> Result<Hash, CliError>;

    /// The account at `address`, `None` if it does not exist.
    ///
    /// # Errors
    ///
    /// [`CliError`] from the node or the transport.
    fn account(&mut self, address: &Address) -> Result<Option<AccountInfo>, CliError>;

    /// Sends a signed transaction and waits until it is confirmed.
    ///
    /// # Errors
    ///
    /// [`CliError`] when it is rejected, fails or is not confirmed in time.
    fn send(&mut self, tx: &Transaction) -> Result<(), CliError>;
}

/// Reads a Solana CLI keypair file (a JSON array of 64 bytes). The error names the path, never
/// the content.
///
/// # Errors
///
/// [`CliError::Keypair`].
pub fn read_keypair(path: &str) -> Result<Keypair, CliError> {
    let mut file = std::fs::File::open(path).map_err(|_| CliError::Keypair {
        path: path.to_owned(),
        what: "cannot be opened",
    })?;
    solana_keypair::read_keypair(&mut file).map_err(|_| CliError::Keypair {
        path: path.to_owned(),
        what: "is not a JSON array of 64 bytes forming a valid keypair",
    })
}

/// Signs `instructions` with `signer` as fee payer and only signer, sends them and waits for
/// confirmation. Returns the transaction signature.
///
/// # Errors
///
/// [`CliError`] from signing, the node or the transport.
pub fn submit<R: Rpc>(
    rpc: &mut R,
    instructions: &[Instruction],
    signer: &Keypair,
) -> Result<String, CliError> {
    let blockhash = rpc.latest_blockhash()?;
    let mut tx = Transaction::new_with_payer(instructions, Some(&signer.pubkey()));
    tx.try_sign(&[signer], blockhash)
        .map_err(|_| CliError::Encode("signing (the instructions need another signer)"))?;
    let signature = tx
        .signatures
        .first()
        .map(ToString::to_string)
        .ok_or(CliError::Encode("no signature"))?;
    rpc.send(&tx)?;
    Ok(signature)
}

/// A human-readable dump of an instruction, for `--dry-run`.
#[must_use]
pub fn describe(instruction: &Instruction) -> String {
    let mut out = format!("program {}\naccounts\n", instruction.program_id);
    for (i, meta) in instruction.accounts.iter().enumerate() {
        let flags = match (meta.is_signer, meta.is_writable) {
            (true, true) => " signer, writable",
            (true, false) => " signer",
            (false, true) => " writable",
            (false, false) => "",
        };
        out.push_str(&format!("  {i:>2} {}{flags}\n", meta.pubkey));
    }
    let hex: String = instruction
        .data
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    out.push_str(&format!("data ({} bytes) {hex}\n", instruction.data.len()));
    out
}

fn fetch<R: Rpc>(rpc: &mut R, address: &Address) -> Result<AccountInfo, CliError> {
    rpc.account(address)?
        .ok_or_else(|| CliError::AccountNotFound(address.to_string()))
}

/// The live config.
///
/// # Errors
///
/// [`CliError::AccountNotFound`] before `init_config`, [`CliError::Decode`] for a foreign
/// account, or a node error.
pub fn read_config<R: Rpc>(rpc: &mut R, program_id: &Address) -> Result<Config, CliError> {
    let address = pda::config(program_id).0;
    let info = fetch(rpc, &address)?;
    decode::config(program_id, &info.owner, &info.data).map_err(|_| CliError::Decode {
        address: address.to_string(),
        what: "config",
    })
}

/// The calendar of `underlying`.
///
/// # Errors
///
/// As [`read_config`].
pub fn read_calendar<R: Rpc>(
    rpc: &mut R,
    program_id: &Address,
    underlying: u8,
) -> Result<Calendar, CliError> {
    let address = pda::calendar(program_id, underlying).0;
    let info = fetch(rpc, &address)?;
    decode::calendar(program_id, &info.owner, &info.data).map_err(|_| CliError::Decode {
        address: address.to_string(),
        what: "calendar",
    })
}

/// The series at `address`.
///
/// # Errors
///
/// As [`read_config`].
pub fn read_series<R: Rpc>(
    rpc: &mut R,
    program_id: &Address,
    address: &Address,
) -> Result<Series, CliError> {
    let info = fetch(rpc, address)?;
    decode::series(program_id, &info.owner, &info.data).map_err(|_| CliError::Decode {
        address: address.to_string(),
        what: "series",
    })
}

/// `init_config`, signed by the program's upgrade authority, who becomes admin.
#[must_use]
pub fn init_config_ix(
    program_id: &Address,
    authority: &Address,
    usdc_mint: &Address,
    params: &ConfigParams,
) -> Instruction {
    ix::init_config(program_id, authority, usdc_mint, params)
}

/// `set_config`, signed by the admin; `new_admin` defaults to the admin itself.
#[must_use]
pub fn set_config_ix(
    program_id: &Address,
    admin: &Address,
    new_admin: Option<&Address>,
    params: &ConfigParams,
) -> Instruction {
    ix::set_config(program_id, admin, new_admin.unwrap_or(admin), params)
}

/// `set_calendar`, signed by the admin. The expiries replace the calendar and must be strictly
/// increasing, at most [`CALENDAR_CAPACITY`]; both are checked here too so a bad list fails
/// before signing.
///
/// # Errors
///
/// [`CliError::Usage`] for too many or unordered expiries.
pub fn set_calendar_ix(
    program_id: &Address,
    admin: &Address,
    underlying: u8,
    expiries: &[i64],
) -> Result<Instruction, CliError> {
    if expiries.len() > CALENDAR_CAPACITY {
        return Err(CliError::Usage(format!(
            "{} expiries; a calendar holds at most {CALENDAR_CAPACITY}",
            expiries.len()
        )));
    }
    if expiries.windows(2).any(|w| w.first() >= w.get(1)) {
        return Err(CliError::Usage(
            "expiries must be strictly increasing".into(),
        ));
    }
    Ok(ix::set_calendar(program_id, admin, underlying, expiries))
}

/// The terms of a new series as given on the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeriesArgs {
    /// `Underlying` discriminant.
    pub underlying: u8,
    /// `OptionKind` discriminant.
    pub kind: u8,
    /// Strike, USDC atoms per ounce.
    pub strike: u64,
    /// Expiry, Unix seconds.
    pub expiry: i64,
    /// Cap, USDC atoms per ounce (0 for a put).
    pub cap: u64,
    /// Multiplier in milli-ounces; `None` takes the config's for the underlying.
    pub multiplier: Option<u32>,
}

/// `create_series`, paid by `payer`. Reads the config for the USDC mint and, unless given, the
/// multiplier. Returns the instruction and the series address.
///
/// # Errors
///
/// As [`read_config`].
pub fn create_series_ix<R: Rpc>(
    rpc: &mut R,
    program_id: &Address,
    payer: &Address,
    args: &SeriesArgs,
) -> Result<(Instruction, Address), CliError> {
    let config = read_config(rpc, program_id)?;
    let multiplier = match args.multiplier {
        Some(m) => m,
        None => config
            .multiplier
            .get(usize::from(args.underlying))
            .copied()
            .ok_or_else(|| CliError::Usage("unknown underlying".into()))?,
    };
    let spec = SeriesSpec {
        underlying: args.underlying,
        kind: args.kind,
        strike: args.strike,
        expiry: args.expiry,
        cap: args.cap,
        multiplier,
    };
    let usdc_mint = Address::new_from_array(config.usdc_mint);
    let series = pda::series(program_id, &spec).0;
    Ok((
        ix::create_series(program_id, payer, &usdc_mint, &spec),
        series,
    ))
}

/// The config as JSON: admin, mint and the parameters in the `ParamsFile` shape.
#[must_use]
pub fn config_json(program_id: &Address, c: &Config) -> Value {
    json!({
        "address": pda::config(program_id).0.to_string(),
        "admin": Address::new_from_array(c.admin).to_string(),
        "usdc_mint": Address::new_from_array(c.usdc_mint).to_string(),
        "params": ParamsFile::from_config(c),
    })
}

/// A calendar as JSON.
#[must_use]
pub fn calendar_json(program_id: &Address, c: &Calendar) -> Value {
    let expiries: Vec<i64> = c
        .expiries
        .iter()
        .take(usize::from(c.count))
        .copied()
        .collect();
    json!({
        "address": pda::calendar(program_id, c.underlying).0.to_string(),
        "underlying": underlying_name(c.underlying),
        "expiries": expiries,
    })
}

/// A series as JSON, dollar amounts as decimal strings.
#[must_use]
pub fn series_json(address: &Address, s: &Series) -> Value {
    let state = match s.state {
        0 => "active",
        1 => "expired",
        2 => "settled",
        3 => "voided",
        _ => "unknown",
    };
    json!({
        "address": address.to_string(),
        "underlying": underlying_name(s.underlying),
        "kind": kind_name(s.kind),
        "strike_usd": format_usd(s.strike),
        "cap_usd": format_usd(s.cap),
        "expiry": s.expiry,
        "multiplier_milli_oz": s.multiplier,
        "state": state,
        "long_mint": Address::new_from_array(s.long_mint).to_string(),
        "vault": Address::new_from_array(s.vault).to_string(),
        "open": s.open,
        "longs_outstanding": s.longs_outstanding,
        "writers_unclaimed": s.writers_unclaimed,
        "payout_atoms": s.payout,
        "residual_atoms": s.residual,
        "vault_owed_atoms": s.vault_owed,
    })
}
