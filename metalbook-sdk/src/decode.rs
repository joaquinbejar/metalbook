//! Account decoders: the owner must be the program, then the shared layouts check size, tag and
//! version (any alignment).

use metalbook_core::accounts::{self, Calendar, Config, Series, Tagged, Writer};
use metalbook_core::CoreError;

use crate::Address;

/// Why an account could not be decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// Not owned by the program.
    WrongOwner,
    /// Wrong size, tag, version or layout.
    Layout(CoreError),
}

fn read<T: Tagged>(program_id: &Address, owner: &Address, data: &[u8]) -> Result<T, DecodeError> {
    if owner != program_id {
        return Err(DecodeError::WrongOwner);
    }
    accounts::read(data).map_err(DecodeError::Layout)
}

/// Decodes the config.
///
/// # Errors
///
/// [`DecodeError`].
pub fn config(program_id: &Address, owner: &Address, data: &[u8]) -> Result<Config, DecodeError> {
    read(program_id, owner, data)
}

/// Decodes a calendar.
///
/// # Errors
///
/// [`DecodeError`].
pub fn calendar(
    program_id: &Address,
    owner: &Address,
    data: &[u8],
) -> Result<Calendar, DecodeError> {
    read(program_id, owner, data)
}

/// Decodes a series.
///
/// # Errors
///
/// [`DecodeError`].
pub fn series(program_id: &Address, owner: &Address, data: &[u8]) -> Result<Series, DecodeError> {
    read(program_id, owner, data)
}

/// Decodes a writer position.
///
/// # Errors
///
/// [`DecodeError`].
pub fn position(program_id: &Address, owner: &Address, data: &[u8]) -> Result<Writer, DecodeError> {
    read(program_id, owner, data)
}
