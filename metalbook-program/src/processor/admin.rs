//! `init_config`, `set_config` and `set_calendar`.

use metalbook_core::{CoreError, OracleParams, Underlying};
use pinocchio::cpi::Seed;
use pinocchio::{AccountView, Address, ProgramResult};
use pinocchio_token::state::Mint;

use crate::data::Reader;
use crate::error::MetalbookError;
use crate::pda::{self, CALENDAR_SEED, CONFIG_SEED, UPGRADEABLE_LOADER_ID};
use crate::state::{self, AccountTag, Calendar, Config, CALENDAR_CAPACITY, CAP_GRID_LEN, VERSION};
use crate::validate;

/// Largest accepted cap offset: 600% above the strike.
pub const MAX_CAP_BPS: u16 = 60_000;

/// Config parameters the admin sets, as encoded after the tag of `init_config` and
/// `set_config`, all little endian (165 bytes): `min_listing_s: u32`, `strike_tick: [u64; 2]`,
/// `multiplier: [u32; 2]`, `cap_bps: [u16; 4]`, `receiver_program: [u8; 32]`,
/// `feed_id: [[u8; 32]; 2]`, `expo: [i8; 2]`, `max_conf_bps: u16`, `slot_tolerance_s: u16`,
/// `min_samples: u16`, `k_next_print: u8`, `window_s: u32`, `slot_s: u32`,
/// `challenge_delay_s: u32`, `fallback_lookback_s: u32`, `hard_deadline_s: u32`,
/// `redeem_deadline_s: u32`. Index 0 of the per-underlying arrays is XAU, 1 is XAG.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Params {
    min_listing_s: u32,
    strike_tick: [u64; 2],
    multiplier: [u32; 2],
    cap_bps: [u16; CAP_GRID_LEN],
    receiver_program: [u8; 32],
    feed_id: [[u8; 32]; 2],
    expo: [i8; 2],
    max_conf_bps: u16,
    slot_tolerance_s: u16,
    min_samples: u16,
    k_next_print: u8,
    window_s: u32,
    slot_s: u32,
    challenge_delay_s: u32,
    fallback_lookback_s: u32,
    hard_deadline_s: u32,
    redeem_deadline_s: u32,
}

impl Params {
    fn read(r: &mut Reader) -> Result<Self, MetalbookError> {
        let p = Self {
            min_listing_s: r.u32()?,
            strike_tick: [r.u64()?, r.u64()?],
            multiplier: [r.u32()?, r.u32()?],
            cap_bps: [r.u16()?, r.u16()?, r.u16()?, r.u16()?],
            receiver_program: r.bytes32()?,
            feed_id: [r.bytes32()?, r.bytes32()?],
            expo: [r.i8()?, r.i8()?],
            max_conf_bps: r.u16()?,
            slot_tolerance_s: r.u16()?,
            min_samples: r.u16()?,
            k_next_print: r.u8()?,
            window_s: r.u32()?,
            slot_s: r.u32()?,
            challenge_delay_s: r.u32()?,
            fallback_lookback_s: r.u32()?,
            hard_deadline_s: r.u32()?,
            redeem_deadline_s: r.u32()?,
        };
        p.validate()?;
        Ok(p)
    }

    /// Grids as in milestone 2; oracle terms through `OracleParams::try_new` for each
    /// underlying's exponent (so every series copies terms that are already valid); a
    /// confidence bound in `1..=10_000` bps; non-zero receiver, feed ids and redeem deadline.
    fn validate(&self) -> Result<(), MetalbookError> {
        let ticks_ok = self.strike_tick.iter().all(|&t| t > 0);
        let mults_ok = self.multiplier.iter().all(|&m| m > 0);
        let caps_ok =
            self.cap_bps.iter().any(|&b| b > 0) && self.cap_bps.iter().all(|&b| b <= MAX_CAP_BPS);
        let oracle_ok = self.expo.iter().all(|&expo| self.oracle(expo).is_ok())
            && (1..=10_000).contains(&self.max_conf_bps)
            && self.receiver_program != [0; 32]
            && self.feed_id.iter().all(|f| f != &[0; 32])
            && self.redeem_deadline_s > 0;
        if ticks_ok && mults_ok && caps_ok && oracle_ok {
            Ok(())
        } else {
            Err(MetalbookError::InvalidConfig)
        }
    }

    fn oracle(&self, expo: i8) -> Result<OracleParams, CoreError> {
        OracleParams::try_new(
            self.window_s,
            self.slot_s,
            self.slot_tolerance_s,
            self.min_samples,
            self.k_next_print,
            self.challenge_delay_s,
            self.fallback_lookback_s,
            self.hard_deadline_s,
            expo,
        )
    }

    fn store(&self, config: &mut Config) {
        config.min_listing_s = self.min_listing_s;
        config.strike_tick = self.strike_tick;
        config.multiplier = self.multiplier;
        config.cap_bps = self.cap_bps;
        config.receiver_program = self.receiver_program;
        config.feed_id = self.feed_id;
        config.expo = self.expo;
        config.max_conf_bps = self.max_conf_bps;
        config.slot_tolerance_s = self.slot_tolerance_s;
        config.min_samples = self.min_samples;
        config.k_next_print = self.k_next_print;
        config.window_s = self.window_s;
        config.slot_s = self.slot_s;
        config.challenge_delay_s = self.challenge_delay_s;
        config.fallback_lookback_s = self.fallback_lookback_s;
        config.hard_deadline_s = self.hard_deadline_s;
        config.redeem_deadline_s = self.redeem_deadline_s;
    }
}

/// `init_config`. Accounts: `[admin (signer, writable), config (writable), program_data,
/// usdc_mint, system_program]`. Data: the config parameters (165 bytes, layout on `Params`
/// in this module's source).
///
/// Only the program's upgrade authority may initialize, so nobody can front-run the
/// deployment and take the admin role.
///
/// # Errors
///
/// See the checks; `AlreadyInitialized` if the config exists.
#[inline(never)]
pub fn init_config(
    program_id: &Address,
    accounts: &mut [AccountView],
    data: &[u8],
) -> ProgramResult {
    let [admin, config, program_data, usdc_mint, system_program, ..] = accounts else {
        return Err(pinocchio::error::ProgramError::NotEnoughAccountKeys);
    };
    let mut r = Reader::new(data);
    let params = Params::read(&mut r)?;
    r.finish()?;

    validate::signer(admin)?;
    validate::writable(admin)?;
    validate::writable(config)?;
    validate::program(system_program, &pinocchio_system::ID)?;
    check_upgrade_authority(program_data, program_id, admin.address())?;

    let (config_address, bump) = pda::find(&[CONFIG_SEED], program_id)?;
    validate::address(config, &config_address, MetalbookError::InvalidPda)?;

    {
        let mint = Mint::from_account_view(usdc_mint).map_err(|_| MetalbookError::InvalidMint)?;
        if !mint.is_initialized() {
            return Err(MetalbookError::InvalidMint.into());
        }
    }

    let bump_seed = [bump];
    let seeds = [Seed::from(CONFIG_SEED), Seed::from(&bump_seed)];
    validate::create_pda_account(
        admin,
        config,
        core::mem::size_of::<Config>(),
        program_id,
        &seeds,
    )?;

    let mut bytes = config.try_borrow_mut()?;
    let c = state::init_mut::<Config>(&mut bytes)?;
    c.tag = AccountTag::Config as u8;
    c.version = VERSION;
    c.bump = bump;
    c.admin = admin.address().to_bytes();
    c.usdc_mint = usdc_mint.address().to_bytes();
    c.token_program = pinocchio_token::ID.to_bytes();
    params.store(c);
    Ok(())
}

/// `set_config`. Accounts: `[admin (signer), config (writable)]`. Data: `new_admin: [u8; 32]`
/// then the parameters of `init_config`. The USDC mint and token program never change; live series keep their own
/// copy of every term they use (`docs/PLAN.md` 4.5).
///
/// # Errors
///
/// `NotAuthority` if the signer is not the admin; see the checks.
#[inline(never)]
pub fn set_config(
    program_id: &Address,
    accounts: &mut [AccountView],
    data: &[u8],
) -> ProgramResult {
    let [admin, config, ..] = accounts else {
        return Err(pinocchio::error::ProgramError::NotEnoughAccountKeys);
    };
    let mut r = Reader::new(data);
    let new_admin = r.bytes32()?;
    let params = Params::read(&mut r)?;
    r.finish()?;

    validate::signer(admin)?;
    validate::writable(config)?;
    validate::owned_by(config, program_id)?;
    let config_address = Address::new_from_array(config.address().to_bytes());
    let mut bytes = config.try_borrow_mut()?;
    let c = state::load_mut::<Config>(&mut bytes)?;
    pda::check(&[CONFIG_SEED], c.bump, program_id, &config_address)?;
    if c.admin != admin.address().to_bytes() {
        return Err(MetalbookError::NotAuthority.into());
    }
    c.admin = new_admin;
    params.store(c);
    Ok(())
}

/// `set_calendar`. Accounts: `[admin (signer, writable), config, calendar (writable),
/// system_program]`. Data: `underlying: u8`, `count: u8`, then `count` expiries (`i64`),
/// strictly increasing. Creates the calendar on first use; replaces its list afterwards.
/// Existing series are not affected: their expiry is part of their own account.
///
/// # Errors
///
/// `NotAuthority` if the signer is not the admin; `InvalidInstructionData` for more than
/// [`CALENDAR_CAPACITY`] entries or a list that is not strictly increasing.
#[inline(never)]
pub fn set_calendar(
    program_id: &Address,
    accounts: &mut [AccountView],
    data: &[u8],
) -> ProgramResult {
    let [admin, config, calendar, system_program, ..] = accounts else {
        return Err(pinocchio::error::ProgramError::NotEnoughAccountKeys);
    };
    let mut r = Reader::new(data);
    let underlying =
        Underlying::try_from(r.u8()?).map_err(|_| MetalbookError::InvalidInstructionData)?;
    let count = r.u8()?;
    if usize::from(count) > CALENDAR_CAPACITY {
        return Err(MetalbookError::InvalidInstructionData.into());
    }
    let mut expiries = [0i64; CALENDAR_CAPACITY];
    let mut previous: Option<i64> = None;
    for slot in expiries.iter_mut().take(usize::from(count)) {
        let e = r.i64()?;
        if previous.is_some_and(|p| e <= p) {
            return Err(MetalbookError::InvalidInstructionData.into());
        }
        previous = Some(e);
        *slot = e;
    }
    r.finish()?;

    validate::signer(admin)?;
    validate::writable(calendar)?;
    validate::program(system_program, &pinocchio_system::ID)?;
    load_config_as_admin(program_id, config, admin.address())?;

    let underlying_seed = [underlying as u8];
    if calendar.is_data_empty() {
        validate::writable(admin)?;
        let (address, bump) = pda::find(&[CALENDAR_SEED, &underlying_seed], program_id)?;
        validate::address(calendar, &address, MetalbookError::InvalidPda)?;
        let bump_seed = [bump];
        let seeds = [
            Seed::from(CALENDAR_SEED),
            Seed::from(&underlying_seed),
            Seed::from(&bump_seed),
        ];
        validate::create_pda_account(
            admin,
            calendar,
            core::mem::size_of::<Calendar>(),
            program_id,
            &seeds,
        )?;
        let mut bytes = calendar.try_borrow_mut()?;
        let c = state::init_mut::<Calendar>(&mut bytes)?;
        c.tag = AccountTag::Calendar as u8;
        c.version = VERSION;
        c.bump = bump;
        c.underlying = underlying as u8;
    }

    validate::owned_by(calendar, program_id)?;
    let calendar_address = Address::new_from_array(calendar.address().to_bytes());
    let mut bytes = calendar.try_borrow_mut()?;
    let c = state::load_mut::<Calendar>(&mut bytes)?;
    pda::check(
        &[CALENDAR_SEED, &underlying_seed],
        c.bump,
        program_id,
        &calendar_address,
    )?;
    if c.underlying != underlying as u8 {
        return Err(MetalbookError::InvalidAccountData.into());
    }
    c.count = count;
    c.expiries = expiries;
    Ok(())
}

/// Loads the config, checks its PDA and that `signer` is its admin. Returns a copy.
fn load_config_as_admin(
    program_id: &Address,
    config: &AccountView,
    signer: &Address,
) -> Result<Config, MetalbookError> {
    let c = load_config(program_id, config)?;
    if c.admin != signer.to_bytes() {
        return Err(MetalbookError::NotAuthority);
    }
    Ok(c)
}

/// Loads the config and checks its owner and PDA. Returns a copy.
///
/// # Errors
///
/// `InvalidAccountOwner`, `InvalidAccountData` or `InvalidPda`.
#[inline(never)]
pub fn load_config(program_id: &Address, config: &AccountView) -> Result<Config, MetalbookError> {
    validate::owned_by(config, program_id)?;
    let bytes = config
        .try_borrow()
        .map_err(|_| MetalbookError::InvalidAccountData)?;
    let c = *state::load::<Config>(&bytes)?;
    pda::check(&[CONFIG_SEED], c.bump, program_id, config.address())?;
    Ok(c)
}

/// `program_data` is this program's `ProgramData` account and names `authority` as upgrade
/// authority. Layout (bincode): `u32` variant `3`, `slot: u64`, then `Option<Address>`
/// (tag byte at offset 12, address at 13..45).
#[inline(never)]
fn check_upgrade_authority(
    program_data: &AccountView,
    program_id: &Address,
    authority: &Address,
) -> Result<(), MetalbookError> {
    validate::owned_by(program_data, &UPGRADEABLE_LOADER_ID)?;
    let (expected, _) = pda::find(&[program_id.as_ref()], &UPGRADEABLE_LOADER_ID)?;
    validate::address(program_data, &expected, MetalbookError::InvalidPda)?;
    let data = program_data
        .try_borrow()
        .map_err(|_| MetalbookError::InvalidAccountData)?;
    let variant = data.get(0..4).ok_or(MetalbookError::InvalidAccountData)?;
    let has_authority = data.get(12).ok_or(MetalbookError::InvalidAccountData)?;
    let key = data.get(13..45).ok_or(MetalbookError::InvalidAccountData)?;
    if variant != [3, 0, 0, 0] {
        return Err(MetalbookError::InvalidAccountData);
    }
    if *has_authority != 1 || key != authority.as_ref() {
        return Err(MetalbookError::NotAuthority);
    }
    Ok(())
}
