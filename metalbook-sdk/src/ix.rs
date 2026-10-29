//! Instruction builders, one per program instruction. Token accounts of writers and holders
//! are their associated token accounts.

use solana_instruction::{AccountMeta, Instruction};

use crate::pda::{self, SeriesSpec};
use crate::{Address, ATA_PROGRAM, SYSTEM_PROGRAM, TOKEN_PROGRAM};

/// Instruction tags (`metalbook-program/src/lib.rs`).
pub mod tag {
    /// `init_config`.
    pub const INIT_CONFIG: u8 = 0;
    /// `set_config`.
    pub const SET_CONFIG: u8 = 1;
    /// `set_calendar`.
    pub const SET_CALENDAR: u8 = 2;
    /// `create_series`.
    pub const CREATE_SERIES: u8 = 3;
    /// `mint`.
    pub const MINT: u8 = 4;
    /// `close`.
    pub const CLOSE: u8 = 5;
    /// `sample_settlement`.
    pub const SAMPLE_SETTLEMENT: u8 = 6;
    /// `settle`.
    pub const SETTLE: u8 = 7;
    /// `redeem`.
    pub const REDEEM: u8 = 8;
    /// `claim_writer`.
    pub const CLAIM_WRITER: u8 = 9;
    /// `close_series`.
    pub const CLOSE_SERIES: u8 = 10;
    /// `close_position`.
    pub const CLOSE_POSITION: u8 = 11;
}

/// The config parameters of `init_config` and `set_config` (161 bytes; index 0 of the
/// per-underlying arrays is XAU, 1 is XAG).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigParams {
    /// Minimum time from listing to expiry, seconds.
    pub min_listing_s: u32,
    /// Strike tick per underlying, USDC atoms per ounce.
    pub strike_tick: [u64; 2],
    /// Multiplier per underlying, milli-ounces.
    pub multiplier: [u32; 2],
    /// Cap grid offsets, basis points (0 = unused).
    pub cap_bps: [u16; 4],
    /// The Pyth receiver program.
    pub receiver_program: Address,
    /// Pyth feed id per underlying.
    pub feed_id: [[u8; 32]; 2],
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

impl ConfigParams {
    /// The 161-byte little-endian encoding.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut d = Vec::with_capacity(161);
        d.extend_from_slice(&self.min_listing_s.to_le_bytes());
        for t in self.strike_tick {
            d.extend_from_slice(&t.to_le_bytes());
        }
        for m in self.multiplier {
            d.extend_from_slice(&m.to_le_bytes());
        }
        for b in self.cap_bps {
            d.extend_from_slice(&b.to_le_bytes());
        }
        d.extend_from_slice(self.receiver_program.as_ref());
        for f in &self.feed_id {
            d.extend_from_slice(f);
        }
        for e in self.expo {
            d.extend_from_slice(&e.to_le_bytes());
        }
        d.extend_from_slice(&self.max_conf_bps.to_le_bytes());
        d.extend_from_slice(&self.slot_tolerance_s.to_le_bytes());
        d.extend_from_slice(&self.min_samples.to_le_bytes());
        d.push(self.k_next_print);
        for v in [
            self.window_s,
            self.slot_s,
            self.challenge_delay_s,
            self.fallback_lookback_s,
            self.hard_deadline_s,
        ] {
            d.extend_from_slice(&v.to_le_bytes());
        }
        d
    }
}

fn w(a: Address) -> AccountMeta {
    AccountMeta::new(a, false)
}

fn r(a: Address) -> AccountMeta {
    AccountMeta::new_readonly(a, false)
}

fn ix(program_id: &Address, accounts: Vec<AccountMeta>, data: Vec<u8>) -> Instruction {
    Instruction {
        program_id: *program_id,
        accounts,
        data,
    }
}

fn with_u64(tag: u8, v: u64) -> Vec<u8> {
    let mut d = vec![tag];
    d.extend_from_slice(&v.to_le_bytes());
    d
}

/// `init_config`: signed by the program's upgrade authority, who becomes admin.
#[must_use]
pub fn init_config(
    program_id: &Address,
    admin: &Address,
    usdc_mint: &Address,
    params: &ConfigParams,
) -> Instruction {
    let mut data = vec![tag::INIT_CONFIG];
    data.extend(params.encode());
    ix(
        program_id,
        vec![
            AccountMeta::new(*admin, true),
            w(pda::config(program_id).0),
            r(pda::program_data(program_id).0),
            r(*usdc_mint),
            r(SYSTEM_PROGRAM),
        ],
        data,
    )
}

/// `set_config`: signed by the admin; `new_admin` may be the same key.
#[must_use]
pub fn set_config(
    program_id: &Address,
    admin: &Address,
    new_admin: &Address,
    params: &ConfigParams,
) -> Instruction {
    let mut data = vec![tag::SET_CONFIG];
    data.extend_from_slice(new_admin.as_ref());
    data.extend(params.encode());
    ix(
        program_id,
        vec![
            AccountMeta::new_readonly(*admin, true),
            w(pda::config(program_id).0),
        ],
        data,
    )
}

/// `set_calendar`: signed by the admin (who pays the calendar's rent on first use). At most 64
/// strictly increasing expiries.
#[must_use]
pub fn set_calendar(
    program_id: &Address,
    admin: &Address,
    underlying: u8,
    expiries: &[i64],
) -> Instruction {
    let mut data = vec![
        tag::SET_CALENDAR,
        underlying,
        u8::try_from(expiries.len()).unwrap_or(u8::MAX),
    ];
    for e in expiries {
        data.extend_from_slice(&e.to_le_bytes());
    }
    ix(
        program_id,
        vec![
            AccountMeta::new(*admin, true),
            r(pda::config(program_id).0),
            w(pda::calendar(program_id, underlying).0),
            r(SYSTEM_PROGRAM),
        ],
        data,
    )
}

/// `create_series`: anyone, paying the rent of the series, its long mint and its vault.
#[must_use]
pub fn create_series(
    program_id: &Address,
    payer: &Address,
    usdc_mint: &Address,
    s: &SeriesSpec,
) -> Instruction {
    let series = pda::series(program_id, s).0;
    let mut data = vec![tag::CREATE_SERIES, s.underlying, s.kind];
    data.extend_from_slice(&s.strike.to_le_bytes());
    data.extend_from_slice(&s.expiry.to_le_bytes());
    data.extend_from_slice(&s.cap.to_le_bytes());
    data.extend_from_slice(&s.multiplier.to_le_bytes());
    ix(
        program_id,
        vec![
            AccountMeta::new(*payer, true),
            r(pda::config(program_id).0),
            r(pda::calendar(program_id, s.underlying).0),
            w(series),
            w(pda::long_mint(program_id, &series).0),
            w(pda::ata(&series, usdc_mint)),
            r(*usdc_mint),
            r(TOKEN_PROGRAM),
            r(ATA_PROGRAM),
            r(SYSTEM_PROGRAM),
        ],
        data,
    )
}

fn position_accounts(
    program_id: &Address,
    writer: &Address,
    series: &Address,
    usdc_mint: &Address,
) -> Vec<AccountMeta> {
    let long_mint = pda::long_mint(program_id, series).0;
    vec![
        AccountMeta::new(*writer, true),
        w(*series),
        w(pda::position(program_id, series, writer).0),
        w(pda::ata(writer, usdc_mint)),
        w(pda::ata(series, usdc_mint)),
        r(*usdc_mint),
        w(long_mint),
        w(pda::ata(writer, &long_mint)),
        r(TOKEN_PROGRAM),
    ]
}

/// `mint`: the writer deposits `contracts × max_payout` and receives `contracts` long tokens.
#[must_use]
pub fn mint(
    program_id: &Address,
    writer: &Address,
    series: &Address,
    usdc_mint: &Address,
    contracts: u64,
) -> Instruction {
    let mut accounts = position_accounts(program_id, writer, series, usdc_mint);
    accounts.push(r(SYSTEM_PROGRAM));
    ix(program_id, accounts, with_u64(tag::MINT, contracts))
}

/// `close`: before expiry, the writer burns its own long tokens and withdraws collateral.
#[must_use]
pub fn close(
    program_id: &Address,
    writer: &Address,
    series: &Address,
    usdc_mint: &Address,
    contracts: u64,
) -> Instruction {
    ix(
        program_id,
        position_accounts(program_id, writer, series, usdc_mint),
        with_u64(tag::CLOSE, contracts),
    )
}

/// `sample_settlement`: anyone, with a posted `PriceUpdateV2` account.
#[must_use]
pub fn sample_settlement(
    program_id: &Address,
    series: &Address,
    price_update: &Address,
) -> Instruction {
    ix(
        program_id,
        vec![w(*series), r(*price_update)],
        vec![tag::SAMPLE_SETTLEMENT],
    )
}

/// `settle`: anyone.
#[must_use]
pub fn settle(program_id: &Address, series: &Address) -> Instruction {
    ix(program_id, vec![w(*series)], vec![tag::SETTLE])
}

/// `redeem`: the holder burns its own long tokens and receives the payout.
#[must_use]
pub fn redeem(
    program_id: &Address,
    holder: &Address,
    series: &Address,
    usdc_mint: &Address,
    contracts: u64,
) -> Instruction {
    let long_mint = pda::long_mint(program_id, series).0;
    ix(
        program_id,
        vec![
            AccountMeta::new_readonly(*holder, true),
            w(*series),
            w(pda::ata(holder, &long_mint)),
            w(pda::ata(holder, usdc_mint)),
            w(pda::ata(series, usdc_mint)),
            r(*usdc_mint),
            w(long_mint),
            r(TOKEN_PROGRAM),
        ],
        with_u64(tag::REDEEM, contracts),
    )
}

/// `claim_writer`: once per position, the writer receives its residual.
#[must_use]
pub fn claim_writer(
    program_id: &Address,
    writer: &Address,
    series: &Address,
    usdc_mint: &Address,
) -> Instruction {
    ix(
        program_id,
        vec![
            AccountMeta::new_readonly(*writer, true),
            w(*series),
            w(pda::position(program_id, series, writer).0),
            w(pda::ata(writer, usdc_mint)),
            w(pda::ata(series, usdc_mint)),
            r(*usdc_mint),
            r(TOKEN_PROGRAM),
        ],
        vec![tag::CLAIM_WRITER],
    )
}

/// `close_series`: anyone, once nothing is owed; rent and any unowed vault balance go to
/// `rent_payer` (the `create_series` payer) and its USDC account.
#[must_use]
pub fn close_series(
    program_id: &Address,
    series: &Address,
    rent_payer: &Address,
    usdc_mint: &Address,
) -> Instruction {
    ix(
        program_id,
        vec![
            w(*series),
            w(pda::ata(series, usdc_mint)),
            w(*rent_payer),
            w(pda::ata(rent_payer, usdc_mint)),
            r(*usdc_mint),
            r(TOKEN_PROGRAM),
        ],
        vec![tag::CLOSE_SERIES],
    )
}

/// `close_position`: anyone, once the position has claimed (or holds nothing); rent goes to the
/// writer.
#[must_use]
pub fn close_position(program_id: &Address, series: &Address, writer: &Address) -> Instruction {
    ix(
        program_id,
        vec![w(pda::position(program_id, series, writer).0), w(*writer)],
        vec![tag::CLOSE_POSITION],
    )
}
