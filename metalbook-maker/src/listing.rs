//! Quotebook quote terms for a metalbook long token, built only from `quotebook-core` types.
//!
//! The quotebook series pairs the long mint (base, 0 decimals, so one base lot is one long
//! token) with USDC (quote). A price in USDC atoms per long token becomes quote lots per base
//! lot by dividing by the quotebook series' quote lot size, exactly.

use quotebook_core::{
    Address, BaseLots, Nonce, QuoteLotsPerBaseLot, QuoteTerms, SeriesId, Side, UnixTimestamp,
};

use crate::MakerError;

/// The quotebook series a metalbook long token trades in, as the maker needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Listing {
    /// The quotebook program.
    pub program_id: Address,
    /// The quotebook deployment id (copied into every quote to bind it to the deployment).
    pub deployment: [u8; 16],
    /// The quotebook series account (long mint / USDC).
    pub series: SeriesId,
    /// Quotebook series' quote lot size, USDC atoms (the price tick).
    pub quote_lot_size_atoms: u64,
}

/// Quote terms for `size` long tokens at `price_atoms` USDC atoms each, on the maker's `side`.
///
/// # Errors
///
/// `OffLotGrid` if the price is not a whole number of quote lots, `InvalidInput` for a zero
/// lot size, and `Quote` if `quotebook_core::Quote::new` rejects the terms (zero price or
/// size, or an expiry not after issuance).
#[allow(clippy::too_many_arguments)]
pub fn quote_terms(
    listing: &Listing,
    maker: Address,
    taker: Address,
    side: Side,
    price_atoms: u64,
    size: u64,
    issued_at: i64,
    expires_at: i64,
    nonce: u64,
) -> Result<QuoteTerms, MakerError> {
    if listing.quote_lot_size_atoms == 0 {
        return Err(MakerError::InvalidInput("quote lot size must be positive"));
    }
    if price_atoms.checked_rem(listing.quote_lot_size_atoms) != Some(0) {
        return Err(MakerError::OffLotGrid);
    }
    let lots = price_atoms
        .checked_div(listing.quote_lot_size_atoms)
        .ok_or(MakerError::Overflow)?;
    let terms = QuoteTerms {
        program_id: listing.program_id,
        deployment: listing.deployment,
        maker,
        series: listing.series,
        taker,
        side,
        price: QuoteLotsPerBaseLot::new(lots),
        size: BaseLots::new(size),
        issued_at: UnixTimestamp::new(issued_at),
        expires_at: UnixTimestamp::new(expires_at),
        nonce: Nonce::new(nonce),
    };
    quotebook_core::Quote::new(terms).map_err(MakerError::Quote)?;
    Ok(terms)
}
