# metalbook: improvement plan (roadmap v2)

Scope agreed with SOLANA-PROJECTS on 2026-11-02 (GO, bus message 43335), drafted from the
roadmap review (bus 42680). Two phases: hardening, then product. One branch and one PR per
milestone, small commits, codex reviews every PR before merge. `docs/PLAN.md` stays the source
of truth for the protocol; this file tracks the improvement milestones and is updated as each
one lands.

Global rules apply to every milestone (`rules/global_rules.md`): no panics outside tests,
checked arithmetic, explicit rounding, dependencies at the latest stable version written
`MAJOR.MINOR` (checked on crates.io and cited), a forged-account negative test for every
validation rule, and the CU of every instruction measured by `make bench`. **No milestone
here adds or changes an on-chain instruction**; if one has to (a fuzzing finding), it gets its
own PR with the negative tests, a re-run of `make bench`, a new verifiable build (H2) and a
devnet upgrade recorded in `docs/devnet-log.md`.

## Facts this plan starts from (measured, 2026-10-30 to 11-02)

- **No CI.** The repository has no `.github/workflows`. `make pre-push` runs `fmt-check lint
  lint-control program-control program quotebook-program pyth-programs test doc`; two targets
  need the network: `quotebook-program` clones quotebook tag `v0.1.0` (commit `6417a0f`), and
  `pyth-programs` dumps the Pyth receiver and Wormhole from devnet, pinned by SHA-256.
- **Toolchains in use**: Agave CLI 4.4.0, `cargo-build-sbf` 4.5.0, platform-tools v1.57 (SBF
  Rust 1.95.0): the same pins as optfix's CI (optfix `d536d42`, M9).
- **One hot key does everything on devnet**: `77ieYLKspRnGzAt1AK6ZVsttnTY6T7EY2k7CpTZeCPPV`
  is the program's upgrade authority, the `Config` admin, the payer of every `create_series`
  and the keeper's payer (69.8 SOL on devnet today).
- **Keeper workload on Oct 30**: 23 series × 30 window slots = 690 posts, each post being two
  transactions (encoded VAA create/write/verify, then `post_update` + `sample_settlement` +
  `reclaim_rent` + `close_encoded_vaa`). Throughput was bound by devnet confirmations, about 6
  posts a minute; the `settle` calls landed between 20:08 and 21:32Z. All 16 XAU series settled at the same
  3877.352833 and all 7 XAG series at 49.184986: every window slot is one canonical print per
  metal, shared by every series of that metal and expiry.
- **Transaction headroom**: the post transaction above (`sMLMjZd7…`) is 911 bytes and 38,247
  CU. The legacy limit is 1,232 bytes, so 321 bytes remain. One more `sample_settlement` adds
  about 39 bytes (a new 32-byte series key, plus 7 bytes of instruction header, account
  indices and data) and 2,621 CU (`docs/benchmarks.md`). Estimate, to be measured: up to 8
  extra series per post, so 9 per post. That gives 3 posts per slot for 16 XAU + 7 XAG
  series, 90 per window instead of 690 (about 7.7×). The "690 to 60" figure would need one
  post per metal per slot, which needs an address lookup table or a price update shared
  across transactions; it is unproven.
- **Devnet calendar**: weekly Fridays, 16:00 New York. Listed and live: 26 series for Nov 6
  (16 XAU, 10 XAG at 50 oz, expiry `1793998800`, 21:00Z). The ten Oct 30 XAG 50 oz series
  have no samples; `hard_deadline_s` is 7 days and `D` 600 s, so they can be voided from
  2026-11-06 20:10Z.
- **Rent**: a `Series` account (504 bytes) holds 0.00321 SOL on devnet. Its vault holds about
  0.0015 SOL. The long mint's rent cannot be reclaimed (`docs/PLAN.md` 4.6). Nothing calls
  `close_series` or `close_position` yet.
- **Settlement logic tests**: `metalbook-core` has property tests for payoffs, the ledger and
  settlement (`tests/settlement_props.rs`); nothing fuzzes the program entry point.

## Phase 1: hardening

### H1. CI on every PR (M)

Copy optfix M9 (`.github/workflows/ci.yml`, `.github/actions/toolchains/action.yml`) and
adapt it:

- Same pins and SHA-256 checks (Agave 4.4.0 tarball, platform-tools v1.57, host Rust exact
  pin), version asserts before any check, `--locked` through a `CARGO_FLAGS` Makefile variable,
  `RUST_TOOLCHAIN` replacing `+stable`, actions pinned by commit, read-only token, no secrets.
- Jobs whose union is exactly `make pre-push`, with a step that fails if the `pre-push`
  prerequisites change without the workflow (as optfix). Split `test` into shards only if
  one runner takes over 30 minutes, measured on the first run.
- `bench` job: `make bench` then `git diff --exit-code docs/benchmarks.md`.
- Network targets: `quotebook-program` stays pinned by commit; `pyth-programs` keeps the
  devnet dump and SHA-256 check, with the two `.so` files cached by their hashes. A cache
  miss dumps again, and a hash mismatch fails. The program binaries are never committed.
- Failing controls on scratch PRs (closed unmerged): an unformatted line, a planted failing
  test, an edited number in `docs/benchmarks.md`, and a wrong `PYTH_RECEIVER_SHA256`. Each
  must fail its own job, for that cause.
- Badge in the README. **No branch protection on metalbook** (Joaquin's decision, relayed in
  bus 43727: required CI on `main` is for optfix only). Instead, a process rule: CI runs on
  every PR, and a PR is merged only when every job is green.
- *Done when* every job passes on the milestone's PR, CI reproduces `docs/benchmarks.md` byte
  for byte, and each failing control fails its job.

### H2. Verifiable build (M)

- `make verifiable`: `solana-verify build` (crate `solana-verify` 0.5.2, checked on crates.io
  2026-11-02, installed with `cargo install --locked solana-verify --version 0.5.2`) in the
  `solanafoundation/solana-verifiable-build` image, pinned by digest:
  `4.3.0@sha256:12fd4c0a0790f0fc41ef74b0cdb6bccc167ba6137eb1adb749bac649481c86bd` (Docker Hub,
  2026-11-02). The newest tag is `4.3.0`; there is no `4.4.0` tag. So the verifiable binary is
  built with the 4.3.0 toolchain, not with the 4.5.0 `cargo-build-sbf` that `make bench` uses.
  `docs/verifiable-build.md` keeps the evidence: the image digest, the `cargo build-sbf
  --version` printed inside the image, the solana-verify version, the commit, and the
  resulting hash. That build's CU is
  measured and recorded as its own row set in `docs/benchmarks.md`, because the deployed
  binary is the one whose costs matter.
- The program hash (`solana-verify get-executable-hash`) is recorded in
  `docs/verifiable-build.md`. A CI job rebuilds it and fails on a different hash.
- Devnet: upgrade the program to the verifiable binary built from the same commit as `main`,
  then check it locally with `solana-verify verify-from-repo -ud`. The upgrade is a devnet
  action, covered by the standing devnet GO. Uploading the verification to the public
  OtterSec API (the remote job and the on-chain verify PDA) is outward-facing, so it waits
  for the coordinator's go.
- *Done when* two clean builds produce the same hash, the CI job reproduces it, and the
  devnet program's hash equals it.

### H3. Separate the upgrade authority from the payer (S on devnet, plan only for mainnet)

There are four roles, held today by one key: the upgrade authority, the `Config` admin
(`set_config`, `set_calendar`), the series lister (any payer), and the keeper payer.

- **Devnet, done in this milestone.**
  - A new upgrade-authority keypair, kept off the keeper's directories and never used to pay
    fees.
  - The admin rotated with `set_config` to a second new key.
  - The keeper and the lister on their own payer.
  - Every signature recorded in `docs/devnet-log.md`.
  - Tests (they already cover `init_config` against forged `ProgramData`): after an admin
    rotation, the old admin is rejected by `set_config` and `set_calendar`, and a forged
    `Config` is rejected.
- **Mainnet, plan only (`docs/keys.md`), no action.**
  - Upgrade authority and admin behind a Squads v4 multisig (threshold and members are
    Joaquin's call), with a time lock on upgrades.
  - Keeper payers are hot keys with a SOL balance alarm (P2) and no authority over anything.
  - Final step: drop the upgrade authority (`--final`) once audited, or keep it under the
    multisig. The decision goes in `docs/PLAN.md` with its reasons.
- *Done when* no devnet key holds two roles and `docs/keys.md` covers the mainnet procedure.

### H4. Runbooks and audit scope (S)

- `docs/runbooks/`: one page each.
  - Keeper restart, including the payer lock, resume after a rate-limited pass and orphan
    cleanup.
  - Hermes outage or 429 storm.
  - RPC outage.
  - Low payer balance.
  - Orphaned encoded VAAs.
  - A stuck settlement: the cascade of `docs/PLAN.md` 4.3, which step applies when, and what
    `SettlementNotReady` means.
  - **Void**: when it happens, who calls `settle`, what holders and writers do after it
    (`redeem` for zero, `claim_writer` for the full collateral), then `close_series`.
  - Program upgrade and rollback.
  - Key compromise of each role in H3.
  - Each page names its commands, with exact arguments and the expected output.
- `docs/audit-scope.md`:
  - In-scope files with line counts: the program, `metalbook-core` accounts and settlement,
    the SDK instruction builders.
  - Trust assumptions: an honest keeper within `D` (4.3), Pyth and Wormhole guardian
    signatures, and the admin's calendar.
  - Invariants an auditor should break: the ledger identity, settlement only through the
    cascade, no payout above the maximum.
  - Off-chain components, scoped separately with their own risks:
    - The keeper and the CLI cannot move user funds through the program. But they sign with
      hot keys that hold SOL and, on devnet today, every authority (H3). A bug or a
      compromise can spend the payer's SOL, leak the Hermes key, or leave orphaned accounts.
      A keeper that stops posting lets a subset settle under the trust assumption of 4.3.
    - The maker sizes quotes and signs QuoteBook fills from its own wallet: its risk is the
      maker's inventory, not the protocol's.
    - These are reviewed for key handling and redaction, not audited as on-chain code.
  - Known limitations: the long mint's rent; quoting during the window.
- *Done when* the void and restart runbooks have been followed verbatim in the P3 drill.

### H5. Fuzzing: `sample_settlement`, conservation and rollback (M)

(Done.) As built: `fuzz/` (its own workspace) with `settlement_model`, `sample_settlement` and
`ledger_sequence`; the harness in `fuzz/harness/`, replayed on stable with the committed
regressions by `metalbook-tests/tests/fuzz_regressions.rs`; three fault controls
(`make fuzz-control`); `.github/workflows/fuzz.yml` nightly. One hour per target ran without a
finding in the program: 46,059, 290 and 37 executions per second (`fuzz/README.md`). The only
crash was a harness bug (two equal donations sent as one transaction twice), fixed and kept as
a regression.

Pattern: a `fuzz/` crate outside the workspace, as tessera-rl did. It uses `cargo-fuzz` 0.13,
`libfuzzer-sys` 0.4 and `arbitrary` 1.5 (all checked 2026-11-02), a nightly-only workflow,
and every crash input saved as a regression replayed by a stable `cargo test`, so CI (H1)
covers it.

Three targets:

1. **`settlement_model`** (pure, fast). Arbitrary sequences of print offers and clock steps
   go through `metalbook_core`'s candidate updates and `decide`. Invariants:
   - a candidate only improves;
   - every `D` restarts on the right kind only;
   - `decide` never settles on a lower step while a higher one is pending;
   - the settlement price lies between the minimum and maximum of the accepted candidates it
     averages.
2. **`sample_settlement`** (LiteSVM, the real program). A `PriceUpdateV2` account with
   arbitrary bytes, owned by the receiver id and also by forged owners, against series in
   every state. The input mixes arbitrary bytes with **seeded valid updates** (a real
   `PriceUpdateV2` with mutated fields: price, conf, exponent, times, feed id, verification
   level), so most runs exercise acceptance and not only rejection.
   - **Acceptance oracle**: an independent model (`settlement_model` plus a decoder written
     from the `PriceUpdateV2` layout, not pyth-lite) predicts accept or reject before the
     transaction runs. The test fails on any disagreement, in either direction, so a
     regression that rejects everything fails too.
   - On accept: the series changes exactly as the model predicts. On reject: the error is a
     `MetalbookError` or a standard `ProgramError` (malformed accounts), and the series and
     every program and token account are byte-identical. Only the fee payer's lamports
     change, by exactly the transaction fee, which is modelled separately.
   - No panic or abort in any case.
3. **`ledger_sequence`** (LiteSVM). Arbitrary sequences of `mint`, `close`, sample, `settle`,
   `redeem`, `claim_writer`, `close_series` and `close_position`, with clock warps,
   donations and several writers and holders.
   - **Conservation** while the series is open: vault token balance = ledger `vault_owed` +
     donations received.
   - **At `close_series`** (`processor/close.rs`): the donations are swept to the rent
     payer's USDC account. The rent payer's USDC rises by exactly the donations, its lamports
     by exactly the series and vault rent, and both accounts are gone.
   - **Draining suffix**: an arbitrary sequence cannot promise an empty vault. Every case
     therefore ends with an explicit valid suffix: settle if possible, redeem every long,
     claim every writer, close. After it, every owed atom has been paid and the vault is
     closed.
   - **Rollback**: a failing transaction leaves every program-owned and token account it
     lists byte-identical. The fee payer's lamports drop by exactly the fee, checked against
     the fee model and not by byte equality.

Report the executions per second of each target. *Done when* each target has run 1 hour
locally without a finding (or with each finding fixed and its input in the regression set),
and the nightly job runs each for 10 minutes.

## Phase 2: product

### P1. Bounded keeper batching (M/L)

**Built** (`metalbook-keeper/src/batch.rs`, `Keeper::pass`; `RpcChain::{batch_capacity,
post_batch, settle_batch}`). Measured in LiteSVM on the real path (Wormhole and the Pyth
receiver as deployed on devnet, the recorded Hermes XAU update): one post samples **up to 10
series**. Its last transaction is 1,213 of 1,232 bytes and 60,380 CU at 10 series, against
871 bytes at 1 series. The capacity is computed per update from the planned transaction
(`last_post_tx_size`), since the Merkle proof length varies. The estimate above was 9.

Per window, each print is posted `ceil(n / 10)` times per feed instead of `n`:
- **Oct 30 shape** (16 XAU, 7 XAG): 3 posts per print instead of 23, about 7.7× fewer.
- **Nov 6 and 13 shape** (16 XAU, 10 XAG): 3 instead of 26, about 8.7× fewer.

The devnet run on Nov 13 measures wall clock and transaction count against the Nov 6 baseline.

Today `Keeper::tick` works one series at a time and `Chain::post` makes one post per series.
New flow per pass:

1. **Plan**: for every live series, compute the candidates it lacks, exactly as `tick` does
   today (slots, links, fallback, with the same `accept_print` eligibility). Each candidate is
   an intent with a series, a feed and a `PrintMsg`.
2. **Group** by feed and the exact signed message (`PrintMsg` equality on every field, then the
   `AccumulatorUpdate` message bytes), never by time alone.
3. **Revalidate** each group just before sending: re-read every series; drop the ones that
   already hold the intent (another keeper) or no longer accept it (settled, filled).
   `sample_settlement` fails on a filled slot, so one stale member would fail the whole
   transaction.
4. **Chunk** by transaction bytes (≤ 1,232, measured on the serialized transaction, not
   estimated) and CU (≤ the requested limit, 2,621 per sample from `docs/benchmarks.md` plus
   the measured `post_update`). `pyth_post::post` already takes the consume instructions from
   a closure, so a chunk is one post with N `sample_settlement` instructions. A second chunk
   of the same print is a second post, until a measured need justifies an address lookup
   table or a price update kept open across transactions (a pyth-post change, agreed with
   VARSWAP first).
5. **Isolate failures**, by outcome, never assuming atomic failure:
   - **Confirmed rejection** (the transaction executed and failed, `pyth_post` returns an
     error that is not `Uncertain`, and the encoded VAA is handled as today: closed, or
     `Orphaned` and kept in `RpcChain`'s pending list). Re-read the chunk's series, drop
     those that now hold the candidate, and resend the rest once in a new chunk. If that
     fails too, fall back to one post per series (today's path) for this pass only.
   - **Uncertain** (`PostError::Uncertain`: the transaction may still land until
     `valid_until`). No retry and no fallback for that chunk while it is live: its encoded
     VAA is tracked as `InFlight` with its last valid block height, exactly as today. The
     chunk's series are re-planned on the next pass, after `close_orphans` has resolved the
     in-flight account. So a late landing can never be posted twice: a re-plan revalidates,
     and the candidate is already there.
   - **Quota or network errors** (Hermes 429, RPC unreachable) stop the pass as today
     (`resume_at`). They are not treated as a rejected member.
   - **Bounds**: at most one resend per chunk per pass, and at most 3 passes for a candidate
     before it is reported as stuck (an alert in P2). A failure blocks `settle` only for the
     series whose candidate is still missing; the rule of codex #14 (never settle a subset
     the keeper knew how to complete) holds per series.
6. **Settle** the series whose cascade allows it, several `settle` instructions per
   transaction (3,176 CU each), chunked the same way.

- **Tests** (LiteSVM, `metalbook-tests/tests/keeper.rs`):
  - batched and unbatched passes reach identical series states and settlement prices on the
    Oct 30 tape shape (16 + 7 series);
  - a chunk with one series already filled by an adversary still stores the others;
  - a failing chunk isolates the failure;
  - **late confirmation**: a chunk reported `Uncertain` that lands after the next pass has
    started posts nothing twice, and the remaining series still progress;
  - **late create**: an encoded VAA whose creation lands after `valid_until` was assumed is
    found by the leftover scan and closed. Every temporary account is either kept in the
    pending list or reclaimed, never leaked;
  - **race between revalidation and send**: another keeper fills a member in between. The
    chunk is rejected (confirmed), the member is dropped, and the rest is stored on the
    resend;
  - the chunker never exceeds 1,232 bytes; a property test over random group sizes builds,
    signs and serializes the real transactions, compute-budget and cleanup instructions
    included.
- **Measurement**: count transactions, bytes and CU per window in LiteSVM for 23 and 26
  series, batched against unbatched. Bytes come from signed, serialized transactions, and CU
  from runs against the deployed ELF (the H2 verifiable build), never from summing
  standalone instruction rows. On devnet, settle the Nov 13 series (listed in P4)
  batched, and compare wall clock and transaction count with the Nov 6 baseline (P3). The
  numbers go in `docs/devnet-log.md` and in this file; the 7.7× estimate above is replaced
  by what was measured.
- *Done when* the tests pass, the gain is measured on devnet, and the keeper's binary uses
  batching by default with `--no-batch` as the fallback.

### P2. A second independent keeper, with alerts (M)

**Decided** (coordinator, bus 44325). Heartbeats and alerts go to the ACS channel
`metalbook-keepers`, with alerts also sent to the coordinator by DM. A heartbeat is posted on
state change and otherwise every 10 minutes. The second keeper runs on the Studio for now, so
it is process-independent only; host independence is a pre-mainnet gate in
`docs/mainnet-readiness.md`.

**Built**: `metalbook-cli/src/watch.rs` (pure alert evaluation, heartbeat format, the
`ai-crew-sync` CLI transport), the binary `metalbook-watch`, and the keeper's `--heartbeat`.
The optional `watch --hermes` oracle check is not built: without it, overdue slots stay
`unknown`.

- **Independent** means its own payer (H3), its own RPC endpoint, and its own host or
  launchd agent. Both keepers post the same canonical candidates; races are
  already benign (the intent check in `keeper.rs`), and `PayerLock` keeps each payer to one
  process.
- **Alerts** come from a separate `metalbook-cli watch`, which reads chain state and a
  remote heartbeat, so it alerts even when both keepers are down. It cannot see when an
  off-chain Pyth print first existed: a missing print leaves no chain evidence, and the
  receiver account of a past post may already be closed. So lag is measured against the
  **schedule**, not against the print:
  - **Window slot overdue**: slot `i` still empty at `t_i + slot_tolerance_s + grace`
    (grace 120 s proposed). Its state is `overdue`, with oracle availability `unknown`.
    With the optional Hermes check below, it becomes `missing at source` (no canonical print
    exists, which is expected during a halt, and the cascade handles it) or `not posted`
    (the print exists and no keeper posted it, which is a keeper fault).
  - **Optional oracle check**: `watch --hermes` queries Hermes for overdue slots only, at
    most one request a minute, taken from metalbook's budget below (without it, states stay
    `unknown`). It is scoped separately, since it adds a credential user and a quota share.
  - **Settle overdue**: not settled by `window_end + D + 10 min`.
  - **Void pending**: past `hard_deadline + D` and not settled or voided.
  - **Payer balance**: under a threshold, 0.5 SOL proposed.
  - **Leftover encoded VAAs** of either payer, found by `LeftoverScanner`.
  - **Keeper heartbeat**: each keeper posts a heartbeat after every pass to a transport the
    watcher reads remotely. Proposed: an ACS bus message on a `metalbook-keepers` channel,
    or a note updated per pass; the coordinator chooses. A missing heartbeat for three
    intervals raises an alert. A local status file is not enough when the keepers run on
    different hosts.
- **Host independence**: two LaunchAgents on the Studio are independent processes and
  payers, not independent hosts. The plan claims host-failure independence only if the
  second keeper runs on another machine; otherwise the doc says "process-independent".
- **Controls** (deterministic, on a LiteSVM clock with a scripted source):
  - a missing print gives `overdue` and `unknown`, or `missing at source` with the Hermes
    check;
  - a dead keeper gives a heartbeat alert after three intervals, while the other keeper
    keeps settling;
  - loss of the remote heartbeat transport gives a `heartbeat unknown` alert, not a false
    "keeper dead";
  - each alert names its runbook.
- **Hermes budget** (coordinator, bus 43727). The Hermes key is shared with varswap, at about
  10 requests a minute on the timestamp endpoint. metalbook gets **at most 7 a minute in
  total**, for the primary keeper, the second keeper and `watch --hermes` together; varswap
  gets 3. Each process paces itself with `HermesSource::with_min_interval`, and the
  intervals are chosen so the sum stays at or under 7:
  - primary keeper: 12 s, 5 a minute (today's 10 s, 6 a minute, is lowered);
  - second keeper: 30 s, 2 a minute; or 60 s, 1 a minute, when `watch --hermes` runs;
  - `watch --hermes`: 60 s, 1 a minute.

  The binary takes the interval as a flag with a floor of 12 s, and the runbook records
  which process runs at which interval. A test checks the pacing (requests per simulated
  minute, at each interval). The cost: a slower backfill, measured in the P3 baseline at
  5 a minute.
- **Questions for SOLANA-PROJECTS**:
  - the alert and heartbeat channel (the ACS bus, a webhook, or both);
  - where the second keeper runs.
- *Done when* both keepers have run one devnet window together, and a drill that stops one of
  them (by PID) leaves the other settling on time while `watch` raises the heartbeat alert.

### P3. Void and restart drill, 2026-11-06 (S, fixed date)

- **20:10Z**: void the ten Oct 30 XAG 50 oz series. Run `settle`, check `method = Void` and
  that nothing is owed, then run `close_series` on each (no longs were minted, so the ledger
  owes nothing). Record each signature and the rent returned. The keeper runs with those
  series excluded: a fallback posted by anyone before 20:10Z would settle them on the
  fallback instead, and the drill would test the wrong path. If that happens, it is recorded
  as found.
- **21:00Z**: settle the 26 Nov 6 series with the current, unbatched keeper. This is the
  baseline for P1: per-pass logging of transactions, bytes and wall clock, added before
  Nov 6 in a small keeper PR.
  - **Restart drill**: during the window, stop the keeper by PID, wait two minutes, then
    restart it following the H4 runbook. Check that it resumes, that the payer lock is
    released, that the leftover scan is clean, and that every series still settles on the
    full window.
- *Done when* `docs/devnet-log.md` records the void, the closes, the baseline and the drill
  with their signatures.

### P4. Safe auto-listing and rent reclaim (M)

- **`metalbook-cli list-week`**:
  - From the on-chain `Calendar` and a Hermes `/latest` print no older than a limit (5 min
    proposed; it refuses when the market is closed and the print is stale), it computes the
    strike grid of the params (ATM ±5%, $50 XAU, $1 XAG) and the caps from `cap_bps`.
  - It creates only the missing series PDAs, so it is idempotent.
  - Bounded: at most a configured number per run (26 per week today).
  - Dry-run by default, printing every series and its cost (`create_series` 29.6k / 34.2k
    CU plus rent); `--execute` sends.
  - It never lists an expiry closer than `min_listing_s`. The program rejects it anyway, and
    a negative test shows the CLI does not try.
- **`metalbook-cli reclaim`**:
  - Finds `Settled` and `Voided` series whose ledger owes nothing and calls `close_series`;
    the rent goes to the recorded payer, as the program enforces.
  - With `--writer`, closes that writer's claimed positions.
  - Dry-run by default, like `list-week`.
- *Done when* `list-week` lists the Nov 13 series on devnet (the input to P1's measurement),
  `reclaim` closes the Oct 30 series that owe nothing, and both are recorded with
  signatures.

## Order and dates

1. **H1** CI, first, because every later PR is checked by it.
2. **Before Nov 6**: the baseline logging PR (it also lowers the keeper's Hermes pacing to 12 s, the P2 budget), and H4's void and restart runbooks. P4's
   `list-week` too, so the Nov 13 series exist in time.
3. **Nov 6**: P3.
4. **Then**: H5, H3, H2, P1 (measured on Nov 13), P2, and H4's audit scope last, once the
   other changes are in.

Each milestone is reported to SOLANA-PROJECTS when its PR merges, with its numbers.

## Not in this plan

Spread margin, the covered-call vault, daily expiries, binaries and a VIX-like index stay
deferred (roadmap v2). `price_strip` in the maker is dropped for now: marks are computed
off-chain, so its CU saving does not apply to metalbook.
