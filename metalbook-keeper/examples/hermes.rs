//! Fetches the latest XAU/USD print from Hermes once.
//!
//! ```sh
//! METALBOOK_HERMES_API_KEY=... cargo run -p metalbook-keeper --example hermes -- --live
//! ```
//!
//! Without `--live` it only prints this usage. Without a key it exits with status 2 and a clear
//! message; it never reads a key from a file. Errors are printed with their `Display`, which
//! never contains response content or the key (tested in `tests/hermes.rs`). The authentication scheme is unverified: set
//! `METALBOOK_HERMES_AUTH=header:<name>` if Hermes expects another header.

#![allow(clippy::disallowed_macros, missing_docs)]

use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use metalbook_keeper::{HermesConfig, HermesSource, UreqHttp};

const XAU: [u8; 32] = [
    0x76, 0x5d, 0x2b, 0xa9, 0x06, 0xdb, 0xc3, 0x2c, 0xa1, 0x7c, 0xc1, 0x1f, 0x53, 0x10, 0xa8, 0x9e,
    0x9e, 0xe1, 0xf6, 0x42, 0x05, 0x08, 0xc6, 0x38, 0x61, 0xf2, 0xf8, 0xba, 0x4e, 0xe3, 0x4b, 0xb2,
];

fn main() -> ExitCode {
    if !std::env::args().any(|a| a == "--live") {
        println!("usage: METALBOOK_HERMES_API_KEY=... hermes --live");
        return ExitCode::SUCCESS;
    }
    let config = match HermesConfig::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "hermes: {e}. Set METALBOOK_HERMES_API_KEY in the environment (never in a file)."
            );
            return ExitCode::from(2);
        }
    };
    let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) else {
        eprintln!("hermes: system clock before 1970");
        return ExitCode::from(1);
    };
    let since = i64::try_from(now.as_secs())
        .unwrap_or(i64::MAX)
        .saturating_sub(60);
    match HermesSource::new(config, UreqHttp::new()).first_at_or_after(&XAU, since) {
        Ok(Some(u)) => {
            println!(
                "XAU/USD {:?} ({} update bytes)",
                u.print,
                u.update_data.len()
            );
            ExitCode::SUCCESS
        }
        Ok(None) => {
            println!("no XAU/USD update at or after {since} yet (market closed?)");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("hermes: {e}");
            ExitCode::from(1)
        }
    }
}
