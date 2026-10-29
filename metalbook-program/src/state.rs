//! Account layouts: defined in `metalbook_core::accounts` (one source for the program, the
//! keeper and SDKs). This module re-exports them and wraps the loaders so that their errors
//! are program errors.

pub use metalbook_core::accounts::{
    AccountTag, Calendar, Config, Series, Tagged, Writer, CALENDAR_CAPACITY, CAP_GRID_LEN,
    SETTLE_METHOD_NONE, VERSION,
};

use crate::error::MetalbookError;

/// [`metalbook_core::accounts::load`] with a program error.
///
/// # Errors
///
/// `InvalidAccountData` on a length, alignment, tag or version mismatch.
#[must_use = "the result carries a possible error"]
pub fn load<T: Tagged>(data: &[u8]) -> Result<&T, MetalbookError> {
    Ok(metalbook_core::accounts::load(data)?)
}

/// [`metalbook_core::accounts::load_mut`] with a program error.
///
/// # Errors
///
/// `InvalidAccountData` on a length, alignment, tag or version mismatch.
#[must_use = "the result carries a possible error"]
pub fn load_mut<T: Tagged>(data: &mut [u8]) -> Result<&mut T, MetalbookError> {
    Ok(metalbook_core::accounts::load_mut(data)?)
}

/// [`metalbook_core::accounts::init_mut`] with a program error.
///
/// # Errors
///
/// `InvalidAccountData` on a length or alignment mismatch.
#[must_use = "the result carries a possible error"]
pub fn init_mut<T: Tagged>(data: &mut [u8]) -> Result<&mut T, MetalbookError> {
    Ok(metalbook_core::accounts::init_mut(data)?)
}
