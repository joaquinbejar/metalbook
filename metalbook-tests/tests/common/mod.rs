//! Shared LiteSVM harness: loads the program, a USDC mint and the config, and builds every
//! instruction. The account layouts and instruction encodings are restated here from
//! `docs/PLAN.md` and the handler docs, not imported from the program, so a layout mistake on
//! either side fails the tests.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::integer_division,
    clippy::disallowed_macros,
    dead_code,
    missing_docs
)]

use std::path::PathBuf;

use litesvm::LiteSVM;
use litesvm_token::{CreateAssociatedTokenAccount, CreateMint, MintTo};
use solana_account::Account;
use solana_address::Address;
use solana_clock::Clock;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_signer::Signer;
use solana_transaction::Transaction;

pub const PROGRAM_ID: Address = Address::new_from_array([0x4d; 32]);
pub const TOKEN_PROGRAM: Address =
    Address::from_str_const("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
pub const TOKEN_2022: Address =
    Address::from_str_const("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");
pub const ATA_PROGRAM: Address =
    Address::from_str_const("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
pub const SYSTEM_PROGRAM: Address = Address::from_str_const("11111111111111111111111111111111");
pub const UPGRADEABLE_LOADER: Address =
    Address::from_str_const("BPFLoaderUpgradeab1e11111111111111111111111");
pub const PYTH_RECEIVER: Address =
    Address::from_str_const("rec5EKMGg6MxZYaMdyBfgwp4d5rB9T1VQH5pJv5LtFJ");
/// `Metal.XAU/USD` and `Metal.XAG/USD` feed ids (`docs/PLAN.md`, verified facts).
pub const FEED_XAU: [u8; 32] = [
    0x76, 0x5d, 0x2b, 0xa9, 0x06, 0xdb, 0xc3, 0x2c, 0xa1, 0x7c, 0xc1, 0x1f, 0x53, 0x10, 0xa8, 0x9e,
    0x9e, 0xe1, 0xf6, 0x42, 0x05, 0x08, 0xc6, 0x38, 0x61, 0xf2, 0xf8, 0xba, 0x4e, 0xe3, 0x4b, 0xb2,
];
pub const FEED_XAG: [u8; 32] = [
    0xf2, 0xfb, 0x02, 0xc3, 0x2b, 0x05, 0x5c, 0x80, 0x5e, 0x72, 0x38, 0xd6, 0x28, 0xe5, 0xe9, 0xda,
    0xde, 0xf2, 0x74, 0x37, 0x61, 0x14, 0xeb, 0x1f, 0x01, 0x23, 0x37, 0xca, 0xbe, 0x93, 0x87, 0x1e,
];
/// Oracle terms of the default config: 30 slots of 60 s, tolerance 10 s, 20 samples, k = 5,
/// D = 600 s, fallback lookback 24 h, hard deadline 7 days, max conf 1%.
pub const WINDOW_S: i64 = 1_800;
pub const SLOT_S: i64 = 60;
pub const TOLERANCE_S: i64 = 10;
pub const MIN_SAMPLES: u16 = 20;
pub const D: i64 = 600;
pub const LOOKBACK_S: i64 = 86_400;
pub const HARD_S: i64 = 604_800;
pub const EXPO_XAU: i8 = -3;
pub const EXPO_XAG: i8 = -5;

/// Program error codes (`MetalbookError`), restated.
pub mod code {
    pub const INVALID_INSTRUCTION_DATA: u32 = 101;
    pub const MISSING_SIGNATURE: u32 = 102;
    pub const INVALID_ACCOUNT_OWNER: u32 = 103;
    pub const INVALID_PDA: u32 = 104;
    pub const INVALID_MINT: u32 = 105;
    pub const INVALID_VAULT: u32 = 106;
    pub const NOT_AUTHORITY: u32 = 108;
    pub const EXPIRY_NOT_IN_CALENDAR: u32 = 110;
    pub const STRIKE_OFF_GRID: u32 = 111;
    pub const INVALID_CAP: u32 = 112;
    pub const SERIES_EXPIRED: u32 = 113;
    pub const INVALID_ACCOUNT_DATA: u32 = 128;
    pub const INVALID_PROGRAM: u32 = 129;
    pub const INVALID_TOKEN_ACCOUNT: u32 = 130;
    pub const ACCOUNT_NOT_WRITABLE: u32 = 131;
    pub const ALREADY_INITIALIZED: u32 = 132;
    pub const INVALID_CONFIG: u32 = 133;
    pub const INVALID_MULTIPLIER: u32 = 134;
    pub const LISTING_TOO_LATE: u32 = 135;
    pub const INSUFFICIENT_WRITTEN: u32 = 136;
    pub const ZERO_AMOUNT: u32 = 137;
    pub const INVALID_STRIKE: u32 = 139;
    pub const SERIES_NOT_EXPIRED: u32 = 114;
    pub const SERIES_ALREADY_SETTLED: u32 = 116;
    pub const ORACLE_OWNER_MISMATCH: u32 = 119;
    pub const ORACLE_NOT_FULLY_VERIFIED: u32 = 120;
    pub const ORACLE_FEED_MISMATCH: u32 = 121;
    pub const ORACLE_CONFIDENCE_TOO_WIDE: u32 = 123;
    pub const SAMPLE_OUTSIDE_WINDOW: u32 = 124;
    pub const SETTLEMENT_NOT_READY: u32 = 141;
    pub const ORACLE_EXPONENT_MISMATCH: u32 = 142;
    pub const INVALID_PRINT: u32 = 143;
    pub const SERIES_NOT_SETTLED: u32 = 115;
    pub const INSUFFICIENT_LONG_TOKENS: u32 = 118;
    pub const ALREADY_CLAIMED: u32 = 144;
    pub const SERIES_NOT_EMPTY: u32 = 145;
    pub const INVALID_RENT_PAYER: u32 = 146;
    pub const POSITION_NOT_CLAIMED: u32 = 147;
}

pub const USD: u64 = 1_000_000;
/// Clock at setup.
pub const NOW: i64 = 1_800_000_000;
/// Calendar expiries, more than a day after `NOW`.
pub const EXPIRY: i64 = NOW + 7 * 86_400;
pub const EXPIRY_2: i64 = NOW + 14 * 86_400;
pub const MIN_LISTING_S: u32 = 3_600;
pub const XAU: u8 = 0;
pub const XAG: u8 = 1;
pub const CALL: u8 = 0;
pub const PUT: u8 = 1;
/// 1 oz for XAU, 50 oz for XAG, in milli-ounces.
pub const MULT_XAU: u32 = 1_000;
pub const MULT_XAG: u32 = 50_000;
/// Strike ticks: 5 USD for XAU, 0.25 USD for XAG.
pub const TICK_XAU: u64 = 5 * USD;
pub const TICK_XAG: u64 = 250_000;
pub const CAP_BPS: [u16; 4] = [1_000, 2_500, 5_000, 0];

#[derive(Debug, Clone, Copy)]
pub struct Params {
    pub min_listing_s: u32,
    pub strike_tick: [u64; 2],
    pub multiplier: [u32; 2],
    pub cap_bps: [u16; 4],
    pub receiver: Address,
    pub feed_id: [[u8; 32]; 2],
    pub expo: [i8; 2],
    pub max_conf_bps: u16,
    pub slot_tolerance_s: u16,
    pub min_samples: u16,
    pub k_next_print: u8,
    pub window_s: u32,
    pub slot_s: u32,
    pub challenge_delay_s: u32,
    pub fallback_lookback_s: u32,
    pub hard_deadline_s: u32,
}

impl Params {
    pub fn default_params() -> Self {
        Self {
            min_listing_s: MIN_LISTING_S,
            strike_tick: [TICK_XAU, TICK_XAG],
            multiplier: [MULT_XAU, MULT_XAG],
            cap_bps: CAP_BPS,
            receiver: PYTH_RECEIVER,
            feed_id: [FEED_XAU, FEED_XAG],
            expo: [EXPO_XAU, EXPO_XAG],
            max_conf_bps: 100,
            slot_tolerance_s: TOLERANCE_S as u16,
            min_samples: MIN_SAMPLES,
            k_next_print: 5,
            window_s: WINDOW_S as u32,
            slot_s: SLOT_S as u32,
            challenge_delay_s: D as u32,
            fallback_lookback_s: LOOKBACK_S as u32,
            hard_deadline_s: HARD_S as u32,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut d = Vec::new();
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
        d.extend_from_slice(self.receiver.as_ref());
        d.extend_from_slice(&self.feed_id[0]);
        d.extend_from_slice(&self.feed_id[1]);
        d.extend_from_slice(&[self.expo[0] as u8, self.expo[1] as u8]);
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
        assert_eq!(d.len(), 161);
        d
    }
}

/// Terms of a series as the instruction encodes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeriesArgs {
    pub underlying: u8,
    pub kind: u8,
    pub strike: u64,
    pub expiry: i64,
    pub cap: u64,
    pub multiplier: u32,
}

impl SeriesArgs {
    pub fn xau_put(strike_usd: u64) -> Self {
        Self {
            underlying: XAU,
            kind: PUT,
            strike: strike_usd * USD,
            expiry: EXPIRY,
            cap: 0,
            multiplier: MULT_XAU,
        }
    }

    /// Capped call with the 10% cap of the grid.
    pub fn xau_call(strike_usd: u64) -> Self {
        Self {
            underlying: XAU,
            kind: CALL,
            strike: strike_usd * USD,
            expiry: EXPIRY,
            cap: strike_usd * USD * 11 / 10,
            multiplier: MULT_XAU,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut d = vec![3u8, self.underlying, self.kind];
        d.extend_from_slice(&self.strike.to_le_bytes());
        d.extend_from_slice(&self.expiry.to_le_bytes());
        d.extend_from_slice(&self.cap.to_le_bytes());
        d.extend_from_slice(&self.multiplier.to_le_bytes());
        d
    }

    pub fn series(&self) -> Address {
        Address::find_program_address(
            &[
                b"series",
                &[self.underlying],
                &[self.kind],
                &self.strike.to_le_bytes(),
                &self.expiry.to_le_bytes(),
                &self.cap.to_le_bytes(),
                &self.multiplier.to_le_bytes(),
            ],
            &PROGRAM_ID,
        )
        .0
    }

    pub fn long_mint(&self) -> Address {
        Address::find_program_address(&[b"long", self.series().as_ref()], &PROGRAM_ID).0
    }

    /// Exact maximum payout per contract rounded up, computed here from the terms.
    pub fn max_payout(&self) -> u64 {
        let per_oz = if self.kind == PUT {
            self.strike
        } else {
            self.cap - self.strike
        };
        let product = u128::from(per_oz) * u128::from(self.multiplier);
        product.div_ceil(1_000) as u64
    }
}

pub fn config_address() -> Address {
    Address::find_program_address(&[b"config"], &PROGRAM_ID).0
}

pub fn calendar_address(underlying: u8) -> Address {
    Address::find_program_address(&[b"calendar", &[underlying]], &PROGRAM_ID).0
}

pub fn position_address(series: &Address, writer: &Address) -> Address {
    Address::find_program_address(&[b"writer", series.as_ref(), writer.as_ref()], &PROGRAM_ID).0
}

pub fn ata(owner: &Address, mint: &Address) -> Address {
    Address::find_program_address(
        &[owner.as_ref(), TOKEN_PROGRAM.as_ref(), mint.as_ref()],
        &ATA_PROGRAM,
    )
    .0
}

pub fn program_data_address() -> Address {
    Address::find_program_address(&[PROGRAM_ID.as_ref()], &UPGRADEABLE_LOADER).0
}

pub fn meta(address: Address, signer: bool, writable: bool) -> AccountMeta {
    if writable {
        AccountMeta::new(address, signer)
    } else {
        AccountMeta::new_readonly(address, signer)
    }
}

pub struct Env {
    pub svm: LiteSVM,
    pub admin: Keypair,
    pub usdc: Address,
    /// Counter for deterministic keypairs: PDA bump searches, and so CU, depend on addresses.
    next_key: u64,
}

/// Fixed address of the test USDC mint.
pub const USDC_MINT: Address = Address::new_from_array([0x55; 32]);

/// The `n`-th deterministic keypair.
pub fn keypair(n: u64) -> Keypair {
    let mut secret = [0x11u8; 32];
    secret[..8].copy_from_slice(&n.to_le_bytes());
    Keypair::new_from_array(secret)
}

/// Outcome of a transaction: compute units on success, or the error rendered with `{:?}`
/// and the logs.
pub type Sent = Result<u64, (String, Vec<String>)>;

impl Env {
    /// Program loaded with `admin` as upgrade authority, a 6-decimal USDC mint, the clock at
    /// [`NOW`], and nothing else.
    pub fn bare() -> Self {
        let mut svm = LiteSVM::new();
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/deploy/metalbook_program.so");
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|e| panic!("{}: {e}; run `make program` first", path.display()));
        svm.add_program(PROGRAM_ID, &bytes).unwrap();
        let admin = keypair(0);
        svm.airdrop(&admin.pubkey(), 1_000_000_000_000).unwrap();
        // LiteSVM writes ProgramData without an upgrade authority; name the admin.
        let pd = program_data_address();
        let mut account = svm.get_account(&pd).unwrap();
        account.data[12] = 1;
        account.data[13..45].copy_from_slice(admin.pubkey().as_ref());
        svm.set_account(pd, account).unwrap();
        // A 6-decimal mint, moved to a fixed address (mint data does not contain its own
        // address) so that every derived address, and so every CU figure, is reproducible.
        let random = CreateMint::new(&mut svm, &admin)
            .decimals(6)
            .authority(&admin.pubkey())
            .send()
            .unwrap();
        let mint_account = svm.get_account(&random).unwrap();
        svm.set_account(USDC_MINT, mint_account).unwrap();
        let mut env = Self {
            svm,
            admin,
            usdc: USDC_MINT,
            next_key: 1,
        };
        env.set_time(NOW);
        env
    }

    /// [`Env::bare`] plus the config and both calendars.
    pub fn new() -> Self {
        Self::with_params(Params::default_params())
    }

    /// [`Env::new`] with other config parameters.
    pub fn with_params(params: Params) -> Self {
        let mut env = Self::bare();
        let admin = env.admin.insecure_clone();
        env.send(&[env.ix_init_config(&admin.pubkey(), params)], &[&admin])
            .unwrap();
        env.send(
            &[env.ix_set_calendar(&admin.pubkey(), XAU, &[EXPIRY, EXPIRY_2])],
            &[&admin],
        )
        .unwrap();
        env.send(
            &[env.ix_set_calendar(&admin.pubkey(), XAG, &[EXPIRY])],
            &[&admin],
        )
        .unwrap();
        env
    }

    pub fn set_time(&mut self, unix_timestamp: i64) {
        let mut clock: Clock = self.svm.get_sysvar();
        clock.unix_timestamp = unix_timestamp;
        self.svm.set_sysvar(&clock);
    }

    /// Sends `ixs` paid by the first signer. Each call gets a fresh blockhash, so identical
    /// transactions do not collide.
    pub fn send(&mut self, ixs: &[Instruction], signers: &[&Keypair]) -> Sent {
        self.svm.expire_blockhash();
        let tx = Transaction::new_signed_with_payer(
            ixs,
            Some(&signers[0].pubkey()),
            signers,
            self.svm.latest_blockhash(),
        );
        match self.svm.send_transaction(tx) {
            Ok(meta) => Ok(meta.compute_units_consumed),
            Err(e) => Err((format!("{:?}", e.err), e.meta.logs)),
        }
    }

    pub fn funded(&mut self) -> Keypair {
        let kp = keypair(self.next_key);
        self.next_key += 1;
        self.svm.airdrop(&kp.pubkey(), 100_000_000_000).unwrap();
        kp
    }

    /// A writer with `usdc_usd` USDC in its associated account.
    pub fn writer(&mut self, usdc_usd: u64) -> Keypair {
        let w = self.funded();
        let admin = self.admin.insecure_clone();
        let usdc = self.usdc;
        let account = CreateAssociatedTokenAccount::new(&mut self.svm, &admin, &usdc)
            .owner(&w.pubkey())
            .send()
            .unwrap();
        MintTo::new(&mut self.svm, &admin, &usdc, &account, usdc_usd * USD)
            .send()
            .unwrap();
        w
    }

    /// Creates `owner`'s associated token account for `mint` (paid by the admin).
    pub fn token_account(&mut self, mint: &Address, owner: &Address) -> Address {
        let admin = self.admin.insecure_clone();
        CreateAssociatedTokenAccount::new(&mut self.svm, &admin, mint)
            .owner(owner)
            .send()
            .unwrap()
    }

    pub fn token_balance(&self, account: &Address) -> u64 {
        let data = self.svm.get_account(account).unwrap().data;
        u64::from_le_bytes(data[64..72].try_into().unwrap())
    }

    pub fn account(&self, address: &Address) -> Account {
        self.svm.get_account(address).unwrap()
    }

    /// Copies `from`'s account to `to`, optionally with another owner: a forged account.
    pub fn forge(&mut self, from: &Address, to: Address, owner: Option<Address>) {
        let mut a = self.account(from);
        if let Some(o) = owner {
            a.owner = o;
        }
        self.svm.set_account(to, a).unwrap();
    }

    pub fn create_series(&mut self, args: SeriesArgs) -> Address {
        let payer = self.funded();
        self.send(&[self.ix_create_series(&payer.pubkey(), args)], &[&payer])
            .unwrap();
        args.series()
    }

    // Instruction builders.

    pub fn ix_init_config(&self, admin: &Address, params: Params) -> Instruction {
        let mut data = vec![0u8];
        data.extend(params.encode());
        Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                meta(*admin, true, true),
                meta(config_address(), false, true),
                meta(program_data_address(), false, false),
                meta(self.usdc, false, false),
                meta(SYSTEM_PROGRAM, false, false),
            ],
            data,
        }
    }

    pub fn ix_set_config(
        &self,
        admin: &Address,
        new_admin: &Address,
        params: Params,
    ) -> Instruction {
        let mut data = vec![1u8];
        data.extend_from_slice(new_admin.as_ref());
        data.extend(params.encode());
        Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                meta(*admin, true, false),
                meta(config_address(), false, true),
            ],
            data,
        }
    }

    pub fn ix_set_calendar(
        &self,
        admin: &Address,
        underlying: u8,
        expiries: &[i64],
    ) -> Instruction {
        let mut data = vec![2u8, underlying, expiries.len() as u8];
        for e in expiries {
            data.extend_from_slice(&e.to_le_bytes());
        }
        Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                meta(*admin, true, true),
                meta(config_address(), false, false),
                meta(calendar_address(underlying), false, true),
                meta(SYSTEM_PROGRAM, false, false),
            ],
            data,
        }
    }

    pub fn ix_create_series(&self, payer: &Address, args: SeriesArgs) -> Instruction {
        let series = args.series();
        Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                meta(*payer, true, true),
                meta(config_address(), false, false),
                meta(calendar_address(args.underlying), false, false),
                meta(series, false, true),
                meta(args.long_mint(), false, true),
                meta(ata(&series, &self.usdc), false, true),
                meta(self.usdc, false, false),
                meta(TOKEN_PROGRAM, false, false),
                meta(ATA_PROGRAM, false, false),
                meta(SYSTEM_PROGRAM, false, false),
            ],
            data: args.encode(),
        }
    }

    /// `mint` (tag 4) or `close` (tag 5) with the writer's own accounts.
    pub fn ix_position(&self, tag: u8, writer: &Address, args: SeriesArgs, n: u64) -> Instruction {
        let series = args.series();
        let long_mint = args.long_mint();
        let mut data = vec![tag];
        data.extend_from_slice(&n.to_le_bytes());
        let mut accounts = vec![
            meta(*writer, true, true),
            meta(series, false, true),
            meta(position_address(&series, writer), false, true),
            meta(ata(writer, &self.usdc), false, true),
            meta(ata(&series, &self.usdc), false, true),
            meta(self.usdc, false, false),
            meta(long_mint, false, true),
            meta(ata(writer, &long_mint), false, true),
            meta(TOKEN_PROGRAM, false, false),
        ];
        if tag == 4 {
            accounts.push(meta(SYSTEM_PROGRAM, false, false));
        }
        Instruction {
            program_id: PROGRAM_ID,
            accounts,
            data,
        }
    }

    pub fn ix_sample(&self, series: &Address, price_update: &Address) -> Instruction {
        Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                meta(*series, false, true),
                meta(*price_update, false, false),
            ],
            data: vec![6],
        }
    }

    pub fn ix_settle(&self, series: &Address) -> Instruction {
        Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![meta(*series, false, true)],
            data: vec![7],
        }
    }

    /// Stores a `PriceUpdateV2` account at a fresh address and returns it.
    pub fn price_update(&mut self, u: PriceUpdateArgs) -> Address {
        let address = Address::new_unique();
        let lamports = self.svm.minimum_balance_for_rent_exemption(134);
        self.svm
            .set_account(
                address,
                Account {
                    lamports,
                    data: u.encode(),
                    owner: u.owner,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        address
    }

    /// Posts a `Full` XAU print to `series` (or another kind of update with `edit`).
    pub fn post(&mut self, series: &Address, price: i64, publish: i64, prev: i64) -> Sent {
        let pu = self.price_update(PriceUpdateArgs::xau(price, publish, prev));
        let ix = self.ix_sample(series, &pu);
        let payer = self.admin.insecure_clone();
        self.send(&[ix], &[&payer])
    }

    pub fn settle(&mut self, series: &Address) -> Sent {
        let ix = self.ix_settle(series);
        let payer = self.admin.insecure_clone();
        self.send(&[ix], &[&payer])
    }

    /// `redeem` (tag 8) with the holder's own accounts.
    pub fn ix_redeem(&self, holder: &Address, args: SeriesArgs, n: u64) -> Instruction {
        let series = args.series();
        let long_mint = args.long_mint();
        let mut data = vec![8u8];
        data.extend_from_slice(&n.to_le_bytes());
        Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                meta(*holder, true, false),
                meta(series, false, true),
                meta(ata(holder, &long_mint), false, true),
                meta(ata(holder, &self.usdc), false, true),
                meta(ata(&series, &self.usdc), false, true),
                meta(self.usdc, false, false),
                meta(long_mint, false, true),
                meta(TOKEN_PROGRAM, false, false),
            ],
            data,
        }
    }

    /// `claim_writer` (tag 9) with the writer's own accounts.
    pub fn ix_claim(&self, writer: &Address, args: SeriesArgs) -> Instruction {
        let series = args.series();
        Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                meta(*writer, true, false),
                meta(series, false, true),
                meta(position_address(&series, writer), false, true),
                meta(ata(writer, &self.usdc), false, true),
                meta(ata(&series, &self.usdc), false, true),
                meta(self.usdc, false, false),
                meta(TOKEN_PROGRAM, false, false),
            ],
            data: vec![9],
        }
    }

    /// `close_series` (tag 10), with the rent payer's associated USDC account.
    pub fn ix_close_series(&self, args: SeriesArgs, rent_payer: &Address) -> Instruction {
        let series = args.series();
        Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                meta(series, false, true),
                meta(ata(&series, &self.usdc), false, true),
                meta(*rent_payer, false, true),
                meta(ata(rent_payer, &self.usdc), false, true),
                meta(self.usdc, false, false),
                meta(TOKEN_PROGRAM, false, false),
            ],
            data: vec![10],
        }
    }

    /// `close_position` (tag 11).
    pub fn ix_close_position(&self, position: &Address, owner: &Address) -> Instruction {
        Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![meta(*position, false, true), meta(*owner, false, true)],
            data: vec![11],
        }
    }

    /// Fills the whole window of `args` with canonical prints at the raw mantissas `prices`
    /// (one per slot, the feed and exponent of the underlying) and settles at expiry.
    pub fn settle_with_window(&mut self, args: SeriesArgs, prices: &[i64; 30]) {
        let series = args.series();
        self.set_time(args.expiry + 10);
        let start = args.expiry - WINDOW_S;
        for (i, &px) in prices.iter().enumerate() {
            let b = start + i as i64 * SLOT_S;
            let pu = self.price_update(PriceUpdateArgs::for_underlying(
                args.underlying,
                px,
                b + 1,
                b - 1,
            ));
            let payer = self.admin.insecure_clone();
            self.send(&[self.ix_sample(&series, &pu)], &[&payer])
                .unwrap();
        }
        self.settle(&series).unwrap();
    }

    pub fn ix_mint(&self, writer: &Address, args: SeriesArgs, n: u64) -> Instruction {
        self.ix_position(4, writer, args, n)
    }

    pub fn ix_close(&self, writer: &Address, args: SeriesArgs, n: u64) -> Instruction {
        self.ix_position(5, writer, args, n)
    }

    /// Ensures the writer has a long-token account for the series.
    pub fn long_account(&mut self, writer: &Address, args: SeriesArgs) -> Address {
        let mint = args.long_mint();
        let address = ata(writer, &mint);
        if self.svm.get_account(&address).is_none() {
            self.token_account(&mint, writer);
        }
        address
    }
}

/// Fails unless `result` is the custom program error `code`.
#[track_caller]
pub fn expect_code(result: Sent, code: u32) {
    match result {
        Ok(_) => panic!("expected custom error {code}, the transaction succeeded"),
        Err((err, logs)) => assert!(
            err.contains(&format!("Custom({code})")),
            "expected custom error {code}, got {err}\n{}",
            logs.join("\n")
        ),
    }
}

/// Series state read from its account (offsets of the layout in `docs/PLAN.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeriesView {
    pub state: u8,
    pub open: u64,
    pub longs_outstanding: u64,
    pub writers_unclaimed: u64,
    pub vault_owed: u64,
}

pub fn read_series(env: &Env, series: &Address) -> SeriesView {
    let d = env.account(series).data;
    assert_eq!(d.len(), 504);
    let u = |o: usize| u64::from_le_bytes(d[o..o + 8].try_into().unwrap());
    SeriesView {
        state: d[4],
        open: u(168),
        longs_outstanding: u(176),
        writers_unclaimed: u(184),
        vault_owed: u(208),
    }
}

/// `written` of a writer position.
pub fn read_written(env: &Env, position: &Address) -> u64 {
    let d = env.account(position).data;
    assert_eq!(d.len(), 80);
    u64::from_le_bytes(d[72..80].try_into().unwrap())
}

/// A `PriceUpdateV2` account (pyth-solana-receiver-sdk 2.0.0 layout), restated here.
#[derive(Debug, Clone, Copy)]
pub struct PriceUpdateArgs {
    pub owner: Address,
    pub discriminator: [u8; 8],
    /// `None` is `Full`, `Some(n)` is `Partial { num_signatures: n }`.
    pub partial: Option<u8>,
    pub feed_id: [u8; 32],
    pub price: i64,
    pub conf: u64,
    pub exponent: i32,
    pub publish: i64,
    pub prev: i64,
}

impl PriceUpdateArgs {
    pub const DISCRIMINATOR: [u8; 8] = [0x22, 0xf1, 0x23, 0x63, 0x9d, 0x7e, 0xf4, 0xcd];

    /// A `Full` XAU/USD print, exponent -3, confidence 0.01% of the price.
    pub fn xau(price: i64, publish: i64, prev: i64) -> Self {
        Self {
            owner: PYTH_RECEIVER,
            discriminator: Self::DISCRIMINATOR,
            partial: None,
            feed_id: FEED_XAU,
            price,
            conf: (price.unsigned_abs() / 10_000).max(1),
            exponent: i32::from(EXPO_XAU),
            publish,
            prev,
        }
    }

    /// A `Full` print for `underlying`'s feed and pinned exponent.
    pub fn for_underlying(underlying: u8, price: i64, publish: i64, prev: i64) -> Self {
        let mut a = Self::xau(price, publish, prev);
        if underlying == XAG {
            a.feed_id = FEED_XAG;
            a.exponent = i32::from(EXPO_XAG);
        }
        a
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut d = Vec::with_capacity(134);
        d.extend_from_slice(&self.discriminator);
        d.extend_from_slice(&[7u8; 32]); // write authority
        match self.partial {
            None => d.push(1),
            Some(n) => d.extend_from_slice(&[0, n]),
        }
        d.extend_from_slice(&self.feed_id);
        d.extend_from_slice(&self.price.to_le_bytes());
        d.extend_from_slice(&self.conf.to_le_bytes());
        d.extend_from_slice(&self.exponent.to_le_bytes());
        d.extend_from_slice(&self.publish.to_le_bytes());
        d.extend_from_slice(&self.prev.to_le_bytes());
        d.extend_from_slice(&self.price.to_le_bytes()); // ema price
        d.extend_from_slice(&self.conf.to_le_bytes()); // ema conf
        d.extend_from_slice(&1u64.to_le_bytes()); // posted slot
        d.resize(134, 0);
        d
    }
}

/// Settlement fields of a series (offsets of the version-2 layout).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettlementView {
    pub state: u8,
    pub filled_slots: u32,
    pub next_len: u8,
    pub method: u8,
    pub price: u64,
    pub samples: u16,
    pub payout: u64,
    pub residual: u64,
}

pub fn read_settlement(env: &Env, series: &Address) -> SettlementView {
    let d = env.account(series).data;
    assert_eq!(d.len(), 504);
    let u64_at = |o: usize| u64::from_le_bytes(d[o..o + 8].try_into().unwrap());
    SettlementView {
        state: d[4],
        filled_slots: u32::from_le_bytes(d[328..332].try_into().unwrap()),
        next_len: d[332],
        method: d[333],
        price: u64_at(456),
        samples: u16::from_le_bytes(d[464..466].try_into().unwrap()),
        payout: u64_at(192),
        residual: u64_at(200),
    }
}
