//! cargo-fuzz entry for `ledger_sequence` (fuzz/harness/ledger_sequence.rs). Loads
//! target/deploy/metalbook_program.so: `make program` first.

#![no_main]

#[path = "../../metalbook-tests/tests/common/mod.rs"]
mod common;
#[path = "../harness/ledger_sequence.rs"]
mod ledger_sequence;
#[path = "../harness/model.rs"]
mod model;
#[path = "../harness/pyth.rs"]
mod pyth;
#[path = "../harness/sample_settlement.rs"]
mod sample_settlement;

use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: ledger_sequence::Input| ledger_sequence::run(&input));
