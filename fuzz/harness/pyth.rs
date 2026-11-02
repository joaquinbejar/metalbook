//! A `PriceUpdateV2` decoder written from the account layout (pyth-solana-receiver-sdk 2.0.0,
//! Anchor), independently of pyth-lite: the acceptance oracle of `sample_settlement` decides
//! with it what the program must accept.
//!
//! Layout: 8-byte discriminator, 32-byte write authority, verification level (`0` then a
//! signature count byte for `Partial`, `1` for `Full`), then the price feed message: feed id
//! (32), price (i64), conf (u64), exponent (i32), publish time (i64), previous publish time
//! (i64), EMA price (i64), EMA conf (u64), and the posted slot (u64). Bytes after the posted
//! slot are ignored; a missing byte anywhere before it is a decoding failure.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::disallowed_macros,
    dead_code,
    missing_docs
)]

pub const DISCRIMINATOR: [u8; 8] = [0x22, 0xf1, 0x23, 0x63, 0x9d, 0x7e, 0xf4, 0xcd];

/// The decoded fields the program's checks read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decoded {
    /// `None` for `Full`, `Some(n)` for `Partial { num_signatures: n }`.
    pub partial: Option<u8>,
    pub feed: [u8; 32],
    pub price: i64,
    pub conf: u64,
    pub exponent: i32,
    pub publish: i64,
    pub prev: i64,
}

/// Decodes `data`, or `None` when it is not a well-formed `PriceUpdateV2`.
pub fn decode(data: &[u8]) -> Option<Decoded> {
    let take = |at: &mut usize, n: usize| -> Option<&[u8]> {
        let s = data.get(*at..at.checked_add(n)?)?;
        *at += n;
        Some(s)
    };
    let mut at = 0usize;
    if take(&mut at, 8)? != DISCRIMINATOR {
        return None;
    }
    take(&mut at, 32)?;
    let partial = match take(&mut at, 1)?[0] {
        0 => Some(take(&mut at, 1)?[0]),
        1 => None,
        _ => return None,
    };
    let feed: [u8; 32] = take(&mut at, 32)?.try_into().ok()?;
    let i64_at =
        |at: &mut usize| -> Option<i64> { Some(i64::from_le_bytes(take(at, 8)?.try_into().ok()?)) };
    let price = i64_at(&mut at)?;
    let conf = u64::from_le_bytes(take(&mut at, 8)?.try_into().ok()?);
    let exponent = i32::from_le_bytes(take(&mut at, 4)?.try_into().ok()?);
    let publish = i64_at(&mut at)?;
    let prev = i64_at(&mut at)?;
    take(&mut at, 8)?; // EMA price
    take(&mut at, 8)?; // EMA conf
    take(&mut at, 8)?; // posted slot
    Some(Decoded {
        partial,
        feed,
        price,
        conf,
        exponent,
        publish,
        prev,
    })
}

/// Encodes `d` (134 bytes for `Partial`, 133 plus one zero for `Full`).
pub fn encode(d: &Decoded) -> Vec<u8> {
    let mut v = Vec::with_capacity(134);
    v.extend_from_slice(&DISCRIMINATOR);
    v.extend_from_slice(&[7u8; 32]);
    match d.partial {
        None => v.push(1),
        Some(n) => v.extend_from_slice(&[0, n]),
    }
    v.extend_from_slice(&d.feed);
    v.extend_from_slice(&d.price.to_le_bytes());
    v.extend_from_slice(&d.conf.to_le_bytes());
    v.extend_from_slice(&d.exponent.to_le_bytes());
    v.extend_from_slice(&d.publish.to_le_bytes());
    v.extend_from_slice(&d.prev.to_le_bytes());
    v.extend_from_slice(&d.price.to_le_bytes());
    v.extend_from_slice(&d.conf.to_le_bytes());
    v.extend_from_slice(&1u64.to_le_bytes());
    v.resize(134, 0);
    v
}
