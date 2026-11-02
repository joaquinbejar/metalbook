//! Target `sample_settlement`: the program's `sample_settlement` and `settle` in LiteSVM, on
//! `PriceUpdateV2` accounts built from seeded valid prints, the same prints with one field
//! mutated, or raw bytes, owned by the Pyth receiver or by forged owners, with the clock moved
//! through the series' whole life (active, the window, expiry, the challenge delays, the hard
//! deadline), so the series is sampled in every state.
//!
//! Acceptance oracle: before each transaction, the decoder of `pyth.rs` and the models of
//! `model.rs` predict the outcome, independently of pyth-lite and metalbook-core: accept (and
//! the exact series account afterwards) or reject (and the exact error code). Any
//! disagreement, in either direction, is a crash, so a program that rejects everything fails
//! as surely as one that accepts too much. On every transaction, accepted or not, the price
//! update, the vault and the long mint are unchanged, a rejected one leaves the series
//! byte-identical, and the fee payer pays exactly the fee (one signature).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::integer_division,
    clippy::disallowed_macros,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    dead_code,
    missing_docs
)]

use std::cell::RefCell;

use arbitrary::Arbitrary;
use litesvm::LiteSVM;
use solana_account::Account;
use solana_address::Address;
use solana_keypair::Keypair;
use solana_signer::Signer;
use solana_transaction::Transaction;

use crate::common::{
    ata, keypair, Env, SeriesArgs, D, EXPIRY, FEED_XAG, FEED_XAU, HARD_S, LOOKBACK_S, MIN_SAMPLES,
    PROGRAM_ID, PYTH_RECEIVER, SLOT_S, SYSTEM_PROGRAM, TOKEN_PROGRAM, TOLERANCE_S, USD, WINDOW_S,
};
use crate::model::{Candidates, Outcome, Payoff, Px, Terms};
use crate::pyth::{self, Decoded};

/// LiteSVM's fee per signature.
pub const FEE: u64 = 5_000;
/// The default config's confidence bound, basis points.
const MAX_CONF_BPS: u128 = 100;

/// `MetalbookError` codes the oracle predicts (`metalbook-program/src/error.rs`).
pub mod code {
    pub const SERIES_NOT_EXPIRED: u32 = 114;
    pub const SERIES_ALREADY_SETTLED: u32 = 116;
    pub const ORACLE_OWNER_MISMATCH: u32 = 119;
    pub const ORACLE_NOT_FULLY_VERIFIED: u32 = 120;
    pub const ORACLE_FEED_MISMATCH: u32 = 121;
    pub const ORACLE_CONFIDENCE_TOO_WIDE: u32 = 123;
    pub const SAMPLE_OUTSIDE_WINDOW: u32 = 124;
    pub const OVERFLOW: u32 = 126;
    pub const INVALID_ACCOUNT_DATA: u32 = 128;
    pub const SETTLEMENT_NOT_READY: u32 = 141;
    pub const ORACLE_EXPONENT_MISMATCH: u32 = 142;
    pub const INVALID_PRINT: u32 = 143;
}

/// One of the two listed series.
#[derive(Debug, Clone, Copy)]
pub struct Listed {
    pub args: SeriesArgs,
    pub feed: [u8; 32],
    pub expo: i32,
    /// Raw mantissa of the strike.
    pub base: i64,
}

pub fn listed(xag: bool) -> Listed {
    if xag {
        // XAG capped call 50 / 55 at 50 oz, exponent -5.
        let args = SeriesArgs {
            underlying: crate::common::XAG,
            kind: crate::common::CALL,
            strike: 50 * USD,
            expiry: EXPIRY,
            cap: 55 * USD,
            multiplier: crate::common::MULT_XAG,
        };
        Listed {
            args,
            feed: FEED_XAG,
            expo: -5,
            base: 5_000_000,
        }
    } else {
        Listed {
            args: SeriesArgs::xau_put(4_000),
            feed: FEED_XAU,
            expo: -3,
            base: 4_000_000,
        }
    }
}

pub fn terms(expo: i32) -> Terms {
    Terms {
        window_s: WINDOW_S,
        slot_s: SLOT_S,
        tolerance_s: TOLERANCE_S,
        min_samples: u32::from(MIN_SAMPLES),
        k: 5,
        d: D,
        lookback_s: LOOKBACK_S,
        hard_s: HARD_S,
        expo,
    }
}

pub fn payoff(a: &SeriesArgs) -> Payoff {
    Payoff {
        kind: a.kind,
        strike: a.strike,
        cap: a.cap,
        mult: a.multiplier,
    }
}

/// Where a seeded print is aimed.
#[derive(Debug, Clone, Copy, Arbitrary)]
pub enum Aim {
    Slot { slot: u8, late: u8, gap: u8 },
    Link { step: u16, repeat_prev: bool },
    Fallback { back: u32, same_second: bool },
    Free { publish_off: i32, gap: u32 },
}

/// One mutated field of a seeded print.
#[derive(Debug, Clone, Copy, Arbitrary)]
pub enum Field {
    Partial(u8),
    Level(u8),
    Feed(u8, u8),
    Price(i64),
    Conf(u64),
    Exponent(i32),
    Publish(i64),
    Prev(i64),
    Discriminator(u8, u8),
    Truncate(u8),
}

#[derive(Debug, Clone, Arbitrary)]
pub enum Data {
    Valid {
        aim: Aim,
        price_off: i16,
    },
    Mutated {
        aim: Aim,
        price_off: i16,
        field: Field,
    },
    Raw(Vec<u8>),
}

#[derive(Debug, Clone, Copy, Arbitrary)]
pub enum Owner {
    Receiver,
    System,
    Program,
    Token,
}

#[derive(Debug, Clone, Arbitrary)]
pub enum Step {
    Sample {
        data: Data,
        owner: Owner,
        dt: u16,
    },
    Settle {
        dt: u16,
    },
    /// Moves the clock to a life boundary plus `off`: the window start, expiry, expiry plus
    /// `D`, the hard deadline, the hard deadline plus `D` (never backwards).
    Jump {
        to: u8,
        off: i16,
    },
}

#[derive(Debug, Clone, Arbitrary)]
pub struct Input {
    pub xag: bool,
    pub steps: Vec<Step>,
}

/// The model of one series: its state discriminant, candidates and settlement fields.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SeriesModel {
    pub state: u8,
    pub cands: Candidates,
    pub payout: u64,
    pub residual: u64,
    pub method: u8,
    pub price: u64,
    pub samples: u16,
}

impl SeriesModel {
    pub fn new() -> Self {
        Self {
            method: u8::MAX,
            ..Self::default()
        }
    }

    /// Writes the model's fields into a copy of the series account `before`.
    pub fn apply(&self, before: &[u8]) -> Vec<u8> {
        let mut s: metalbook_core::accounts::Series = bytemuck::pod_read_unaligned(before);
        s.state = self.state;
        s.payout = self.payout;
        s.residual = self.residual;
        s.settle_method = self.method;
        s.settlement_price = self.price;
        s.settlement_samples = self.samples;
        let c = &self.cands;
        s.sample_sum = c.sample_sum().to_le_bytes();
        s.filled_slots = c.bitmap();
        s.next_len = c.chain.len() as u8;
        s.next_times = [0; 5];
        s.next_prices = [0; 5];
        for (i, &(t, p)) in c.chain.iter().enumerate() {
            s.next_times[i] = t;
            s.next_prices[i] = p;
        }
        let (ft, fp) = c.fallback.unwrap_or((0, 0));
        s.fallback_time = ft;
        s.fallback_price = fp;
        s.window_changed = c.window_changed;
        s.next_print_changed = c.next_changed;
        s.fallback_changed = c.fallback_changed;
        bytemuck::bytes_of(&s).to_vec()
    }

    /// The program's `sample_settlement` as the oracle sees it.
    pub fn sample(
        &self,
        l: &Listed,
        receiver_owned: bool,
        data: &[u8],
        now: i64,
    ) -> Result<Self, u32> {
        if self.state >= 2 {
            return Err(code::SERIES_ALREADY_SETTLED);
        }
        if !receiver_owned {
            return Err(code::ORACLE_OWNER_MISMATCH);
        }
        let d = pyth::decode(data).ok_or(code::INVALID_ACCOUNT_DATA)?;
        if d.partial.is_some() {
            return Err(code::ORACLE_NOT_FULLY_VERIFIED);
        }
        if d.feed != l.feed {
            return Err(code::ORACLE_FEED_MISMATCH);
        }
        if d.price <= 0 {
            return Err(code::INVALID_PRINT);
        }
        let price = d.price as u64;
        // Pyth's own bound: conf at most the price.
        if u128::from(d.conf) > u128::from(price) {
            return Err(code::ORACLE_CONFIDENCE_TOO_WIDE);
        }
        if d.exponent != l.expo {
            return Err(code::ORACLE_EXPONENT_MISMATCH);
        }
        if u128::from(d.conf) * 10_000 > u128::from(price) * MAX_CONF_BPS {
            return Err(code::ORACLE_CONFIDENCE_TOO_WIDE);
        }
        let x = Px {
            price,
            publish: d.publish,
            prev: d.prev,
        };
        if !x.valid() {
            return Err(code::INVALID_PRINT);
        }
        let mut next = self.clone();
        next.cands
            .offer(&terms(l.expo), EXPIRY, x, now)
            .ok_or(code::SAMPLE_OUTSIDE_WINDOW)?;
        if now >= EXPIRY && next.state == 0 {
            next.state = 1;
        }
        Ok(next)
    }

    /// The program's `settle` as the oracle sees it.
    pub fn settle(&self, l: &Listed, now: i64) -> Result<Self, u32> {
        if now < EXPIRY {
            return Err(code::SERIES_NOT_EXPIRED);
        }
        if self.state >= 2 {
            return Err(code::SERIES_ALREADY_SETTLED);
        }
        let pay = payoff(&l.args);
        let mut next = self.clone();
        match self.cands.decide(&terms(l.expo), EXPIRY, now) {
            Outcome::Wait(_) => return Err(code::SETTLEMENT_NOT_READY),
            Outcome::Overflow => return Err(code::OVERFLOW),
            Outcome::Settle(method, price, samples) => {
                next.state = 2;
                next.payout = pay.payout(price);
                next.residual = pay.residual(price);
                next.method = method;
                next.price = price;
                next.samples = samples;
            }
            Outcome::Void => {
                next.state = 3;
                next.payout = 0;
                next.residual = pay.collateral();
                next.method = 5;
                next.price = 0;
                next.samples = 0;
            }
        }
        Ok(next)
    }
}

/// A seeded print for `aim`, valid for the series unless the aim itself is off.
pub fn seeded(l: &Listed, m: &SeriesModel, aim: Aim, price_off: i16) -> Decoded {
    let price = (l.base + i64::from(price_off) * (l.base / 2_000)).max(1);
    let (publish, prev) = match aim {
        Aim::Slot { slot, late, gap } => {
            let i = i64::from(slot % 30);
            let t_i = EXPIRY - WINDOW_S + i * SLOT_S;
            let publish = t_i + i64::from(late) % (TOLERANCE_S + 2);
            (publish, publish - 1 - i64::from(gap) % 90)
        }
        Aim::Link { step, repeat_prev } => {
            let last = m.cands.chain.last().map_or(EXPIRY, |&(t, _)| t);
            let publish = last + 1 + i64::from(step % 400);
            (publish, if repeat_prev { publish } else { last })
        }
        Aim::Fallback { back, same_second } => {
            let publish = EXPIRY - i64::from(back) % (LOOKBACK_S + 600);
            (publish, if same_second { publish } else { publish - 1 })
        }
        Aim::Free { publish_off, gap } => {
            let publish = EXPIRY + i64::from(publish_off);
            (publish, publish - i64::from(gap))
        }
    };
    Decoded {
        partial: None,
        feed: l.feed,
        price,
        conf: (price / 10_000).max(1) as u64,
        exponent: l.expo,
        publish,
        prev,
    }
}

pub fn bytes_of(l: &Listed, m: &SeriesModel, data: &Data) -> Vec<u8> {
    match data {
        Data::Raw(b) => b.iter().copied().take(256).collect(),
        Data::Valid { aim, price_off } => pyth::encode(&seeded(l, m, *aim, *price_off)),
        Data::Mutated {
            aim,
            price_off,
            field,
        } => {
            let mut d = seeded(l, m, *aim, *price_off);
            match *field {
                Field::Partial(n) => d.partial = Some(n),
                Field::Price(p) => d.price = p,
                Field::Conf(c) => d.conf = c,
                Field::Exponent(e) => d.exponent = e,
                Field::Publish(t) => d.publish = t,
                Field::Prev(t) => d.prev = t,
                Field::Feed(i, x) => d.feed[usize::from(i % 32)] ^= x,
                Field::Level(_) | Field::Discriminator(_, _) | Field::Truncate(_) => {}
            }
            let mut v = pyth::encode(&d);
            match *field {
                Field::Level(tag) => v[40] = tag,
                Field::Discriminator(i, x) => v[usize::from(i % 8)] ^= x,
                Field::Truncate(n) => v.truncate(usize::from(n) % 134),
                _ => {}
            }
            v
        }
    }
}

/// The program, config, calendars and both series, built once per process; each input runs on
/// a clone of its LiteSVM. `env` also builds the instructions.
struct Base {
    env: Env,
    payer: Keypair,
}

thread_local! {
    static BASE: RefCell<Option<Base>> = const { RefCell::new(None) };
}

pub fn set_clock(svm: &mut LiteSVM, now: i64) {
    let mut clock: solana_clock::Clock = svm.get_sysvar();
    clock.unix_timestamp = now;
    clock.slot += 1;
    svm.set_sysvar(&clock);
}

/// The custom error code of a failed transaction, or `None` for a non-custom error.
pub fn custom_code(err: &str) -> Option<u32> {
    let rest = err.strip_prefix("InstructionError(0, Custom(")?;
    rest.strip_suffix("))")?.parse().ok()
}

fn send(
    svm: &mut LiteSVM,
    payer: &Keypair,
    ix: solana_instruction::Instruction,
) -> Result<(), String> {
    svm.expire_blockhash();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[payer],
        svm.latest_blockhash(),
    );
    svm.send_transaction(tx)
        .map(|_| ())
        .map_err(|e| format!("{:?}", e.err))
}

/// Transactions of one input, by outcome.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    pub accepted: u32,
    pub rejected: u32,
}

/// Runs one input.
pub fn run(input: &Input) -> Stats {
    BASE.with(|b| {
        let mut b = b.borrow_mut();
        let base = b.get_or_insert_with(|| {
            let mut env = Env::new();
            env.create_series(listed(false).args);
            env.create_series(listed(true).args);
            let payer = env.funded();
            Base { env, payer }
        });
        run_on(&base.env, base.env.svm.clone(), &base.payer, input)
    })
}

fn run_on(env_for_ix: &Env, mut svm: LiteSVM, payer: &Keypair, input: &Input) -> Stats {
    let mut stats = Stats::default();
    let l = listed(input.xag);
    let series = l.args.series();
    let vault = ata(&series, &crate::common::USDC_MINT);
    let long_mint = l.args.long_mint();
    let mut model = SeriesModel::new();
    let mut now = EXPIRY - WINDOW_S - 120;

    for step in input.steps.iter().take(40) {
        let (ix, want, watched) = match step {
            Step::Jump { to, off } => {
                let hard = EXPIRY + HARD_S;
                let at = match to % 5 {
                    0 => EXPIRY - WINDOW_S,
                    1 => EXPIRY,
                    2 => EXPIRY + D,
                    3 => hard,
                    _ => hard + D,
                };
                now = now.max(at + i64::from(*off));
                continue;
            }
            Step::Settle { dt } => {
                now += i64::from(*dt);
                (env_for_ix.ix_settle(&series), model.settle(&l, now), None)
            }
            Step::Sample { data, owner, dt } => {
                now += i64::from(*dt);
                let bytes = bytes_of(&l, &model, data);
                let owner_id = match owner {
                    Owner::Receiver => PYTH_RECEIVER,
                    Owner::System => SYSTEM_PROGRAM,
                    Owner::Program => PROGRAM_ID,
                    Owner::Token => TOKEN_PROGRAM,
                };
                let at = Address::new_unique();
                let lamports = svm.minimum_balance_for_rent_exemption(bytes.len());
                svm.set_account(
                    at,
                    Account {
                        lamports,
                        data: bytes.clone(),
                        owner: owner_id,
                        executable: false,
                        rent_epoch: 0,
                    },
                )
                .unwrap();
                let want = model.sample(&l, owner_id == PYTH_RECEIVER, &bytes, now);
                (env_for_ix.ix_sample(&series, &at), want, Some(at))
            }
        };
        set_clock(&mut svm, now);
        let before = svm.get_account(&series).unwrap().data;
        let watched_before = watched.map(|a| svm.get_account(&a).unwrap());
        let vault_before = svm.get_account(&vault).unwrap();
        let mint_before = svm.get_account(&long_mint).unwrap();
        let lamports_before = svm.get_balance(&payer.pubkey()).unwrap();

        let got = send(&mut svm, payer, ix);

        let after = svm.get_account(&series).unwrap().data;
        assert_eq!(
            svm.get_balance(&payer.pubkey()).unwrap(),
            lamports_before - FEE,
            "the fee payer paid something other than the fee"
        );
        if let (Some(a), Some(b)) = (watched, watched_before) {
            assert_eq!(svm.get_account(&a).unwrap(), b, "the price update changed");
        }
        assert_eq!(
            svm.get_account(&vault).unwrap(),
            vault_before,
            "the vault changed"
        );
        assert_eq!(
            svm.get_account(&long_mint).unwrap(),
            mint_before,
            "the long mint changed"
        );
        match (got, want) {
            (Ok(()), Ok(next)) => {
                assert_eq!(
                    after,
                    next.apply(&before),
                    "series differs from the model at {now}"
                );
                model = next;
                stats.accepted += 1;
            }
            (Err(e), Err(want)) => {
                assert_eq!(
                    custom_code(&e),
                    Some(want),
                    "error {e}, model {want} at {now}"
                );
                assert_eq!(after, before, "a rejected transaction wrote the series");
                stats.rejected += 1;
            }
            (Ok(()), Err(want)) => panic!("accepted on chain, model rejects with {want} at {now}"),
            (Err(e), Ok(_)) => panic!("rejected on chain with {e}, model accepts at {now}"),
        }
    }
    stats
}

/// The admin keypair of [`Env`] (fixed), for harnesses that sign with it.
pub fn admin() -> Keypair {
    keypair(0)
}
