//! The `metalbook-admin` binary refuses `--dry-run` with `--execute` on `list-week` and
//! `reclaim` before reading or sending anything: no RPC is reachable at the given URL (a closed
//! port), so any read would fail with a different error. `metalbook-watch --once` exits with an
//! error on the same unreachable RPC instead of looping. `metalbook-keeper` keeps running and
//! retries each pass on it, and exits with an error only under `--once`.

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

/// Runs `metalbook-keeper` against a closed RPC port with a throwaway payer, its own lock
/// directory and a dummy Hermes key (never used: no series is read), for `wait`, then stops it
/// (or lets it exit) and returns whether it exited, its success, and its stderr.
fn keeper_on_dead_rpc(extra: &[&str], wait: std::time::Duration) -> (bool, bool, String) {
    let dir = std::env::temp_dir().join(format!(
        "metalbook-keeper-flags-{}-{}",
        std::process::id(),
        extra.len()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let keypair = dir.join("payer.json");
    let kp = solana_keypair::Keypair::new_from_array([7; 32]);
    std::fs::write(&keypair, format!("{:?}", kp.to_bytes().to_vec())).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_metalbook-keeper"))
        .args([
            "--url",
            "http://127.0.0.1:9",
            "--program-id",
            "FPPuou5kgSp6tbwQRwKPBNhrMyf1Xb8n2ib3Yqpbsuwg",
            "--keypair",
            keypair.to_str().unwrap(),
            "--interval-s",
            "1",
        ])
        .args(extra)
        .env("METALBOOK_HERMES_API_KEY", "not-a-key")
        .env("PYTH_POST_LOCK_DIR", &dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now().checked_add(wait).unwrap();
    let exited = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status);
        }
        if std::time::Instant::now() > deadline {
            break None;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    };
    if exited.is_none() {
        child.kill().unwrap();
    }
    let out = child.wait_with_output().unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    (
        exited.is_some(),
        exited.is_some_and(|s| s.success()),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn test_keeper_survives_an_unreadable_chain_and_retries() {
    // Without `--once`, an RPC error reading the series list or the clock skips the pass: the
    // process keeps running and tries again at the next pass, instead of exiting and leaving
    // every window unattended.
    let (exited, _, err) = keeper_on_dead_rpc(&[], std::time::Duration::from_secs(20));
    assert!(!exited, "the keeper exited on an unreadable chain: {err}");
    assert!(
        err.contains("pass 1: skipped, the chain could not be read"),
        "{err}"
    );
    assert!(
        err.contains("pass 2: skipped, the chain could not be read"),
        "{err}"
    );
}

#[test]
fn test_keeper_once_fails_on_an_unreadable_chain() {
    let (exited, ok, err) = keeper_on_dead_rpc(&["--once"], std::time::Duration::from_secs(60));
    assert!(exited, "metalbook-keeper --once did not exit: {err}");
    assert!(!ok, "{err}");
    assert!(
        err.contains("pass 1: skipped, the chain could not be read"),
        "{err}"
    );
}
