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

### H1. CI, as a required check (M)

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
- Badge in the README. After the first green run on `main`, set the CI jobs as required checks on `main`
  (branch protection through `gh api`). The GO in message 43335 covers it.
- *Done when* every job passes on the milestone's PR, CI reproduces `docs/benchmarks.md` byte
  for byte, each failing control fails its job, and the checks are required on `main`.

### H2. Verifiable build (M)

- `make verifiable`: `solana-verify build` (crate `solana-verify` 0.5.2, checked on crates.io
  2026-11-02) in the `solanafoundation/solana-verifiable-build` image. The newest tag is
  `4.3.0`; there is no `4.4.0` tag. So the verifiable binary is built with the 4.3.0
  toolchain, not with the 4.5.0 `cargo-build-sbf` that `make bench` uses. That build's CU is
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
  - Out of scope: the maker, the CLI, the keeper (off-chain, cannot move funds).
  - Known limitations: the long mint's rent; quoting during the window.
- *Done when* the void and restart runbooks have been followed verbatim in the P3 drill.

### H5. Fuzzing: `sample_settlement`, conservation and rollback (M)

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
   every state.
   - Invariants: no panic or abort, only `MetalbookError` codes; a rejected instruction
     changes no account.
   - Differential check: an accepted one changes the series exactly as the `settlement_model`
     reference predicts.
3. **`ledger_sequence`** (LiteSVM). Arbitrary sequences of `mint`, `close`, sample, `settle`,
   `redeem`, `claim_writer`, `close_series` and `close_position`, with clock warps,
   donations and several writers and holders.
   - **Conservation** after every step: vault token balance = ledger `vault_owed` +
     donations; at the end, every owed atom paid and the vault empty.
   - **Rollback**: every failing transaction leaves every account it lists byte-identical,
     compared on snapshots taken before it.

Report the executions per second of each target. *Done when* each target has run 1 hour
locally without a finding (or with each finding fixed and its input in the regression set),
and the nightly job runs each for 10 minutes.

## Phase 2: product

### P1. Bounded keeper batching (M/L)

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
5. **Isolate failures**: if a chunk fails, re-read its series and drop those that hold the
   candidate. Retry the rest once, then fall back to one post per series (today's path).
   A failure blocks `settle` only for the series whose candidate is still missing; the rule
   of codex #14 (never settle a subset the keeper knew how to complete) holds per series.
6. **Settle** the series whose cascade allows it, several `settle` instructions per
   transaction (3,176 CU each), chunked the same way.

- **Tests** (LiteSVM, `metalbook-tests/tests/keeper.rs`):
  - batched and unbatched passes reach identical series states and settlement prices on the
    Oct 30 tape shape (16 + 7 series);
  - a chunk with one series already filled by an adversary still stores the others;
  - a failing chunk isolates the failure;
  - the chunker never exceeds 1,232 bytes; a property test over random group sizes builds
    and serializes real transactions.
- **Measurement**: count transactions, bytes and CU per window in LiteSVM for 23 and 26
  series, batched against unbatched. On devnet, settle the Nov 13 series (listed in P4)
  batched, and compare wall clock and transaction count with the Nov 6 baseline (P3). The
  numbers go in `docs/devnet-log.md` and in this file; the 7.7× estimate above is replaced
  by what was measured.
- *Done when* the tests pass, the gain is measured on devnet, and the keeper's binary uses
  batching by default with `--no-batch` as the fallback.

### P2. A second independent keeper, with alerts (M)

- **Independent** means its own payer (H3), its own Hermes key, its own RPC endpoint, and
  its own host or launchd agent. Both keepers post the same canonical candidates; races are
  already benign (the intent check in `keeper.rs`), and `PayerLock` keeps each payer to one
  process.
- **Alerts** come from a separate `metalbook-cli watch` that reads only chain state, so it
  alerts even when both keepers are down.
  - Rules:
    - a window slot still empty two minutes after its print exists;
    - `settle` not done by `window_end + D + 10 min`;
    - a series past `hard_deadline` and not voided;
    - a payer balance under a threshold (0.5 SOL proposed);
    - leftover encoded VAAs;
    - no keeper heartbeat (each keeper writes a status file per pass) for three intervals.
  - Each alert says what is wrong and which runbook (H4) applies.
- **Questions for SOLANA-PROJECTS**:
  - the alert channel (the ACS bus, a webhook, or both);
  - a second Hermes key, and where the second keeper runs.
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
2. **Before Nov 6**: the baseline logging PR, and H4's void and restart runbooks. P4's
   `list-week` too, so the Nov 13 series exist in time.
3. **Nov 6**: P3.
4. **Then**: H5, H3, H2, P1 (measured on Nov 13), P2, and H4's audit scope last, once the
   other changes are in.

Each milestone is reported to SOLANA-PROJECTS when its PR merges, with its numbers.

## Not in this plan

Spread margin, the covered-call vault, daily expiries, binaries and a VIX-like index stay
deferred (roadmap v2). `price_strip` in the maker is dropped for now: marks are computed
off-chain, so its CU saving does not apply to metalbook.
