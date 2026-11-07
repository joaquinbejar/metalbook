# Runbook: stop and restart the keeper

What survives a restart and what does not:

- **Survives (on chain)**: every series' candidates and state. Each pass re-reads them and posts
  only what is missing, so the posting itself resumes where it was.
- **Lost (in memory)**: the keeper's tracking of its own temporary accounts. That covers
  encoded VAAs a failed post left open (`Orphaned`) and posts whose confirmation was uncertain
  (`InFlight`, which may still land until their blockhash expires). It also covers the
  rate-limit cursor (below) and the Hermes cache.
- **Recovered later**: an encoded VAA from either kind is found by the leftover scan of the
  new process and closed, once two scans 300 blocks apart have seen it unchanged. Until then
  it holds rent (about 0.003 SOL each).
- **Not lost**: no candidate. A post that lands after the restart is simply found stored on
  the next pass. Run
`tick` more often than the challenge delay `D` (600 s on devnet): a gap longer than `D` during
a window lets anyone settle a subset of it (`docs/PLAN.md` 4.3, trust assumption).

## Rules

- **One keeper per payer.** The keeper takes `pyth_post::PayerLock` on its payer at start
  (`$PYTH_POST_LOCK_DIR`, default `~/.local/state/pyth-post/payer-<address>.lock`), shared
  with every pyth-post poster on the machine (varswap's keeper included). A second process
  with the same payer exits at once with "another process posts with payer … one poster per
  payer". Never delete the lock file to get past it: find the other process.
- **Stop by PID only.** The Studio runs other repositories' keepers; never `pkill` or
  `killall` by name. Record the PID when starting.
- **Hermes budget.** metalbook's processes share at most 7 requests a minute of a key shared
  with varswap: the primary keeper runs at `--hermes-interval-s 12` (the default and floor),
  a second keeper at 30.

## Start

```sh
source ~/.zshrc   # METALBOOK_HERMES_API_KEY; never print it
export SOLANA_URL=https://api.devnet.solana.com
export METALBOOK_PROGRAM_ID=FPPuou5kgSp6tbwQRwKPBNhrMyf1Xb8n2ib3Yqpbsuwg
export METALBOOK_KEYPAIR=$HOME/.config/solana/metalbook-devnet-keeper.json   # docs/keys.md
nohup target/release/metalbook-keeper --interval-s 60 \
  --exclude <SERIES TO VOID> ... >> keeper.log 2>&1 &
echo $!   # record this PID
```

Run the binary from outside the repository's `target/` if the repository may be rebuilt
during the window (a rebuild replaces the file under a running process). Expect, within the
first minute, either nothing (no series in a window) or lines such as
`<SERIES>: slots +N links +0 fallback false skipped 0 settled false` and one
`pass N: … tx attempts N (ok N, failed 0), attempted bytes … max …; hermes …` line per pass
that sent anything.

## Stop

```sh
ps -p <PID> -o pid,command   # check it is the keeper you started
kill <PID>
ps -p <PID> || echo stopped
```

The lock is released when the process exits; nothing else needs cleaning. An encoded VAA
left open by a post interrupted mid-way is closed later by the leftover scan (below).

## Restart

Start again with the same flags (above) and record the new PID. Then check, in the log:

1. **No lock error** at start. If it says another process posts with the payer, the old
   process (or another repository's tool with the same payer) is still running: find it with
   `ps`, never by deleting the lock.
2. **Resumption**: within one pass, the series in a window show new `slots +N` for the slots
   that came due while it was down, and `pass N` lines show transactions confirmed.
3. **Leftovers**: the leftover scan runs at startup and every 10 passes. An encoded VAA left by
   the stopped process is reported only after two scans at least 300 blocks apart saw it
   unchanged, so `closing N orphaned encoded VAA(s)` may appear about 10 minutes after the
   restart. Until then it costs rent only (about 0.003 SOL each).
4. **Settlement**: after the window, each series settles; if a restart took longer than `D`
   during the window, check its `settle_method` (and `settlement_samples`) in `show-series` and record it: anything other than `window` with 30 samples means the gap cost slots.

## Failure modes

- **Hermes 429 storm**: the source cools down 61 s after a 429, and the pass stops at the
  series whose tick hit the limit. The next pass rebuilds its series list (the `--series`
  list, or every series of the program) and rotates it to start at that series if it is
  still listed (`resume_at` in `metalbook-cli/src/bin/keeper.rs`). The cursor lives in
  memory only, so a restart starts from the first series again. Persistent 429s mean another
  process is spending metalbook's share: check every process's `--hermes-interval-s`.
- **RPC errors**: an error reading the series list or the clock skips the whole pass
  (`pass N: skipped, the chain could not be read: …`) and the next pass tries again; the process
  does not exit (it used to, before 2026-11-02). Each failing series prints its error and the loop goes on; a post that fails
  without its candidate on chain stops that series' tick before `settle` (codex #14), so a
  subset is never settled by the keeper itself.
- **Low payer balance**: each post costs 0.000025 SOL in fees (5 signatures) plus temporary rent that
  returns at the end of the post. Keep the payer above 0.5 SOL.
