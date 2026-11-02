//! cargo-fuzz entry for `settlement_model` (fuzz/harness/settlement_model.rs).

#![no_main]

#[path = "../harness/model.rs"]
mod model;
#[path = "../harness/settlement_model.rs"]
mod settlement_model;

use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: settlement_model::Input| settlement_model::run(&input));
