//! The shared Hermes limiter (`docs/PLAN-phase2.md`, R1 step 3a) on a scaled clock: a short
//! window in a temporary directory shared by every limiter of a test, each limiter standing for
//! one process. One test uses a real child process, killed while it holds a slot.

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

use std::cell::Cell;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use metalbook_keeper::{
    state_files, Admission, HermesLimiter, HttpGet, HttpResponse, KeeperError, LimitedHttp,
    FILE_PREFIX,
};

const KEY: &str = "test-key";
/// The scaled window and block, in milliseconds.
const W: u64 = 400;

fn temp_dir(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "metalbook-limiter-{name}-{}-{nanos}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn limiter(dir: &Path, max: u32) -> HermesLimiter {
    HermesLimiter::with_window(dir, KEY, max, W, W).unwrap()
}

/// The largest number of `times` inside any window of `w` (half open).
fn max_in_window(times: &[Instant], w: Duration) -> usize {
    let mut t = times.to_vec();
    t.sort();
    let mut best = 0;
    for (i, start) in t.iter().enumerate() {
        let n = t[i..]
            .iter()
            .take_while(|x| x.duration_since(*start) < w)
            .count();
        best = best.max(n);
    }
    best
}

#[test]
fn test_a_backfilling_keeper_and_a_roll_never_exceed_the_limit() {
    // A keeper sending as fast as the limiter lets it, outside any quiet period, and a roller
    // trying every quarter window: no 8 requests inside one window, and the roller is told the
    // budget is busy rather than waiting.
    let dir = temp_dir("load");
    let sends = Arc::new(Mutex::new(Vec::new()));
    let stop = Arc::new(AtomicBool::new(false));
    let keeper = {
        let (dir, sends, stop) = (dir.clone(), sends.clone(), stop.clone());
        std::thread::spawn(move || {
            let l = limiter(&dir, 7);
            while !stop.load(Ordering::Relaxed) {
                let slot = l.acquire().unwrap();
                sends.lock().unwrap().push(Instant::now());
                std::thread::sleep(Duration::from_millis(5));
                l.confirm(slot).unwrap();
            }
        })
    };
    let roller = limiter(&dir, 7);
    let (mut busy, mut admitted) = (0, 0);
    let end = Instant::now() + Duration::from_millis(5 * W);
    while Instant::now() < end {
        match roller.try_acquire().unwrap() {
            Some(slot) => {
                sends.lock().unwrap().push(Instant::now());
                admitted += 1;
                roller.confirm(slot).unwrap();
            }
            None => busy += 1,
        }
        std::thread::sleep(Duration::from_millis(W / 4));
    }
    stop.store(true, Ordering::Relaxed);
    keeper.join().unwrap();
    let sends = sends.lock().unwrap().clone();
    assert!(
        sends.len() >= 7 * 4,
        "the keeper got its budget: {}",
        sends.len()
    );
    assert!(max_in_window(&sends, Duration::from_millis(W)) <= 7);
    assert!(
        busy > 0,
        "the roller saw the budget busy ({admitted} admitted)"
    );
}

#[test]
fn test_a_dropped_slot_is_reaped_and_counts_one_more_window() {
    let dir = temp_dir("drop");
    let a = limiter(&dir, 1);
    let b = limiter(&dir, 1);
    let slot = a.try_acquire().unwrap().unwrap();
    assert!(b.try_acquire().unwrap().is_none());
    // The owner gives up the request without confirming (as a crash would).
    drop(slot);
    let freed = Instant::now();
    // Reaped now, so it counts until now + W.
    assert!(b.try_acquire().unwrap().is_none());
    let slot = b.acquire().unwrap();
    assert!(freed.elapsed() >= Duration::from_millis(W));
    b.confirm(slot).unwrap();
}

/// The child half of [`test_a_killed_holder_keeps_its_slot_until_it_dies`]: holds one slot of
/// the limiter in `METALBOOK_LIMITER_CHILD_DIR` and sleeps. Runs only when spawned by it.
#[test]
#[ignore = "spawned by test_a_killed_holder_keeps_its_slot_until_it_dies"]
fn child_holds_a_slot() {
    let Ok(dir) = std::env::var("METALBOOK_LIMITER_CHILD_DIR") else {
        return;
    };
    let l = limiter(Path::new(&dir), 1);
    let _slot = l.acquire().unwrap();
    println!("held");
    std::thread::sleep(Duration::from_secs(60));
}

#[test]
fn test_a_killed_holder_keeps_its_slot_until_it_dies() {
    let dir = temp_dir("kill");
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "child_holds_a_slot", "--nocapture"])
        .env("METALBOOK_LIMITER_CHILD_DIR", &dir)
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    // Kill the child on every exit of this test.
    struct Kill(std::process::Child);
    impl Drop for Kill {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let stdout = child.stdout.take().unwrap();
    let mut child = Kill(child);
    let mut lines = BufReader::new(stdout).lines();
    assert!(lines.any(|l| l.unwrap().contains("held")));
    let l = limiter(&dir, 1);
    // Alive and holding: never released, however many windows pass.
    let until = Instant::now() + Duration::from_millis(3 * W);
    while Instant::now() < until {
        assert!(l.try_acquire().unwrap().is_none());
        std::thread::sleep(Duration::from_millis(W / 4));
    }
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    let killed = Instant::now();
    // Dead: the kernel released its lock; reaped and counted for one more window.
    assert!(l.try_acquire().unwrap().is_none());
    let slot = l.acquire().unwrap();
    assert!(killed.elapsed() >= Duration::from_millis(W));
    l.confirm(slot).unwrap();
}

/// An [`HttpGet`] answering every request with `status`, counting the requests it receives.
struct Fixed {
    status: u16,
    calls: Rc<Cell<u32>>,
}

impl HttpGet for Fixed {
    fn get(&self, _url: &str, _headers: &[(String, String)]) -> Result<HttpResponse, KeeperError> {
        self.calls.set(self.calls.get() + 1);
        Ok(HttpResponse {
            status: self.status,
            body: String::new(),
        })
    }
}

#[test]
fn test_a_429_blocks_the_roller_too() {
    let dir = temp_dir("429");
    let keeper = LimitedHttp::new(
        Fixed {
            status: 429,
            calls: Rc::default(),
        },
        limiter(&dir, 7),
        Admission::Wait,
    );
    assert_eq!(keeper.get("u", &[]).unwrap().status, 429);
    let calls = Rc::new(Cell::new(0));
    let roller_http = Fixed {
        status: 200,
        calls: calls.clone(),
    };
    let roller = LimitedHttp::new(roller_http, limiter(&dir, 7), Admission::Try);
    assert_eq!(roller.get("u", &[]), Err(KeeperError::BudgetBusy));
    assert_eq!(calls.get(), 0, "nothing sent while blocked");
    std::thread::sleep(Duration::from_millis(W + 50));
    assert_eq!(roller.get("u", &[]).unwrap().status, 200);
    assert_eq!(calls.get(), 1);
}

#[test]
fn test_metalbook_and_varswap_files_never_mix() {
    let dir = temp_dir("mix");
    let l = limiter(&dir, 1);
    let slot = l.try_acquire().unwrap().unwrap();
    l.confirm(slot).unwrap();
    let ours = state_files(&dir).unwrap();
    assert_eq!(ours.len(), 1);
    let name = ours[0].file_name().unwrap().to_str().unwrap().to_owned();
    assert!(name.starts_with(FILE_PREFIX), "{name}");
    assert!(!name.contains(KEY), "the key never names a file");
    // varswap's file for the same key (`hermes-<same hash>`), spent and blocked far ahead.
    let varswap = dir.join(name.strip_prefix("metalbook-").unwrap());
    std::fs::write(&varswap, format!("{}\n1 1\n2 2\n3 3\nend\n", u64::MAX)).unwrap();
    assert_eq!(
        state_files(&dir).unwrap(),
        ours,
        "varswap's file is not ours"
    );
    // Ours, once its own window has passed, admits again: varswap's spent and blocked budget
    // for the same key does not count against it, and ours never writes varswap's file.
    std::thread::sleep(Duration::from_millis(W + 50));
    let slot = l.try_acquire().unwrap().unwrap();
    l.confirm(slot).unwrap();
    assert_eq!(
        std::fs::read_to_string(&varswap).unwrap(),
        format!("{}\n1 1\n2 2\n3 3\nend\n", u64::MAX)
    );
}
