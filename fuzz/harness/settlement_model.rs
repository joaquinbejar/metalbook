//! Target `settlement_model`: `metalbook_core::SettlementState` (`add_print`, `decide`,
//! `restore`) against the reference model of `model.rs`, on arbitrary oracle terms and
//! arbitrary sequences of print offers and clock steps.
//!
//! Invariants, after every step:
//! - `add_print` stores exactly what the model stores (slot, link, fallback), or rejects
//!   exactly when the model does, and the stored parts equal the model's field by field;
//! - candidates only improve: filled slots keep their prices, the chain only grows, the
//!   fallback only moves later, and each kind's change time moves only when that kind
//!   changed (so each `D` restarts on its own kind only);
//! - `restore(parts)` gives back the same state;
//! - `decide` equals the model's cascade; a settlement on a lower step never happens while a
//!   higher step exists; the price lies within the bounds of the prints it averages.

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

use arbitrary::Arbitrary;
use metalbook_core::{
    CoreError, Decision, OracleParams, Print, SettleMethod, SettlementParts, SettlementState,
    UnixTimestamp, MAX_NEXT_PRINTS,
};

use crate::model::{atoms_bounds, Candidates, Outcome, Px, Terms};

const EXPIRY: i64 = 1_800_000_000;

/// Oracle terms, mapped into the valid range of `OracleParams::try_new`.
#[derive(Debug, Clone, Copy, Arbitrary)]
pub struct TermsIn {
    pub slot_s: u8,
    pub slots: u8,
    pub tolerance: u8,
    pub min_samples: u8,
    pub k: u8,
    pub d: u16,
    pub lookback: u16,
    pub hard_extra: u16,
    pub expo: u8,
}

impl TermsIn {
    fn terms(&self) -> Terms {
        let slot_s = 2 + i64::from(self.slot_s % 120);
        let slots = 1 + u32::from(self.slots % 32);
        let window_s = slot_s * i64::from(slots);
        Terms {
            window_s,
            slot_s,
            tolerance_s: i64::from(self.tolerance) % slot_s,
            min_samples: 1 + u32::from(self.min_samples) % slots,
            k: 1 + usize::from(self.k) % MAX_NEXT_PRINTS,
            d: 1 + i64::from(self.d),
            lookback_s: 1 + i64::from(self.lookback) * 8,
            hard_s: window_s + 1 + i64::from(self.hard_extra) * 16,
            expo: -i32::from(self.expo % 13),
        }
    }
}

/// One step.
#[derive(Debug, Clone, Copy, Arbitrary)]
pub enum Op {
    /// A print at an arbitrary time near expiry.
    Free {
        price: u64,
        publish_off: i32,
        gap: u16,
        dt: u16,
    },
    /// A print aimed at window slot `slot`: `late` seconds after its boundary, previous
    /// aggregate `gap` seconds before the print.
    Slot {
        slot: u8,
        late: u8,
        gap: u16,
        price: u32,
        dt: u16,
    },
    /// A print aimed at the next link of the model's chain (or the first one).
    Link {
        step: u16,
        repeat_prev: bool,
        price: u32,
        dt: u16,
    },
    /// A print aimed at the fallback range, `back` seconds before expiry.
    Fallback {
        back: u32,
        same_second: bool,
        price: u32,
        dt: u16,
    },
    /// Advance the clock.
    Wait { dt: u32 },
    /// Jump to a cascade boundary plus `off` seconds: window end, `+ D`, hard deadline,
    /// hard deadline plus `D`.
    Jump { to: u8, off: i8 },
}

#[derive(Debug, Arbitrary)]
pub struct Input {
    pub terms: TermsIn,
    pub ops: Vec<Op>,
}

fn params(t: &Terms) -> OracleParams {
    OracleParams::try_new(
        t.window_s as u32,
        t.slot_s as u32,
        t.tolerance_s as u16,
        t.min_samples as u16,
        t.k as u8,
        t.d as u32,
        t.lookback_s as u32,
        t.hard_s as u32,
        t.expo as i8,
    )
    .expect("terms are built inside the valid range")
}

fn model_parts(m: &Candidates) -> SettlementParts {
    let mut p = SettlementParts {
        sample_sum: m.sample_sum(),
        filled_slots: m.bitmap(),
        next_len: m.chain.len() as u8,
        window_changed: m.window_changed,
        next_print_changed: m.next_changed,
        fallback_changed: m.fallback_changed,
        ..SettlementParts::default()
    };
    for (i, &(t, px)) in m.chain.iter().enumerate() {
        p.next_times[i] = t;
        p.next_prices[i] = px;
    }
    if let Some((t, px)) = m.fallback {
        p.fallback_time = t;
        p.fallback_price = px;
    }
    p
}

fn method_code(m: SettleMethod) -> u8 {
    m as u8
}

/// Runs one input.
pub fn run(input: &Input) {
    let t = input.terms.terms();
    let params = params(&t);
    let expiry = UnixTimestamp::new(EXPIRY);
    let mut state = SettlementState::default();
    let mut model = Candidates::default();
    let mut now = EXPIRY - t.window_s - 60;

    for op in input.ops.iter().take(64) {
        let offered = match *op {
            Op::Free {
                price,
                publish_off,
                gap,
                dt,
            } => {
                now += i64::from(dt);
                let publish = EXPIRY + i64::from(publish_off) % (t.lookback_s + t.hard_s + 3_600);
                Some(Px {
                    price,
                    publish,
                    prev: publish - i64::from(gap),
                })
            }
            Op::Slot {
                slot,
                late,
                gap,
                price,
                dt,
            } => {
                now += i64::from(dt);
                let i = i64::from(u32::from(slot) % t.slots());
                let t_i = EXPIRY - t.window_s + i * t.slot_s;
                let publish = t_i + i64::from(late) % (t.tolerance_s + 2);
                Some(Px {
                    price: u64::from(price),
                    publish,
                    prev: publish - i64::from(gap),
                })
            }
            Op::Link {
                step,
                repeat_prev,
                price,
                dt,
            } => {
                now += i64::from(dt);
                let last = model.chain.last().map_or(EXPIRY, |&(t, _)| t);
                let publish = last + i64::from(step % 300);
                let prev = if repeat_prev { publish } else { last };
                Some(Px {
                    price: u64::from(price),
                    publish,
                    prev,
                })
            }
            Op::Fallback {
                back,
                same_second,
                price,
                dt,
            } => {
                now += i64::from(dt);
                let publish = EXPIRY - i64::from(back) % (t.lookback_s + 120);
                let prev = if same_second { publish } else { publish - 1 };
                Some(Px {
                    price: u64::from(price),
                    publish,
                    prev,
                })
            }
            Op::Wait { dt } => {
                now += i64::from(dt % 2_000_000);
                None
            }
            Op::Jump { to, off } => {
                let hard = EXPIRY + t.hard_s;
                let base = match to % 4 {
                    0 => EXPIRY,
                    1 => EXPIRY + t.d,
                    2 => hard,
                    _ => hard + t.d,
                };
                // The clock never goes back.
                now = now.max(base + i64::from(off));
                None
            }
        };

        if let Some(x) = offered {
            let core_print = Print::try_new(x.price, x.publish, x.prev);
            assert_eq!(
                core_print.is_ok(),
                x.valid(),
                "Print::try_new validity {x:?}"
            );
            if let Ok(print) = core_print {
                let before_model = model.clone();
                let before_parts = state.parts();
                let got = state.add_print(&params, expiry, print, now);
                let want = model.offer(&t, EXPIRY, x, now);
                match (got, want) {
                    (Ok(u), Some(w)) => {
                        assert_eq!(u.slot, w.slot, "slot of {x:?} at {now}");
                        assert_eq!(u.next_print, w.link, "link of {x:?} at {now}");
                        assert_eq!(u.fallback, w.fallback, "fallback of {x:?} at {now}");
                    }
                    (Err(CoreError::PrintNotUsable), None) => {
                        assert_eq!(
                            state.parts(),
                            before_parts,
                            "a rejected print changed the state"
                        );
                    }
                    (got, want) => {
                        panic!("add_print {got:?}, model {want:?}, print {x:?} at {now}")
                    }
                }
                let after = state.parts();
                assert_eq!(
                    after,
                    model_parts(&model),
                    "stored parts differ from the model"
                );
                monotone(&before_model, &model);
                let restored = SettlementState::restore(&params, after).expect("reachable state");
                assert_eq!(restored, state, "restore(parts) differs");
            }
        }

        check_decide(&state, &model, &params, &t, expiry, now);
    }
}

/// Candidates only improve, and each change time moves only with its own kind.
fn monotone(before: &Candidates, after: &Candidates) {
    for (b, a) in before.slots.iter().zip(after.slots.iter()) {
        if let Some(p) = b {
            assert_eq!(a, &Some(*p), "a filled slot changed");
        }
    }
    assert!(
        after.chain.starts_with(&before.chain),
        "the chain was not extended"
    );
    if let Some((bt, _)) = before.fallback {
        assert!(
            after.fallback.is_some_and(|(at, _)| at >= bt),
            "the fallback moved back"
        );
    }
    if after.slots == before.slots {
        assert_eq!(
            after.window_changed, before.window_changed,
            "window D restarted"
        );
    }
    if after.chain == before.chain {
        assert_eq!(after.next_changed, before.next_changed, "chain D restarted");
    }
    if after.fallback == before.fallback {
        assert_eq!(
            after.fallback_changed, before.fallback_changed,
            "fallback D restarted"
        );
    }
}

fn check_decide(
    state: &SettlementState,
    model: &Candidates,
    params: &OracleParams,
    t: &Terms,
    expiry: UnixTimestamp,
    now: i64,
) {
    let got = state.decide(params, expiry, now);
    let want = model.decide(t, EXPIRY, now);
    match (got, want) {
        (Ok(Decision::Wait { until }), Outcome::Wait(w)) => assert_eq!(until, w, "wait until"),
        (Ok(Decision::Void), Outcome::Void) => {
            assert!(model.filled() == 0 && model.chain.is_empty() && model.fallback.is_none());
        }
        (
            Ok(Decision::Settle {
                method,
                price,
                samples,
            }),
            Outcome::Settle(m, p, n),
        ) => {
            assert_eq!(
                (method_code(method), price.get(), samples),
                (m, p, n),
                "settlement"
            );
            // Never a lower step while a higher one exists.
            let filled = model.filled();
            if m >= 1 {
                assert!(
                    filled < t.min_samples,
                    "settled below Window with enough slots"
                );
            }
            if m >= 2 {
                assert!(
                    model.chain.len() < t.k,
                    "settled below NextPrint with a full chain"
                );
            }
            if m >= 3 {
                assert!(model.chain.is_empty(), "settled below a partial chain");
            }
            if m >= 4 {
                assert_eq!(filled, 0, "settled on the fallback with window slots");
            }
            // Within the bounds of what it averages.
            let prices: Vec<u64> = match m {
                0 | 3 => model.slots.iter().flatten().copied().collect(),
                1 | 2 => model.chain.iter().map(|&(_, p)| p).collect(),
                _ => model.fallback.iter().map(|&(_, p)| p).collect(),
            };
            let lo = prices
                .iter()
                .map(|&p| atoms_bounds(p, t.expo).0)
                .min()
                .unwrap();
            let hi = prices
                .iter()
                .map(|&p| atoms_bounds(p, t.expo).1)
                .max()
                .unwrap();
            assert!(
                (lo..=hi).contains(&u128::from(p)),
                "price {p} outside [{lo}, {hi}]"
            );
        }
        (Err(CoreError::Overflow), Outcome::Overflow) => {}
        (got, want) => panic!("decide {got:?}, model {want:?} at {now}"),
    }
}
