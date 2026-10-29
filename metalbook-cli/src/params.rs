//! Human-facing values: the config parameters file, USD amounts, underlyings and kinds.

use metalbook_sdk::core::accounts::Config;
use metalbook_sdk::ix::ConfigParams;
use metalbook_sdk::Address;
use serde::{Deserialize, Serialize};

use crate::error::CliError;

/// USDC atoms per dollar (6 decimals).
const ATOMS_PER_USD: u64 = 1_000_000;

/// The config parameters as a JSON file: `ConfigParams` with the receiver program in base58 and
/// the feed ids in hex. Index 0 of the per-underlying arrays is XAU, 1 is XAG. `show-config`
/// prints the same shape, so a live config can be edited and sent back with `set-config`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParamsFile {
    /// Minimum time from listing to expiry, seconds.
    pub min_listing_s: u32,
    /// Strike tick per underlying, USDC atoms per ounce.
    pub strike_tick: [u64; 2],
    /// Multiplier per underlying, milli-ounces.
    pub multiplier: [u32; 2],
    /// Cap grid offsets, basis points (0 = unused).
    pub cap_bps: [u16; 4],
    /// The Pyth receiver program, base58.
    pub receiver_program: String,
    /// Pyth feed id per underlying, 64 hex digits.
    pub feed_id: [String; 2],
    /// Pinned Pyth exponent per underlying.
    pub expo: [i8; 2],
    /// Largest accepted `conf / price`, basis points.
    pub max_conf_bps: u16,
    /// Slot tolerance, seconds.
    pub slot_tolerance_s: u16,
    /// Filled slots for a `Window` settlement.
    pub min_samples: u16,
    /// Links of a complete next-print chain.
    pub k_next_print: u8,
    /// Averaging window, seconds.
    pub window_s: u32,
    /// Slot spacing, seconds.
    pub slot_s: u32,
    /// Challenge delay `D`, seconds.
    pub challenge_delay_s: u32,
    /// Fallback lookback, seconds.
    pub fallback_lookback_s: u32,
    /// Hard deadline after expiry, seconds.
    pub hard_deadline_s: u32,
}

impl ParamsFile {
    /// Parses the JSON text of a parameters file.
    ///
    /// # Errors
    ///
    /// [`CliError::ParamsFile`] for invalid JSON, unknown or missing fields.
    pub fn from_json(text: &str) -> Result<Self, CliError> {
        serde_json::from_str(text).map_err(|e| CliError::ParamsFile(e.to_string()))
    }

    /// The SDK parameters. The program validates the values themselves.
    ///
    /// # Errors
    ///
    /// [`CliError::ParamsFile`] for a bad receiver address or feed id.
    pub fn to_params(&self) -> Result<ConfigParams, CliError> {
        let receiver_program = parse_address(&self.receiver_program)
            .map_err(|_| CliError::ParamsFile("receiver_program is not a base58 address".into()))?;
        let [xau, xag] = &self.feed_id;
        Ok(ConfigParams {
            min_listing_s: self.min_listing_s,
            strike_tick: self.strike_tick,
            multiplier: self.multiplier,
            cap_bps: self.cap_bps,
            receiver_program,
            feed_id: [parse_feed_id(xau)?, parse_feed_id(xag)?],
            expo: self.expo,
            max_conf_bps: self.max_conf_bps,
            slot_tolerance_s: self.slot_tolerance_s,
            min_samples: self.min_samples,
            k_next_print: self.k_next_print,
            window_s: self.window_s,
            slot_s: self.slot_s,
            challenge_delay_s: self.challenge_delay_s,
            fallback_lookback_s: self.fallback_lookback_s,
            hard_deadline_s: self.hard_deadline_s,
        })
    }

    /// The parameters of a live config.
    #[must_use]
    pub fn from_config(c: &Config) -> Self {
        let [xau, xag] = &c.feed_id;
        Self {
            min_listing_s: c.min_listing_s,
            strike_tick: c.strike_tick,
            multiplier: c.multiplier,
            cap_bps: c.cap_bps,
            receiver_program: Address::new_from_array(c.receiver_program).to_string(),
            feed_id: [hex(xau), hex(xag)],
            expo: c.expo,
            max_conf_bps: c.max_conf_bps,
            slot_tolerance_s: c.slot_tolerance_s,
            min_samples: c.min_samples,
            k_next_print: c.k_next_print,
            window_s: c.window_s,
            slot_s: c.slot_s,
            challenge_delay_s: c.challenge_delay_s,
            fallback_lookback_s: c.fallback_lookback_s,
            hard_deadline_s: c.hard_deadline_s,
        }
    }
}

/// Parses a base58 address.
///
/// # Errors
///
/// [`CliError::Usage`] when it is not one.
pub fn parse_address(s: &str) -> Result<Address, CliError> {
    s.parse::<Address>()
        .map_err(|_| CliError::Usage(format!("{s:?} is not a base58 address")))
}

/// Parses 64 hex digits (an optional `0x` prefix allowed) into a feed id.
///
/// # Errors
///
/// [`CliError::ParamsFile`] for any other input.
pub fn parse_feed_id(s: &str) -> Result<[u8; 32], CliError> {
    let digits = s.strip_prefix("0x").unwrap_or(s).as_bytes();
    let bad = || CliError::ParamsFile(format!("feed id {s:?} is not 64 hex digits"));
    if digits.len() != 64 {
        return Err(bad());
    }
    let mut out = [0u8; 32];
    for (byte, pair) in out.iter_mut().zip(digits.as_chunks::<2>().0) {
        let mut v = 0u8;
        for c in pair {
            let nibble = char::from(*c).to_digit(16).ok_or_else(bad)?;
            // Two nibbles: at most 0xff, so neither step overflows.
            v = v
                .checked_mul(16)
                .and_then(|v| v.checked_add(u8::try_from(nibble).ok()?))
                .ok_or_else(bad)?;
        }
        *byte = v;
    }
    Ok(out)
}

/// Lower-case hex of a feed id.
#[must_use]
pub fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Parses a dollar amount with at most 6 decimals (`"2400"`, `"0.25"`, `"2412.5"`) into USDC
/// atoms, exactly: no rounding, more decimals are an error.
///
/// # Errors
///
/// [`CliError::Usage`] for anything else, or an amount that overflows `u64` atoms.
pub fn parse_usd(s: &str) -> Result<u64, CliError> {
    let bad = || {
        CliError::Usage(format!(
            "{s:?} is not a dollar amount with at most 6 decimals"
        ))
    };
    let (whole, frac) = s.split_once('.').unwrap_or((s, ""));
    let digits_only = |t: &str| t.bytes().all(|b| b.is_ascii_digit());
    if whole.is_empty() || !digits_only(whole) || !digits_only(frac) || frac.len() > 6 {
        return Err(bad());
    }
    if s.contains('.') && frac.is_empty() {
        return Err(bad());
    }
    let whole: u64 = whole.parse().map_err(|_| bad())?;
    let mut frac_atoms = 0u64;
    let mut scale = ATOMS_PER_USD;
    for b in frac.bytes() {
        // At most 6 digits, so `scale` stays a positive power of ten.
        scale = scale.checked_div(10).ok_or_else(bad)?;
        let digit = u64::from(b.checked_sub(b'0').ok_or_else(bad)?);
        frac_atoms = digit
            .checked_mul(scale)
            .and_then(|d| frac_atoms.checked_add(d))
            .ok_or_else(bad)?;
    }
    whole
        .checked_mul(ATOMS_PER_USD)
        .and_then(|w| w.checked_add(frac_atoms))
        .ok_or_else(bad)
}

/// Formats USDC atoms as dollars with every significant decimal (`2400`, `0.25`).
#[must_use]
pub fn format_usd(atoms: u64) -> String {
    let whole = atoms.checked_div(ATOMS_PER_USD).unwrap_or(0);
    let frac = atoms.checked_rem(ATOMS_PER_USD).unwrap_or(0);
    if frac == 0 {
        return whole.to_string();
    }
    let digits = format!("{frac:06}");
    format!("{whole}.{}", digits.trim_end_matches('0'))
}

/// `xau` or `xag` (any case) to the `Underlying` discriminant.
///
/// # Errors
///
/// [`CliError::Usage`] for anything else.
pub fn parse_underlying(s: &str) -> Result<u8, CliError> {
    match s.to_ascii_lowercase().as_str() {
        "xau" => Ok(0),
        "xag" => Ok(1),
        _ => Err(CliError::Usage(format!(
            "underlying {s:?} is not xau or xag"
        ))),
    }
}

/// `call` or `put` (any case) to the `OptionKind` discriminant.
///
/// # Errors
///
/// [`CliError::Usage`] for anything else.
pub fn parse_kind(s: &str) -> Result<u8, CliError> {
    match s.to_ascii_lowercase().as_str() {
        "call" => Ok(0),
        "put" => Ok(1),
        _ => Err(CliError::Usage(format!("kind {s:?} is not call or put"))),
    }
}

/// The name of an `Underlying` discriminant.
#[must_use]
pub fn underlying_name(u: u8) -> &'static str {
    match u {
        0 => "xau",
        1 => "xag",
        _ => "unknown",
    }
}

/// The name of an `OptionKind` discriminant.
#[must_use]
pub fn kind_name(k: u8) -> &'static str {
    match k {
        0 => "call",
        1 => "put",
        _ => "unknown",
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::disallowed_macros
)]
mod tests {
    use super::*;

    #[test]
    fn usd_amounts_parse_exactly() {
        assert_eq!(parse_usd("2400").unwrap(), 2_400_000_000);
        assert_eq!(parse_usd("0.25").unwrap(), 250_000);
        assert_eq!(parse_usd("2412.5").unwrap(), 2_412_500_000);
        assert_eq!(parse_usd("0.000001").unwrap(), 1);
        assert_eq!(parse_usd("0").unwrap(), 0);
        for bad in [
            "",
            ".5",
            "5.",
            "-1",
            "+1",
            "1e3",
            "1.0000001",
            "1,5",
            " 1",
            "18446744073710",
        ] {
            assert!(parse_usd(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn usd_amounts_format_back() {
        for (atoms, text) in [
            (2_400_000_000, "2400"),
            (250_000, "0.25"),
            (2_412_500_000, "2412.5"),
            (1, "0.000001"),
            (0, "0"),
        ] {
            assert_eq!(format_usd(atoms), text);
            assert_eq!(parse_usd(text).unwrap(), atoms);
        }
    }

    #[test]
    fn feed_ids_round_trip_through_hex() {
        let id = "765d2ba906dbc32ca17cc11f5310a89e9ee1f6420508c63861f2f8ba4ee34bb2";
        let bytes = parse_feed_id(id).unwrap();
        assert_eq!(bytes[0], 0x76);
        assert_eq!(bytes[31], 0xb2);
        assert_eq!(hex(&bytes), id);
        assert_eq!(parse_feed_id(&format!("0x{id}")).unwrap(), bytes);
        assert_eq!(
            parse_feed_id(&id.to_ascii_uppercase()).unwrap(),
            bytes,
            "upper case is accepted"
        );
        assert!(parse_feed_id(&id[1..]).is_err());
        assert!(parse_feed_id(&format!("{}g", &id[1..])).is_err());
    }

    #[test]
    fn names_parse_and_print() {
        assert_eq!(parse_underlying("XAU").unwrap(), 0);
        assert_eq!(parse_underlying("xag").unwrap(), 1);
        assert!(parse_underlying("btc").is_err());
        assert_eq!(parse_kind("Call").unwrap(), 0);
        assert_eq!(parse_kind("put").unwrap(), 1);
        assert!(parse_kind("straddle").is_err());
        assert_eq!(underlying_name(1), "xag");
        assert_eq!(kind_name(1), "put");
    }
}
