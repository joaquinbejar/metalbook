//! cargo-fuzz entry for `sample_settlement` (fuzz/harness/sample_settlement.rs). Loads
//! target/deploy/metalbook_program.so: `make program` first.

#![no_main]

#[path = "../../metalbook-tests/tests/common/mod.rs"]
mod common;
#[path = "../harness/model.rs"]
mod model;
#[path = "../harness/pyth.rs"]
mod pyth;
#[path = "../harness/sample_settlement.rs"]
mod sample_settlement;

use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: sample_settlement::Input| {
    sample_settlement::run(&input);
});
