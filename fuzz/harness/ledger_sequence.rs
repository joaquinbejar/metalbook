//! Target `ledger_sequence`: arbitrary sequences of `mint`, `close`, long-token transfers,
//! USDC donations to the vault, window samples, `settle`, `redeem`, `claim_writer`,
//! `close_series` and `close_position` on one series in LiteSVM, with clock warps through its
//! life, three writers and two holders. Every case ends with an explicit draining suffix:
//! settle (or void), redeem every long token, claim every position, close the series, close
//! every position.
//!
//! Invariants:
//! - **Rollback**: a failing transaction leaves every account it touches (series, vault, long
//!   mint, positions, token accounts, the rent payer) byte-identical, and the fee payer's
//!   lamports drop by exactly the fee (one signature), its data unchanged.
//! - **Conservation** while the series is open: the vault's token balance is the ledger's
//!   `vault_owed` plus the donations it received; the long mint's supply is the ledger's
//!   `longs_outstanding`; the USDC of everyone plus the vault equals the initial USDC plus the
//!   donations.
//! - **Amounts**, from the payoff model of `model.rs` and the stored settlement price: a
//!   redeem pays `n × payout`, a claim pays `written × residual`, a void pays the whole
//!   collateral back to writers and nothing to longs.
//! - **Closing**: `close_series` sends exactly the donations to the rent payer's USDC account
//!   and exactly the series' and vault's lamports to the rent payer, and both accounts are
//!   gone; `close_position` returns exactly the position's lamports to its writer.
//! - **Draining**: after the suffix every owed atom is paid (the USDC of everyone equals the
//!   initial USDC plus the donations), the vault and series are closed and no long token is
//!   left.

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
use litesvm_token::MintTo;
use solana_account::Account;
use solana_address::Address;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_signer::Signer;
use solana_transaction::Transaction;

use crate::common::{
    ata, position_address, Env, SeriesArgs, D, EXPIRY, HARD_S, SLOT_S, TOKEN_PROGRAM, USD,
    USDC_MINT, WINDOW_S,
};
use crate::model::Payoff;
use crate::pyth::{self, Decoded};
use crate::sample_settlement::{set_clock, FEE};

/// Writers (0 to 2) start with this USDC; holders (3 and 4) with none.
const WRITER_USDC: u64 = 50_000 * USD;
const ACTORS: usize = 5;

/// The three series a case can run on: an XAU put, an XAU capped call, an XAG capped call at
/// 50 oz.
fn series_args(i: u8) -> (SeriesArgs, [u8; 32], i32, i64) {
    match i % 3 {
        0 => (
            SeriesArgs::xau_put(4_000),
            crate::common::FEED_XAU,
            -3,
            4_000_000,
        ),
        1 => (
            SeriesArgs::xau_call(4_000),
            crate::common::FEED_XAU,
            -3,
            4_000_000,
        ),
        _ => (
            SeriesArgs {
                underlying: crate::common::XAG,
                kind: crate::common::CALL,
                strike: 50 * USD,
                expiry: EXPIRY,
                cap: 55 * USD,
                multiplier: crate::common::MULT_XAG,
            },
            crate::common::FEED_XAG,
            -5,
            5_000_000,
        ),
    }
}

#[derive(Debug, Clone, Copy, Arbitrary)]
pub enum Op {
    Mint { a: u8, n: u8 },
    Close { a: u8, n: u8 },
    Transfer { from: u8, to: u8, n: u8 },
    Donate { amount: u32 },
    Sample { slot: u8, price_off: i16 },
    Warp { to: u8, off: i16 },
    Settle,
    Redeem { a: u8, n: u8 },
    Claim { a: u8 },
    CloseSeries,
    ClosePosition { a: u8 },
}

#[derive(Debug, Clone, Arbitrary)]
pub struct Input {
    pub series: u8,
    pub ops: Vec<Op>,
    /// End from the hard deadline (a void when nothing was sampled, else the cascade's partial
    /// step), or by settling on a full window at `end_price_off` from the strike.
    pub end_void: bool,
    pub end_price_off: i16,
}

struct Base {
    env: Env,
    actors: Vec<Keypair>,
    keeper: Keypair,
}

thread_local! {
    static BASE: RefCell<Option<Base>> = const { RefCell::new(None) };
}

/// Runs one input.
pub fn run(input: &Input) {
    BASE.with(|b| {
        let mut b = b.borrow_mut();
        let base = b.get_or_insert_with(|| {
            let mut env = Env::new();
            let mut actors = Vec::new();
            for i in 0..ACTORS {
                actors.push(if i < 3 {
                    env.writer(WRITER_USDC / USD)
                } else {
                    let h = env.funded();
                    env.token_account(&USDC_MINT, &h.pubkey());
                    h
                });
            }
            for s in 0..3 {
                let (args, ..) = series_args(s);
                let series = env.create_series(args);
                let rent_payer = rent_payer(&env.svm, &series);
                env.token_account(&USDC_MINT, &rent_payer);
                for a in &actors {
                    env.token_account(&args.long_mint(), &a.pubkey());
                }
            }
            let keeper = env.funded();
            Base {
                env,
                actors,
                keeper,
            }
        });
        let mut case = Case {
            env: &base.env,
            svm: base.env.svm.clone(),
            actors: &base.actors,
            keeper: &base.keeper,
            series: input.series,
            now: crate::common::NOW,
            donated: 0,
            donations_total: 0,
            initial_usdc: 0,
            closed: false,
        };
        case.run(input);
    });
}

fn rent_payer(svm: &LiteSVM, series: &Address) -> Address {
    let d = svm.get_account(series).unwrap().data;
    let s: metalbook_core::accounts::Series = bytemuck::pod_read_unaligned(&d);
    Address::new_from_array(s.rent_payer)
}

fn token_amount(svm: &LiteSVM, a: &Address) -> u64 {
    svm.get_account(a)
        .filter(|acc| acc.data.len() >= 72)
        .map_or(0, |acc| {
            u64::from_le_bytes(acc.data[64..72].try_into().unwrap())
        })
}

struct Case<'a> {
    env: &'a Env,
    svm: LiteSVM,
    actors: &'a [Keypair],
    keeper: &'a Keypair,
    series: u8,
    now: i64,
    /// Donations in the vault not yet swept.
    donated: u64,
    donations_total: u64,
    initial_usdc: u64,
    closed: bool,
}

impl Case<'_> {
    fn args(&self) -> SeriesArgs {
        series_args(self.series).0
    }

    fn series_addr(&self) -> Address {
        self.args().series()
    }

    fn vault(&self) -> Address {
        ata(&self.series_addr(), &USDC_MINT)
    }

    fn series_state(&self) -> Option<metalbook_core::accounts::Series> {
        self.svm
            .get_account(&self.series_addr())
            .filter(|a| a.data.len() == 504)
            .map(|a| bytemuck::pod_read_unaligned(&a.data))
    }

    fn payoff(&self) -> Payoff {
        let a = self.args();
        Payoff {
            kind: a.kind,
            strike: a.strike,
            cap: a.cap,
            mult: a.multiplier,
        }
    }

    fn rent_payer(&self) -> Address {
        rent_payer(&self.env.svm, &self.series_addr())
    }

    fn usdc_of(&self, owner: &Address) -> u64 {
        token_amount(&self.svm, &ata(owner, &USDC_MINT))
    }

    fn longs_of(&self, owner: &Address) -> u64 {
        token_amount(&self.svm, &ata(owner, &self.args().long_mint()))
    }

    fn position(&self, owner: &Address) -> Option<(u64, bool, u64)> {
        let a = self
            .svm
            .get_account(&position_address(&self.series_addr(), owner))
            .filter(|a| a.data.len() == 80)?;
        let written = u64::from_le_bytes(a.data[72..80].try_into().unwrap());
        Some((written, a.data[3] == 1, a.lamports))
    }

    /// Every USDC atom held by the actors, the rent payer and the vault.
    fn total_usdc(&self) -> u64 {
        let mut t: u64 = self.actors.iter().map(|a| self.usdc_of(&a.pubkey())).sum();
        t += self.usdc_of(&self.rent_payer());
        t += token_amount(&self.svm, &self.vault());
        t
    }

    /// Every account a transaction of this case can touch.
    fn watched(&self) -> Vec<Address> {
        let series = self.series_addr();
        let long_mint = self.args().long_mint();
        let mut v = vec![series, self.vault(), long_mint, self.rent_payer()];
        v.push(ata(&self.rent_payer(), &USDC_MINT));
        for a in self.actors {
            let k = a.pubkey();
            v.push(k);
            v.push(ata(&k, &USDC_MINT));
            v.push(ata(&k, &long_mint));
            v.push(position_address(&series, &k));
        }
        v.push(self.keeper.pubkey());
        v
    }

    fn snapshot(&self) -> Vec<(Address, Option<Account>)> {
        self.watched()
            .into_iter()
            .map(|a| (a, self.svm.get_account(&a)))
            .collect()
    }

    /// Sends `ix` signed by `payer`. On failure, checks the rollback rule.
    fn send(&mut self, ix: Instruction, payer: &Keypair) -> Result<(), String> {
        set_clock(&mut self.svm, self.now);
        let before = self.snapshot();
        self.svm.expire_blockhash();
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&payer.pubkey()),
            &[payer],
            self.svm.latest_blockhash(),
        );
        let result = self
            .svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{:?}", e.err));
        if result.is_err() {
            for (a, acc) in before {
                let now = self.svm.get_account(&a);
                if a == payer.pubkey() {
                    let (b, n) = (acc.unwrap(), now.unwrap());
                    assert_eq!(
                        n.lamports,
                        b.lamports - FEE,
                        "the fee payer paid more than the fee"
                    );
                    assert_eq!(n.data, b.data, "the fee payer's data changed");
                } else {
                    assert_eq!(now, acc, "a failed transaction changed {a}");
                }
            }
        }
        result
    }

    fn check_conservation(&self) {
        assert_eq!(
            self.total_usdc(),
            self.initial_usdc + self.donations_total,
            "USDC was created or destroyed"
        );
        if self.closed {
            return;
        }
        let s = self
            .series_state()
            .expect("the series exists until it is closed");
        assert_eq!(
            token_amount(&self.svm, &self.vault()),
            s.vault_owed + self.donated,
            "vault balance is not owed plus donations"
        );
        let mint = self.svm.get_account(&self.args().long_mint()).unwrap();
        let supply = u64::from_le_bytes(mint.data[36..44].try_into().unwrap());
        assert_eq!(
            supply, s.longs_outstanding,
            "long supply is not longs outstanding"
        );
        let written: u64 = self
            .actors
            .iter()
            .filter_map(|a| self.position(&a.pubkey()))
            .map(|(w, claimed, _)| if claimed { 0 } else { w })
            .sum();
        assert_eq!(
            written, s.writers_unclaimed,
            "positions do not add up to writers unclaimed"
        );
    }

    fn actor(&self, a: u8) -> &Keypair {
        &self.actors[usize::from(a) % ACTORS]
    }

    fn sample_ix(&mut self, slot: u8, price_off: i16) -> Instruction {
        let (_, feed, expo, base) = series_args(self.series);
        let i = i64::from(slot % 30);
        let t_i = EXPIRY - WINDOW_S + i * SLOT_S;
        // From a tenth of the strike to about nine times it; the confidence stays at 0.01%.
        let price = (base + i64::from(price_off) * (base / 4_000)).max(base / 10);
        let d = Decoded {
            partial: None,
            feed,
            price,
            conf: (price / 10_000).max(1) as u64,
            exponent: expo,
            publish: t_i + 1,
            prev: t_i - 1,
        };
        let at = Address::new_unique();
        let data = pyth::encode(&d);
        let lamports = self.svm.minimum_balance_for_rent_exemption(data.len());
        self.svm
            .set_account(
                at,
                Account {
                    lamports,
                    data,
                    owner: crate::common::PYTH_RECEIVER,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        self.env.ix_sample(&self.series_addr(), &at)
    }

    /// Applies one operation; `must` asserts it succeeds (the draining suffix).
    fn op(&mut self, op: Op, must: bool) {
        let args = self.args();
        let env = self.env;
        let result = match op {
            Op::Mint { a, n } => {
                let w = self.actor(a).insecure_clone();
                self.send(env.ix_mint(&w.pubkey(), args, u64::from(n % 4)), &w)
            }
            Op::Close { a, n } => {
                let w = self.actor(a).insecure_clone();
                self.send(env.ix_close(&w.pubkey(), args, u64::from(n % 4)), &w)
            }
            Op::Transfer { from, to, n } => {
                let f = self.actor(from).insecure_clone();
                let t = self.actor(to).pubkey();
                let mint = args.long_mint();
                let mut data = vec![3u8];
                data.extend_from_slice(&u64::from(n % 4).to_le_bytes());
                let ix = Instruction {
                    program_id: TOKEN_PROGRAM,
                    accounts: vec![
                        AccountMeta::new(ata(&f.pubkey(), &mint), false),
                        AccountMeta::new(ata(&t, &mint), false),
                        AccountMeta::new_readonly(f.pubkey(), true),
                    ],
                    data,
                };
                self.send(ix, &f)
            }
            Op::Donate { amount } => {
                if self.closed {
                    return;
                }
                let amount = u64::from(amount % 1_000_000);
                let admin = env.admin.insecure_clone();
                let vault = self.vault();
                MintTo::new(&mut self.svm, &admin, &USDC_MINT, &vault, amount)
                    .send()
                    .unwrap();
                self.donated += amount;
                self.donations_total += amount;
                Ok(())
            }
            Op::Sample { slot, price_off } => {
                let ix = self.sample_ix(slot, price_off);
                let k = self.keeper.insecure_clone();
                self.send(ix, &k)
            }
            Op::Warp { to, off } => {
                let hard = EXPIRY + HARD_S;
                let at = match to % 5 {
                    0 => EXPIRY - WINDOW_S,
                    1 => EXPIRY,
                    2 => EXPIRY + D,
                    3 => hard,
                    _ => hard + D,
                };
                self.now = self.now.max(at + i64::from(off));
                return;
            }
            Op::Settle => {
                let k = self.keeper.insecure_clone();
                self.send(env.ix_settle(&self.series_addr()), &k)
            }
            Op::Redeem { a, n } => {
                let h = self.actor(a).insecure_clone();
                let n = u64::from(n % 4);
                self.redeem(&h, n)
            }
            Op::Claim { a } => {
                let w = self.actor(a).insecure_clone();
                self.claim(&w)
            }
            Op::CloseSeries => self.close_series(),
            Op::ClosePosition { a } => {
                let w = self.actor(a).insecure_clone();
                let pos = position_address(&self.series_addr(), &w.pubkey());
                let before = self.position(&w.pubkey());
                let lamports_before = self.svm.get_balance(&w.pubkey()).unwrap_or(0);
                let k = self.keeper.insecure_clone();
                let r = self.send(env.ix_close_position(&pos, &w.pubkey()), &k);
                if r.is_ok() {
                    let (_, _, rent) = before.expect("closed a position that did not exist");
                    assert_eq!(
                        self.svm.get_balance(&w.pubkey()).unwrap(),
                        lamports_before + rent,
                        "close_position returned other than the position's rent"
                    );
                    assert!(
                        self.position(&w.pubkey()).is_none(),
                        "the position survived"
                    );
                }
                r
            }
        };
        if must {
            if let Err(e) = &result {
                panic!("draining step {op:?} failed: {e}");
            }
        }
        self.check_conservation();
    }

    fn redeem(&mut self, h: &Keypair, n: u64) -> Result<(), String> {
        let before = self.usdc_of(&h.pubkey());
        let s = self.series_state();
        let r = self.send(self.env.ix_redeem(&h.pubkey(), self.args(), n), h);
        if r.is_ok() {
            let s = s.unwrap();
            let payout = if s.state == 3 {
                0
            } else {
                self.payoff().payout(s.settlement_price)
            };
            assert_eq!(
                s.payout, payout,
                "stored payout differs from the payoff model"
            );
            assert_eq!(
                self.usdc_of(&h.pubkey()),
                before + n * payout,
                "redeem paid other than n × payout"
            );
        }
        r
    }

    fn claim(&mut self, w: &Keypair) -> Result<(), String> {
        let before = self.usdc_of(&w.pubkey());
        let pos = self.position(&w.pubkey());
        let s = self.series_state();
        let r = self.send(self.env.ix_claim(&w.pubkey(), self.args()), w);
        if r.is_ok() {
            let s = s.unwrap();
            let (written, ..) = pos.unwrap();
            let residual = if s.state == 3 {
                self.payoff().collateral()
            } else {
                self.payoff().residual(s.settlement_price)
            };
            assert_eq!(
                s.residual, residual,
                "stored residual differs from the payoff model"
            );
            assert_eq!(
                self.usdc_of(&w.pubkey()),
                before + written * residual,
                "claim paid other than written × residual"
            );
        }
        r
    }

    fn close_series(&mut self) -> Result<(), String> {
        let rp = self.rent_payer();
        let series = self.series_addr();
        let vault = self.vault();
        let usdc_before = self.usdc_of(&rp);
        let lamports_before = self.svm.get_balance(&rp).unwrap();
        let rent =
            self.svm.get_balance(&series).unwrap_or(0) + self.svm.get_balance(&vault).unwrap_or(0);
        let k = self.keeper.insecure_clone();
        let r = self.send(self.env.ix_close_series(self.args(), &rp), &k);
        if r.is_ok() {
            assert_eq!(
                self.usdc_of(&rp),
                usdc_before + self.donated,
                "donations not swept exactly"
            );
            assert_eq!(
                self.svm.get_balance(&rp).unwrap(),
                lamports_before + rent,
                "rent not returned exactly"
            );
            for a in [series, vault] {
                assert!(
                    self.svm.get_account(&a).is_none_or(|x| x.lamports == 0),
                    "{a} survived close_series"
                );
            }
            self.closed = true;
            self.donated = 0;
        }
        r
    }

    fn run(&mut self, input: &Input) {
        self.initial_usdc = self.total_usdc();
        for op in input.ops.iter().take(24) {
            self.op(*op, false);
        }
        self.drain(input);
    }

    /// Settles (or voids), pays everyone, closes everything; every step must succeed.
    fn drain(&mut self, input: &Input) {
        if !self.closed {
            let state = self.series_state().unwrap().state;
            if state < 2 {
                let s = self.series_state().unwrap();
                if input.end_void {
                    // From the hard deadline plus D after every change: settles on whatever
                    // was sampled, or voids.
                    let last_change = s
                        .window_changed
                        .max(s.next_print_changed)
                        .max(s.fallback_changed);
                    self.now = self.now.max(EXPIRY + HARD_S).max(last_change) + D + 1;
                } else {
                    self.now = self.now.max(EXPIRY + 10);
                    for slot in 0..30u8 {
                        if s.filled_slots & (1 << slot) == 0 {
                            self.op(
                                Op::Sample {
                                    slot,
                                    price_off: input.end_price_off,
                                },
                                true,
                            );
                        }
                    }
                }
                self.op(Op::Settle, true);
            }
            for i in 0..ACTORS {
                let h = self.actors[i].insecure_clone();
                let n = self.longs_of(&h.pubkey());
                if n > 0 {
                    let r = self.redeem(&h, n);
                    assert!(r.is_ok(), "draining redeem failed: {r:?}");
                    self.check_conservation();
                }
            }
            for i in 0..ACTORS {
                let w = self.actors[i].pubkey();
                if let Some((written, claimed, _)) = self.position(&w) {
                    if written > 0 && !claimed {
                        self.op(Op::Claim { a: i as u8 }, true);
                    }
                }
            }
            self.op(Op::CloseSeries, true);
        }
        for i in 0..ACTORS {
            if self.position(&self.actors[i].pubkey()).is_some() {
                self.op(Op::ClosePosition { a: i as u8 }, true);
            }
        }
        // Everything paid: no USDC lost or created, no long left, nothing open.
        assert_eq!(self.total_usdc(), self.initial_usdc + self.donations_total);
        let mint = self.svm.get_account(&self.args().long_mint()).unwrap();
        assert_eq!(
            u64::from_le_bytes(mint.data[36..44].try_into().unwrap()),
            0,
            "longs left"
        );
        assert!(self.series_state().is_none(), "the series is still open");
    }
}
