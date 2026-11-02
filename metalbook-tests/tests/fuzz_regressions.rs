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

use std::path::{Path, PathBuf};

use arbitrary::{Arbitrary, Unstructured};

/// The committed regressions directory.
fn committed() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../fuzz/regressions")
}

/// Decodes like libFuzzer's typed entry point (`arbitrary_take_rest`).
fn decode<T: for<'a> Arbitrary<'a>>(bytes: &[u8]) -> arbitrary::Result<T> {
    T::arbitrary_take_rest(Unstructured::new(bytes))
}

/// The decoded inputs under `root/<target>/`, sorted by name.
///
/// `arbitrary` decodes any byte string (even an empty one) into some input, so a change to the
/// input types would silently turn a committed crash into a different, harmless input. Each
/// input file `<name>` therefore has a committed `<name>.debug`, the `{:#?}` rendering of what
/// it decodes to, and a file whose decoding differs, has no rendering, or does not decode fails
/// here with its name. `METALBOOK_FUZZ_BLESS=1` rewrites the renderings after a deliberate
/// change to the input types (review the diff: it shows what each regression now means).
fn regressions<T: for<'a> Arbitrary<'a> + std::fmt::Debug>(
    root: &Path,
    target: &str,
) -> Vec<(String, T)> {
    let dir = root.join(target);
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_file() && p.extension().is_none_or(|x| x != "debug"))
        .collect();
    files.sort();
    let bless = std::env::var_os("METALBOOK_FUZZ_BLESS").is_some();
    let v: Vec<(String, T)> = files
        .iter()
        .map(|path| {
            let name = path.display().to_string();
            let input = decode::<T>(&std::fs::read(path).unwrap())
                .unwrap_or_else(|e| panic!("{name}: does not decode as a {target} input ({e})"));
            let rendering = format!("{input:#?}\n");
            let debug = path.with_extension("debug");
            if bless {
                std::fs::write(&debug, &rendering).unwrap();
            }
            let want = std::fs::read_to_string(&debug).unwrap_or_else(|e| {
                panic!("{name}: no committed rendering {} ({e})", debug.display())
            });
            assert!(
                want == rendering,
                "{name}: decodes to another input than its committed rendering; the input types \
                 changed, so re-minimize the crash or bless after review\n--- committed\n{want}\
                 --- now\n{rendering}"
            );
            (name, input)
        })
        .collect();
    assert!(!v.is_empty(), "no committed input for {target}");
    v
}

#[test]
fn settlement_model_regressions() {
    let v = regressions::<settlement_model::Input>(&committed(), "settlement_model");
    for (name, i) in &v {
        eprintln!("{name}");
        settlement_model::run(i);
    }
    eprintln!("settlement_model: {} regressions replayed", v.len());
}

#[test]
fn sample_settlement_regressions() {
    let v = regressions::<sample_settlement::Input>(&committed(), "sample_settlement");
    for (name, i) in &v {
        eprintln!("{name}");
        sample_settlement::run(i);
    }
    eprintln!("sample_settlement: {} regressions replayed", v.len());
}

#[test]
fn ledger_sequence_regressions() {
    let v = regressions::<ledger_sequence::Input>(&committed(), "ledger_sequence");
    for (name, i) in &v {
        eprintln!("{name}");
        ledger_sequence::run(i);
    }
    eprintln!("ledger_sequence: {} regressions replayed", v.len());
}

/// Control for `regressions`: a committed input replaced by other bytes (here empty, which
/// still decodes), an input without its rendering, and an empty directory all fail with the
/// file name instead of replaying something else or nothing.
#[test]
fn regressions_refuse_a_changed_or_unrendered_input() {
    let root = std::env::temp_dir().join(format!("metalbook-fuzz-control-{}", std::process::id()));
    let message = |r: std::thread::Result<Vec<(String, settlement_model::Input)>>| -> String {
        let e = r.expect_err("the control input was accepted");
        e.downcast_ref::<String>()
            .cloned()
            .or_else(|| e.downcast_ref::<&str>().map(|s| (*s).to_owned()))
            .unwrap_or_default()
    };
    let run = || {
        std::panic::catch_unwind(|| {
            regressions::<settlement_model::Input>(&root, "settlement_model")
        })
    };
    let dir = root.join("settlement_model");
    std::fs::create_dir_all(&dir).unwrap();
    let empty_dir = message(run());
    // A real regression with its rendering replays; the same name with other bytes does not.
    let real = committed().join("settlement_model/corpus-07f421b3e86a");
    std::fs::copy(&real, dir.join("crash")).unwrap();
    std::fs::copy(real.with_extension("debug"), dir.join("crash.debug")).unwrap();
    assert_eq!(run().map(|v| v.len()).ok(), Some(1));
    std::fs::write(dir.join("crash"), []).unwrap();
    let changed = message(run());
    std::fs::remove_file(dir.join("crash.debug")).unwrap();
    let unrendered = message(run());
    std::fs::remove_dir_all(&root).unwrap();
    assert!(empty_dir.contains("no committed input"), "{empty_dir}");
    assert!(
        changed.contains("crash: decodes to another input"),
        "{changed}"
    );
    assert!(
        unrendered.contains("crash: no committed rendering"),
        "{unrendered}"
    );
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
