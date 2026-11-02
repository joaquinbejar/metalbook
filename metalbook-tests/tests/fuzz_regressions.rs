//! The fuzz harnesses of `fuzz/harness/` on stable (`docs/PLAN-improvements.md`, H5): every
//! committed input under `fuzz/regressions/<target>/` is replayed through the same code the
//! cargo-fuzz targets run, decoded the way libFuzzer decodes it (`arbitrary_take_rest`), plus
//! hand-built inputs that make sure each harness exercises its accepting paths. A crash input
//! found by fuzzing is added to the regression directory with the fix. Run `make program`
//! first.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::integer_division,
    clippy::panic_in_result_fn,
    clippy::disallowed_macros,
    missing_docs
)]

mod common;
#[path = "../../fuzz/harness/ledger_sequence.rs"]
mod ledger_sequence;
#[path = "../../fuzz/harness/model.rs"]
mod model;
#[path = "../../fuzz/harness/pyth.rs"]
mod pyth;
#[path = "../../fuzz/harness/sample_settlement.rs"]
mod sample_settlement;
#[path = "../../fuzz/harness/settlement_model.rs"]
mod settlement_model;

use arbitrary::{Arbitrary, Unstructured};

/// The committed inputs of `target`, sorted by name.
fn inputs(target: &str) -> Vec<(String, Vec<u8>)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fuzz/regressions")
        .join(target);
    let mut v: Vec<(String, Vec<u8>)> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_file())
        .map(|p| (p.display().to_string(), std::fs::read(&p).unwrap()))
        .collect();
    v.sort();
    assert!(!v.is_empty(), "no committed input for {target}");
    v
}

/// Decodes like libFuzzer's typed entry point; `None` where the fuzzer skips the input.
fn decode<T: for<'a> Arbitrary<'a>>(bytes: &[u8]) -> Option<T> {
    T::arbitrary_take_rest(Unstructured::new(bytes)).ok()
}

#[test]
fn settlement_model_regressions() {
    for (name, bytes) in inputs("settlement_model") {
        if let Some(i) = decode::<settlement_model::Input>(&bytes) {
            eprintln!("{name}");
            settlement_model::run(&i);
        }
    }
}

#[test]
fn sample_settlement_regressions() {
    for (name, bytes) in inputs("sample_settlement") {
        if let Some(i) = decode::<sample_settlement::Input>(&bytes) {
            eprintln!("{name}");
            sample_settlement::run(&i);
        }
    }
}

#[test]
fn ledger_sequence_regressions() {
    for (name, bytes) in inputs("ledger_sequence") {
        if let Some(i) = decode::<ledger_sequence::Input>(&bytes) {
            eprintln!("{name}");
            ledger_sequence::run(&i);
        }
    }
}

#[test]
fn settlement_model_hand_built_cascade() {
    use settlement_model::{Input, Op};
    // Every step of the cascade on default-like terms: a few slots, a chain, a fallback, then
    // the clock through D, the hard deadline and its D.
    let terms = settlement_model::TermsIn {
        slot_s: 58,
        slots: 29,
        tolerance: 10,
        min_samples: 19,
        k: 4,
        d: 599,
        lookback: 10_800,
        hard_extra: 37_800,
        expo: 3,
    };
    let mut ops = vec![];
    for slot in 0..25u8 {
        ops.push(Op::Slot {
            slot,
            late: 1,
            gap: 2,
            price: 4_000_000 + u32::from(slot),
            dt: 1,
        });
    }
    for _ in 0..6 {
        ops.push(Op::Link {
            step: 7,
            repeat_prev: false,
            price: 4_000_100,
            dt: 1,
        });
    }
    ops.push(Op::Fallback {
        back: 5,
        same_second: false,
        price: 3_999_000,
        dt: 1,
    });
    for to in 0..4 {
        ops.push(Op::Jump { to, off: 1 });
    }
    settlement_model::run(&Input { terms, ops });
}

#[test]
fn sample_settlement_accepts_a_full_window_and_settles() {
    use sample_settlement::{Aim, Data, Input, Owner, Step};
    for xag in [false, true] {
        let mut steps = vec![Step::Jump { to: 1, off: 20 }];
        for slot in 0..30u8 {
            steps.push(Step::Sample {
                data: Data::Valid {
                    aim: Aim::Slot {
                        slot,
                        late: 1,
                        gap: 3,
                    },
                    price_off: i16::from(slot) - 15,
                },
                owner: Owner::Receiver,
                dt: 0,
            });
        }
        // A forged owner and a second settle are rejected with their codes.
        steps.push(Step::Sample {
            data: Data::Valid {
                aim: Aim::Fallback {
                    back: 30,
                    same_second: false,
                },
                price_off: 0,
            },
            owner: Owner::System,
            dt: 0,
        });
        steps.push(Step::Settle { dt: 1 });
        steps.push(Step::Settle { dt: 1 });
        let stats = sample_settlement::run(&Input { xag, steps });
        // 30 slots and one settle accepted; the forged owner and the second settle rejected.
        assert_eq!((stats.accepted, stats.rejected), (31, 2), "{stats:?}");
    }
}

#[test]
fn ledger_sequence_hand_built_lifecycles() {
    use ledger_sequence::{Input, Op};
    for series in 0..3u8 {
        for end_void in [false, true] {
            let ops = vec![
                Op::Mint { a: 0, n: 3 },
                Op::Mint { a: 1, n: 2 },
                Op::Transfer {
                    from: 0,
                    to: 3,
                    n: 2,
                },
                Op::Close { a: 1, n: 1 },
                Op::Donate { amount: 1_234 },
                Op::Redeem { a: 3, n: 1 },
                Op::Warp { to: 0, off: 0 },
                Op::Sample {
                    slot: 0,
                    price_off: 200,
                },
                Op::Settle,
                Op::Claim { a: 0 },
                Op::CloseSeries,
            ];
            ledger_sequence::run(&Input {
                series,
                ops,
                end_void,
                end_price_off: -300,
            });
        }
    }
}
