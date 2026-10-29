//! `create_series`.

use metalbook_core::{
    OptionKind, OuncesPerContract, SeriesKey, SeriesTerms, StrikeUsd, Underlying, UnixTimestamp,
};
use pinocchio::cpi::Seed;
use pinocchio::sysvars::clock::Clock;
use pinocchio::sysvars::Sysvar;
use pinocchio::{AccountView, Address, ProgramResult};
use pinocchio_associated_token_account::instructions::CreateIdempotent;
use pinocchio_token::instructions::InitializeMint2;
use pinocchio_token::state::Mint;

use crate::data::Reader;
use crate::error::MetalbookError;
use crate::pda::{self, SeriesSeeds, CALENDAR_SEED, LONG_SEED};
use crate::processor::admin::load_config;
use crate::state::{self, AccountTag, Calendar, Config, Series, VERSION};
use crate::validate;

/// Size of an SPL Token mint account.
const MINT_LEN: usize = 82;

/// `create_series`. Accounts: `[payer (signer, writable), config, calendar, series
/// (writable), long_mint (writable), vault (writable), usdc_mint, token_program,
/// associated_token_program, system_program]`. Data: `underlying: u8`, `kind: u8`,
/// `strike: u64`, `expiry: i64`, `cap: u64`, `multiplier: u32` (30 bytes).
///
/// Creates the series account, its long mint (decimals 0, mint authority the series, no
/// freeze authority) and its USDC vault (the series' associated token account). Anyone may
/// list a series inside the calendar and the grids.
///
/// # Errors
///
/// See `docs/PLAN.md` 3.3.
#[inline(never)]
pub fn create_series(
    program_id: &Address,
    accounts: &mut [AccountView],
    data: &[u8],
) -> ProgramResult {
    let [payer, config, calendar, series, long_mint, vault, usdc_mint, token_program, ata_program, system_program, ..] =
        accounts
    else {
        return Err(pinocchio::error::ProgramError::NotEnoughAccountKeys);
    };
    let mut r = Reader::new(data);
    let underlying_raw = r.u8()?;
    let kind_raw = r.u8()?;
    let strike = r.u64()?;
    let expiry = r.i64()?;
    let cap = r.u64()?;
    let multiplier = r.u32()?;
    r.finish()?;
    let underlying =
        Underlying::try_from(underlying_raw).map_err(|_| MetalbookError::InvalidInstructionData)?;
    let kind =
        OptionKind::try_from(kind_raw).map_err(|_| MetalbookError::InvalidInstructionData)?;

    validate::signer(payer)?;
    validate::writable(payer)?;
    validate::writable(series)?;
    validate::writable(long_mint)?;
    validate::writable(vault)?;
    validate::program(system_program, &pinocchio_system::ID)?;
    validate::program(ata_program, &pinocchio_associated_token_account::ID)?;

    let cfg = load_config(program_id, config)?;
    validate::program(token_program, &Address::new_from_array(cfg.token_program))?;
    validate::address(
        usdc_mint,
        &Address::new_from_array(cfg.usdc_mint),
        MetalbookError::InvalidMint,
    )?;

    // Terms: zero strike, cap rule and a representable maximum payout (metalbook-core).
    let key = SeriesKey::try_new(
        underlying,
        kind,
        StrikeUsd::new(strike),
        UnixTimestamp::new(expiry),
    )
    .map_err(MetalbookError::from)?;
    let multiplier_checked =
        OuncesPerContract::try_new(multiplier).map_err(MetalbookError::from)?;
    let terms = SeriesTerms::try_new(key, multiplier_checked, StrikeUsd::new(cap))
        .map_err(MetalbookError::from)?;
    check_grids(&cfg, &terms)?;

    // Calendar and listing time.
    check_calendar(program_id, calendar, underlying, expiry)?;
    let now = Clock::get()?.unix_timestamp;
    let earliest = now
        .checked_add(i64::from(cfg.min_listing_s))
        .ok_or(MetalbookError::Overflow)?;
    if expiry < earliest {
        return Err(MetalbookError::ListingTooLate.into());
    }

    // Series, long mint and vault addresses.
    let seeds = SeriesSeeds::new(underlying_raw, kind_raw, strike, expiry, cap, multiplier);
    let (series_address, series_bump) = pda::find(&seeds.as_slices(), program_id)?;
    validate::address(series, &series_address, MetalbookError::InvalidPda)?;
    let (long_address, long_bump) = pda::find(&[LONG_SEED, series_address.as_ref()], program_id)?;
    validate::address(long_mint, &long_address, MetalbookError::InvalidPda)?;
    let (vault_address, _) = pda::find(
        &[
            series_address.as_ref(),
            token_program.address().as_ref(),
            usdc_mint.address().as_ref(),
        ],
        &pinocchio_associated_token_account::ID,
    )?;
    validate::address(vault, &vault_address, MetalbookError::InvalidVault)?;

    let usdc_decimals = {
        let mint = Mint::from_account_view(usdc_mint).map_err(|_| MetalbookError::InvalidMint)?;
        mint.decimals()
    };

    // The series account first, so its state is written before the token CPIs.
    let series_bump_seed = [series_bump];
    let [s0, s1, s2, s3, s4, s5, s6] = seeds.as_slices();
    let series_signer_seeds = [
        Seed::from(s0),
        Seed::from(s1),
        Seed::from(s2),
        Seed::from(s3),
        Seed::from(s4),
        Seed::from(s5),
        Seed::from(s6),
        Seed::from(&series_bump_seed),
    ];
    validate::create_pda_account(
        payer,
        series,
        core::mem::size_of::<Series>(),
        program_id,
        &series_signer_seeds,
    )?;
    {
        let mut bytes = series.try_borrow_mut()?;
        let s = state::init_mut::<Series>(&mut bytes)?;
        s.tag = AccountTag::Series as u8;
        s.version = VERSION;
        s.bump = series_bump;
        s.long_mint_bump = long_bump;
        s.state = metalbook_core::SeriesState::Active as u8;
        s.underlying = underlying_raw;
        s.kind = kind_raw;
        s.usdc_decimals = usdc_decimals;
        s.multiplier = multiplier;
        s.strike = strike;
        s.cap = cap;
        s.expiry = expiry;
        s.usdc_mint = cfg.usdc_mint;
        s.token_program = cfg.token_program;
        s.long_mint = long_address.to_bytes();
        s.vault = vault_address.to_bytes();
        // Oracle snapshot (`docs/PLAN.md` 4.5): the series never reads the config again.
        let index = usize::from(underlying_raw);
        s.receiver_program = cfg.receiver_program;
        s.feed_id = *cfg
            .feed_id
            .get(index)
            .ok_or(MetalbookError::InvalidConfig)?;
        s.expo = *cfg.expo.get(index).ok_or(MetalbookError::InvalidConfig)?;
        s.k_next_print = cfg.k_next_print;
        s.max_conf_bps = cfg.max_conf_bps;
        s.slot_tolerance_s = cfg.slot_tolerance_s;
        s.min_samples = cfg.min_samples;
        s.window_s = cfg.window_s;
        s.slot_s = cfg.slot_s;
        s.challenge_delay_s = cfg.challenge_delay_s;
        s.fallback_lookback_s = cfg.fallback_lookback_s;
        s.hard_deadline_s = cfg.hard_deadline_s;
        s.rent_payer = payer.address().to_bytes();
        s.settle_method = state::SETTLE_METHOD_NONE;
    }

    // Long mint: a PDA of the series, owned by the token program.
    let long_bump_seed = [long_bump];
    let long_seeds = [
        Seed::from(LONG_SEED),
        Seed::from(series_address.as_ref()),
        Seed::from(&long_bump_seed),
    ];
    validate::create_pda_account(
        payer,
        long_mint,
        MINT_LEN,
        token_program.address(),
        &long_seeds,
    )?;
    InitializeMint2::new(long_mint, 0, series.address(), None)
        .invoke_with_program(token_program.address())?;

    // Vault: the series' associated token account for USDC.
    // Vault: the series' associated token account for USDC. Idempotent, because anyone can
    // create a valid associated token account for any wallet first, which would otherwise
    // block the listing. An existing account is then checked like a new one.
    CreateIdempotent {
        funding_account: payer,
        account: vault,
        wallet: series,
        mint: usdc_mint,
        system_program,
        token_program,
    }
    .invoke()?;
    validate::vault_account(vault, usdc_mint.address(), series.address())?;
    Ok(())
}

/// Strike on the underlying's tick, multiplier as configured, call cap on the cap grid.
#[inline(never)]
fn check_grids(cfg: &Config, terms: &SeriesTerms) -> Result<(), MetalbookError> {
    let index = usize::from(terms.key().underlying() as u8);
    let tick = *cfg
        .strike_tick
        .get(index)
        .ok_or(MetalbookError::InvalidConfig)?;
    let strike = terms.key().strike().get();
    let remainder = strike
        .checked_rem(tick)
        .ok_or(MetalbookError::InvalidConfig)?;
    if remainder != 0 {
        return Err(MetalbookError::StrikeOffGrid);
    }
    let multiplier = *cfg
        .multiplier
        .get(index)
        .ok_or(MetalbookError::InvalidConfig)?;
    if terms.multiplier().get() != multiplier {
        return Err(MetalbookError::InvalidMultiplier);
    }
    if terms.key().kind() == OptionKind::Call
        && !cap_on_grid(&cfg.cap_bps, strike, terms.cap().get())?
    {
        return Err(MetalbookError::InvalidCap);
    }
    Ok(())
}

/// Whether `cap × 10_000 == strike × (10_000 + bps)` for a non-zero `bps` of the grid. Exact
/// integer equality: a strike whose offset is not a whole number of atoms has no cap there.
fn cap_on_grid(grid: &[u16], strike: u64, cap: u64) -> Result<bool, MetalbookError> {
    let lhs = u128::from(cap)
        .checked_mul(10_000)
        .ok_or(MetalbookError::Overflow)?;
    for &bps in grid.iter().filter(|&&b| b > 0) {
        let factor = 10_000u128
            .checked_add(u128::from(bps))
            .ok_or(MetalbookError::Overflow)?;
        let rhs = u128::from(strike)
            .checked_mul(factor)
            .ok_or(MetalbookError::Overflow)?;
        if lhs == rhs {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The calendar of `underlying` is genuine and lists `expiry`.
#[inline(never)]
fn check_calendar(
    program_id: &Address,
    calendar: &AccountView,
    underlying: Underlying,
    expiry: i64,
) -> Result<(), MetalbookError> {
    validate::owned_by(calendar, program_id)?;
    let bytes = calendar
        .try_borrow()
        .map_err(|_| MetalbookError::InvalidAccountData)?;
    let c = state::load::<Calendar>(&bytes)?;
    let underlying_seed = [underlying as u8];
    pda::check(
        &[CALENDAR_SEED, &underlying_seed],
        c.bump,
        program_id,
        calendar.address(),
    )?;
    if c.underlying != underlying as u8 {
        return Err(MetalbookError::InvalidAccountData);
    }
    if !c.contains(expiry) {
        return Err(MetalbookError::ExpiryNotInCalendar);
    }
    Ok(())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::disallowed_macros
)]
mod tests {
    use super::cap_on_grid;

    #[test]
    fn test_cap_on_grid_requires_exact_offset() {
        let grid = [1_000u16, 2_500, 0, 0];
        assert_eq!(cap_on_grid(&grid, 4_000_000_000, 4_400_000_000), Ok(true));
        assert_eq!(cap_on_grid(&grid, 4_000_000_000, 5_000_000_000), Ok(true));
        assert_eq!(cap_on_grid(&grid, 4_000_000_000, 4_400_000_001), Ok(false));
        assert_eq!(cap_on_grid(&grid, 4_000_000_000, 4_000_000_000), Ok(false));
        assert_eq!(cap_on_grid(&[0u16; 4], 1, 2), Ok(false));
    }
}
