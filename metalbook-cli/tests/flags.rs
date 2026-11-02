//! The `metalbook-admin` binary refuses `--dry-run` with `--execute` on `list-week` and
//! `reclaim` before reading or sending anything: no RPC is reachable at the given URL (a closed
//! port), so any read would fail with a different error. `metalbook-watch --once` exits with an
//! error on the same unreachable RPC instead of looping.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::disallowed_macros,
    missing_docs
)]

use std::process::Command;

fn admin(args: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_metalbook-admin"))
        .args([
            "--url",
            "http://127.0.0.1:9",
            "--program-id",
            "FPPuou5kgSp6tbwQRwKPBNhrMyf1Xb8n2ib3Yqpbsuwg",
            "--keypair",
            "/nonexistent/keypair.json",
        ])
        .args(args)
        .env_remove("METALBOOK_HERMES_API_KEY")
        .output()
        .unwrap();
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn test_dry_run_and_execute_conflict_before_any_read_or_send() {
    for cmd in [
        vec!["--dry-run", "list-week", "--underlying", "xag", "--execute"],
        vec!["--dry-run", "reclaim", "--execute"],
    ] {
        let (ok, err) = admin(&cmd);
        assert!(!ok, "{cmd:?}");
        assert!(
            err.contains("--dry-run and --execute conflict"),
            "{cmd:?}: {err}"
        );
    }
}

#[test]
fn test_watch_once_fails_fast_on_an_unreachable_rpc() {
    // `--once` must report an unreadable chain and exit, never loop: the RPC is a closed port.
    let mut child = Command::new(env!("CARGO_BIN_EXE_metalbook-watch"))
        .args([
            "--url",
            "http://127.0.0.1:9",
            "--program-id",
            "FPPuou5kgSp6tbwQRwKPBNhrMyf1Xb8n2ib3Yqpbsuwg",
            "--interval-s",
            "1",
            "--once",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if std::time::Instant::now() > deadline {
            child.kill().unwrap();
            panic!("metalbook-watch --once did not exit on an unreachable RPC");
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    };
    assert!(!status.success());
}
