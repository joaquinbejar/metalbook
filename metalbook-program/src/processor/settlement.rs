//! `sample_settlement` and `settle` (`docs/PLAN.md` 4.2 and 4.3).

use metalbook_core::{Decision, Print, SeriesLedger, SeriesState, UnixTimestamp};
use pinocchio::sysvars::clock::Clock;
use pinocchio::sysvars::Sysvar;
use pinocchio::{AccountView, Address, ProgramResult};
use pyth_lite::{FeedId, PriceRequirements, PriceUpdate};

use crate::error::MetalbookError;
use crate::processor::position::load_series;
use crate::state::{self, Series};
use crate::validate;

/// `sample_settlement`. Accounts: `[series (writable), price_update]`. Data: none.
///
/// Anyone may post any `PriceUpdateV2` for the series' feed: it must be owned by the series'
/// receiver program, `Full`-verified, for the series' feed id, with the pinned exponent, a
/// positive price, `prev_publish_time > 0` and `conf / price` within the bound. It is then
/// stored as every candidate it qualifies for (window slot, next-print link, fallback). No
/// staleness bound: canonical prints can be backfilled, and the candidates bound time
/// themselves. The first call at or after expiry moves the series from `Active` to
/// `Expired`.
///
/// # Errors
///
/// `SeriesAlreadySettled` once settled or voided, the oracle errors above, `InvalidPrint`, or
/// `SampleOutsideWindow` if the print qualifies as nothing.
#[inline(never)]
pub fn sample_settlement(
    program_id: &Address,
    accounts: &mut [AccountView],
    data: &[u8],
) -> ProgramResult {
    let [series, price_update, ..] = accounts else {
        return Err(pinocchio::error::ProgramError::NotEnoughAccountKeys);
    };
    if !data.is_empty() {
        return Err(MetalbookError::InvalidInstructionData.into());
    }
    validate::writable(series)?;
    let s = load_series(program_id, series)?;
    let now = Clock::get()?.unix_timestamp;
    let mut ledger = open_ledger(&s)?;
    let params = s.oracle_params()?;
    let mut candidates = s.settlement(&params)?;

    let print = {
        let bytes = price_update
            .try_borrow()
            .map_err(|_| MetalbookError::InvalidAccountData)?;
        let update = PriceUpdate::load(
            &price_update.owner().to_bytes(),
            &bytes,
            &s.receiver_program,
        )
        .map_err(MetalbookError::from)?;
        let req = PriceRequirements::new(FeedId::new(s.feed_id), None, s.max_conf_bps);
        let price = update
            .verify(&req, pyth_lite::UnixTimestamp::new(now))
            .map_err(MetalbookError::from)?;
        if price.exponent() != i32::from(s.expo) {
            return Err(MetalbookError::OracleExponentMismatch.into());
        }
        Print::try_new(
            price.price().get(),
            price.publish_time().get(),
            price.prev_publish_time().get(),
        )
        .map_err(MetalbookError::from)?
    };
    candidates
        .add_print(&params, UnixTimestamp::new(s.expiry), print, now)
        .map_err(MetalbookError::from)?;

    let mut bytes = series.try_borrow_mut()?;
    let stored = state::load_mut::<Series>(&mut bytes)?;
    stored.store_ledger(&ledger_after_expiry(&mut ledger, now, s.expiry)?);
    stored.store_settlement(&candidates);
    Ok(())
}

/// `settle`. Accounts: `[series (writable)]`. Data: none. Anyone may call it.
///
/// Runs the cascade of `docs/PLAN.md` 4.3 at the current time and fixes the settlement price
/// (payout per long token rounded down, toward the protocol) or voids the series. A
/// candidate inside its challenge delay blocks every lower step, so a pending better price
/// is never skipped.
///
/// # Errors
///
/// `SeriesNotExpired` before expiry, `SeriesAlreadySettled` on a second call,
/// `SettlementNotReady` while the cascade waits.
#[inline(never)]
pub fn settle(program_id: &Address, accounts: &mut [AccountView], data: &[u8]) -> ProgramResult {
    let [series, ..] = accounts else {
        return Err(pinocchio::error::ProgramError::NotEnoughAccountKeys);
    };
    if !data.is_empty() {
        return Err(MetalbookError::InvalidInstructionData.into());
    }
    validate::writable(series)?;
    let s = load_series(program_id, series)?;
    let now = Clock::get()?.unix_timestamp;
    if now < s.expiry {
        return Err(MetalbookError::SeriesNotExpired.into());
    }
    let mut ledger = open_ledger(&s)?;
    let mut ledger = ledger_after_expiry(&mut ledger, now, s.expiry)?;
    let params = s.oracle_params()?;
    let candidates = s.settlement(&params)?;
    let decision = candidates
        .decide(&params, UnixTimestamp::new(s.expiry), now)
        .map_err(MetalbookError::from)?;

    let (method, price, samples) = match decision {
        Decision::Settle {
            method,
            price,
            samples,
        } => {
            ledger.settle(price).map_err(MetalbookError::from)?;
            (method, price.get(), samples)
        }
        Decision::Void => {
            ledger.void().map_err(MetalbookError::from)?;
            (metalbook_core::SettleMethod::Void, 0, 0)
        }
        Decision::Wait { .. } => return Err(MetalbookError::SettlementNotReady.into()),
    };

    let mut bytes = series.try_borrow_mut()?;
    let stored = state::load_mut::<Series>(&mut bytes)?;
    stored.store_ledger(&ledger);
    stored.settle_method = method as u8;
    stored.settlement_price = price;
    stored.settlement_samples = samples;
    Ok(())
}

/// The series' ledger, which must still accept prints: `Active` or `Expired`.
fn open_ledger(s: &Series) -> Result<SeriesLedger, MetalbookError> {
    let ledger = s.ledger()?;
    match ledger.state() {
        SeriesState::Active | SeriesState::Expired => Ok(ledger),
        SeriesState::Settled | SeriesState::Voided => Err(MetalbookError::SeriesAlreadySettled),
    }
}

/// Moves an `Active` series to `Expired` once `now >= expiry` (the lazy transition of
/// `docs/PLAN.md` 3.3); `mint` and `close` check the clock themselves.
fn ledger_after_expiry(
    ledger: &mut SeriesLedger,
    now: i64,
    expiry: i64,
) -> Result<SeriesLedger, MetalbookError> {
    if now >= expiry && ledger.state() == SeriesState::Active {
        ledger.expire().map_err(MetalbookError::from)?;
    }
    Ok(*ledger)
}
