# metalbook: phase 2 plan (devnet operations)

Status: **draft r2**, for codex review (r2 answers codex's findings on r1: R1's shared Hermes
budget, R5's alert timing, R6's immutable outcome records and the close gate). Brief from SOLANA-PROJECTS, bus 51622 and 51663
(2026-11-03). Baseline: `main` at `275525d`. Devnet only: no mainnet action in any milestone.

**Nothing here is implemented before the Nov 6 drill, and nothing touches the keeper binary
before it.** The order after the drill is the optfix 0.3 bump (bus 47754) and H3 part 2 (the key
separation in `docs/keys.md`), then this plan.

**No milestone adds or changes an on-chain instruction.** `docs/PLAN.md` stays the source of
truth for the protocol. Process and rules are those of `docs/PLAN-improvements.md`: one branch
and one PR per milestone, `make pre-push` clean before each push, green CI on the exact head,
codex reviews every PR, a report to SOLANA-PROJECTS when a milestone closes. Effort: **S** up
to a day, **M** two to four days.

Scope:
1. The metalbook section of the shared dashboard (series, prints, settlements, voids). varswap
   owns the shell and the JSON format.
2. The real quotebook × metalbook trade: series and inventory for quotebook's demo maker.
3. The Nov 13 batched measurement and the Nov 20 two-keeper drill.
4. A weekly, idempotent roller that lists 1 oz expiries.

## Facts this plan relies on (read on devnet, 2026-11-04)

**Series and config**
- `Config.multiplier` is `[1000, 1000]`: XAU and XAG are both **1 oz** today. XAG was lowered
  from 50 oz on devnet (`docs/devnet-log.md`). So `list-week` already lists 1 oz; the 50 oz
  series are only the Nov 6 ones, which `quotebook` read as the current shape.
- **Nov 13 is listed**: 24 series, 14 XAU and 10 XAG, all 1 oz, expiry `1794603600`. XAG capped
  calls at strikes 48, 49, 50, 51 and 52, locking 4.8 to 5.2 USDC per contract. Only
  `B66M…Zx21` (50/55) is written: 3 longs, `vault_owed` 15 USDC. The listing signatures go into
  `docs/devnet-log.md` with the Nov 6 record.
- `create_series` needs a signer that pays and no admin
  (`metalbook-program/src/processor/series.rs`), so **the roller does not need the admin key**;
  it can use a key that only holds SOL. Rent goes back to the recorded `rent_payer`.
- Calendars (both metals): Oct 30, Nov 6, 13, 20, Dec 4, 11, 18. Capacity is 64. **No Nov 27**,
  and nothing after Dec 18.
- A `Series` account stores `filled_slots`, `sample_sum`, `settle_method`, `settlement_price`
  and `settlement_samples`, not the price of each slot. So the chain gives "how many prints
  and what average", never the per-slot tape. The settlement signature is not stored either.
- **`reclaim` and `close-series` delete the account.** The 23 settled Oct 30 series are already
  closed, so their result now exists only in `docs/devnet-log.md`. A dashboard that reads only
  live accounts loses a series as soon as it is reclaimed.

**Keeper**
- Without `--series`, the keeper takes every series of the program, so newly listed series are
  picked up with no restart.
- It runs by hand today (`~/tmp/worktrees/metalbook/keeper-run/`). No LaunchAgent exists, so
  **no series settles unless someone starts a keeper**. A weekly trade needs that to change.
- Batching: 10 series per post at most (P1). Nov 13 shape: `ceil(14/10) + ceil(10/10)` = 3
  posts per print against 24 unbatched, 90 against 720 per window.
- Hermes budget: 7 requests a minute across metalbook processes (primary 12 s, second 30 s:
  exactly 7), and `list-week` reads Hermes too. `HEARTBEAT_TIMEOUT_S` in the watcher is 1,800 s.

**Funds**
- Maker `7tVP…cACQ`: 14.23 USDC, 2 `B66M` longs, and the writer of all 3 `B66M` contracts.
- After Nov 13, `redeem` of its 2 longs plus `claim_writer` returns `15 − payout` USDC, where
  `payout` (0 to 5) is what the taker's one long takes. So 10 to 15 USDC come back.
- Taker `3ZBj…sqLs` (quotebook's) holds the third long and redeems it itself.

**Siblings**
- quotebook plan: PR #40, head `1877f70`, D5: the demo maker is the writer and trades XAG capped
  calls only, one lot, with about 14 USDC.
- varswap plan: PR #45, P3: `section.json` version 1, a static shell, publication on GitHub
  Pages from a scheduled workflow (coordinator, bus 51643). `metalbook` is public.

## R1. Weekly series roller (M)

An idempotent job, on the model of varswap's `scripts/window-roll.sh`, except that the decision
logic is Rust (pure, tested) and only the launchd plumbing is shell.

- **`metalbook-admin roll`** (dry run by default, `--execute` sends):
  1. Reads the config and both calendars.
  2. **Target expiry**: the first calendar expiry at least `--min-lead-s` ahead (default
     259,200, 3 days) and not before `--min-expiry`.
  3. **Quiet period**: does nothing, before any Hermes request, when `now` is within 2 h of any
     calendar expiry, so the keepers keep the whole Hermes budget during their window. This
     avoids contention in the normal case; it does not by itself bound the rate (a keeper
     backfilling old series outside that span can spend the whole 7), which step 3a does.
  3a. **Shared Hermes budget** (r2, codex): every metalbook process takes each Hermes request
     from one cross-process limiter, on the model of varswap's `keeper/src/limiter.rs` (one
     state file under `flock`, an entry per request counted from admission until a window after
     it was answered, a 429 blocking all processes 61 s). metalbook's file has its own name
     (`metalbook-hermes-<fnv1a of the key>`, never the key) and its own limit, **7 a minute**, so
     it never counts against varswap's 3. The keepers adopt it (after the Nov 6 drill; their
     `--hermes-interval-s` pacing stays as a floor). The roller only **tries** to acquire: when
     no slot is free it skips that metal for this run and says so (`hermes budget busy`), and
     the next run, 6 h later, tries again. So the aggregate is bounded by the limiter, not by
     timing, and the roller always yields to keepers.
  4. **Multiplier check**: refuses unless the config multiplier equals `--multiplier-milli-oz`
     (default 1,000). A later `set_config` then cannot silently list 50 oz series.
  5. For each metal, runs the same plan as `list-week`: a put and a capped call at every grid
     strike within ±5%, creating only the missing addresses. At most 2 Hermes requests a run,
     spaced 12 s, each print checked with `listing::fresh_print`.
  6. **A stale print is a skip, not a failure**: market closed is the normal case from Friday
     to Sunday. Any other read error stops the run with nothing sent for that metal, and the
     exit status is non-zero.
  7. Bounds: `--max` new series a run (default 28), and `--max-per-expiry` listed per metal and
     expiry (default 18), so a run-away spot cannot keep adding series a keeper must then
     sample. Reaching a bound stops and says so.
  8. Reclaim, added by R6 and absent until then: closes what owes nothing, only when its record
     is published.
- **Top-ups**: a later run adds strikes that have newly entered the band, up to the bound. That
  is what the demo needs (a strike near spot) and it costs one more series.
- **`scripts/series-roll.sh run|install|uninstall|status <name>`**: LaunchAgent
  `com.metalbook.roll.<name>`, at load and every 6 h, logging to
  `~/.local/share/metalbook-roller/<name>/roll.log`. The Hermes key comes from
  `zsh -c 'source ~/.zshrc; exec …'`, never from the plist or a file. The binary is copied out
  of the checkout. Log rotation by size (10 MB, keep 5, copy and truncate) runs at the start of
  each `run`.
- **Key**: `~/.config/solana/metalbook-devnet-roller.json`, new (`solana-keygen new --silent`,
  as `docs/keys.md`), funded with 1 SOL from `77ie…CPPV`. Cost per series from the plan's
  facts: 0.00321 SOL for the `Series`, about 0.0015 for the vault, and the long mint's rent is
  not reclaimable. The first run measures the real weekly net and records it.
- **First install uses `--min-expiry 1795208400` (Nov 20)**: a run before Nov 13 must not top up
  the Nov 13 series and change the shape R3 measures.
- **Calendar**: the roller never calls `set_calendar`. It runs out after Dec 18, so:
  - two new alerts in `metalbook-watch`: `NoNextSeries` (a metal has no live 1 oz series expiring
    at least 24 h ahead, checked only while the market is open) and `CalendarLow` (fewer than 2
    future expiries). Both name a new `docs/runbooks/series-roll.md`.
  - a `set-calendar` by the admin, by hand and recorded, before Dec 4. Dates: see "Questions".
- **Strikes and expiries are coordinated with quotebook** (R2): the plan is the existing grid,
  every calendar expiry.
- **Tests**: pure, on a scripted clock: target expiry across the Nov 27 gap, across a lead
  boundary and past the end of the calendar; the quiet period on both edges; multiplier
  mismatch; stale print gives skip, Hermes 429 gives failure; both bounds. Limiter, on a
  scripted clock and a shared temporary directory: a keeper backfilling at its full rate
  **outside** the quiet period plus a roll run never admits more than 7 requests in any 60 s
  window (the roller skips with `hermes budget busy`); a process killed while holding a slot
  frees it; a 429 blocks the roller too; metalbook's file and varswap's never mix. On a fake RPC: a
  second run sends nothing; a run failing halfway is finished by the next one. Negative: no
  expiry closer than `min_listing_s` is ever attempted. Script (`scripts/tests/series-roll.sh`,
  offline with a fake binary, as varswap's): the plist holds no secret (a planted key value
  must not appear), `run` is idempotent, `status` reports the agent.
- *Done when* the tests pass, the agent lists the Nov 20 series on devnet with no human in the
  loop, a later run sends nothing, and the signatures and the net cost are in
  `docs/devnet-log.md`.
- **If R1 is late**: `list-week --execute` by hand lists Nov 20. It exists today.

## R2. Series and inventory for the demo maker (S; needs quotebook)

Coordinated with `joaquin/s-e3613763b15f`. metalbook supplies, quotebook writes and trades.

- **Series**: quotebook's D5 can start on the **Nov 13** XAG capped calls already live (above),
  and it follows the roller's output from Nov 20. Long mints and strikes from `show-series`.
- **USDC**: `7tVP…cACQ` signs a transfer of **14.00 USDC** to the demo maker's USDC account, when
  D5 starts and quotebook gives the address. It keeps 0.23. metalbook needs no USDC from it
  before Nov 13, when 10 to 15 USDC come back. Further transfers on request, recorded the same
  way.
- **After Nov 13**: 7tVP redeems its 2 `B66M` longs and claims as writer. `3ZBj…` redeems its own
  long (quotebook's key). `B66M` then owes nothing and can close.
- **Settlement every week**: depends on R4's agents; until then a keeper is started by hand for
  Nov 13 and Nov 20. Tell quotebook before any week with no keeper, because its writer's
  collateral stays locked until settlement.
- **Record**: the transfer, redemptions and claim in `docs/devnet-log.md`, with a cross-link to
  quotebook's record of the first demo trade (and quotebook PR #23 for I9).
- *Done when* a real trade on a roller-listed series has settled, redeemed and claimed, with
  both logs cross-linked.

## R3. Nov 13 batched measurement (S, fixed date)

Closes P1's *done when* (`docs/PLAN-improvements.md`).

- Keeper started **by hand** from about 20:25Z (the window opens at 20:30Z), batched, Hermes
  12 s, over all 24 series, with the Nov 6 sheet as its model. No agent yet: this run measures
  batching and nothing else. The binary is built from `main` and its commit recorded.
- **Compared with the Nov 6 baseline** (P3): posts, transactions, bytes, CU, wall clock from the
  first post to the last `settle`, SOL spent, leftover encoded VAAs. Nov 6 has 26 series and
  Nov 13 has 24, so the table gives both the totals and the per-series figures; P1's "8×" is
  replaced by what was measured.
- Then R2's redemptions and claim for `B66M`.
- *Done when* `docs/devnet-log.md` has the table and signatures, and P1 in
  `docs/PLAN-improvements.md` carries the measured number.

## R4. Keeper and watcher agents (S)

A weekly trade needs settlement with no human, and R5 needs both keepers supervised.

- `scripts/keeper-agent.sh` and `scripts/watch-agent.sh`, `install|uninstall|status <name>`, the
  shape of varswap's: plists from heredocs, `plutil -lint`, `launchctl bootout` and `bootstrap`;
  `KeepAlive {SuccessfulExit: false}`, `ThrottleInterval` 120; binaries copied out of the
  checkout; logs under `~/.local/share/metalbook-keeper/<name>/`; the Hermes key only through
  `zsh -c 'source ~/.zshrc; exec …'`.
- **Keys** (H3): `metalbook-devnet-keeper.json` and `metalbook-devnet-keeper2.json`, one payer per
  keeper, as the Oct 30 interference showed. Primary at 12 s, second at 30 s; the flag's floor
  of 12 s stays. RPC URLs by environment, never printed.
- **Scope**: every series of the program, `--heartbeat`, batched. The watcher reads the chain and
  the channel `metalbook-keepers`, and holds no key.
- **Tests**: offline, as R1's script test: no secret in any plist, a stale plist is replaced,
  `status` tells loaded from not loaded. `docs/runbooks/keeper-restart.md` gets the agent
  commands.
- *Done when* both agents and the watcher run on the Studio through one idle day, posting
  heartbeats, with their PIDs and commit recorded.

## R5. Nov 20 two-keeper drill (S, fixed date)

Closes P2's *done when*.

- `metalbook-keepers` channel created before the window. The Nov 20 series come from R1.
- From 20:25Z both agents run. **Hermes**: 5 + 2 = 7 a minute, and R1's quiet period keeps the
  roller out.
- **Stage 1, supervised restart**, 20:32Z: `kill <pid>` of the second keeper, by exact PID. launchd
  relaunches it after the throttle. Check the payer lock, the leftover scan and that it rejoins.
- **Stage 2, loss of a keeper**, 20:35Z: stop the primary with `launchctl bootout` of its exact
  label (a plain `kill` would relaunch it), and record its PID before and after. Expected:
  - the second keeper finishes the window and settles every series on the full window, by
    `window_end + D + 10 min` (21:20Z);
  - `metalbook-watch` raises `KeeperSilent` for the primary once `now − last heartbeat >
    1,800 s` on one of its polls (`--interval-s` 60). The bound is therefore
    `last_hb + 1,800 + 60 s + one poll's run time`, not a fixed clock time. The primary posts a
    heartbeat on every state change, and during the window state changes every pass, so
    `last_hb` is within a pass (60 s) of the bootout: with the bootout at 20:35Z the alert is
    expected by about **21:07Z**, and the drill fails this check if it has not come by
    **21:10Z** (`last_hb` + 1,800 + 60 + a 5 min grace, `last_hb` being read from the channel,
    not assumed). The alert must reach the channel and the coordinator. That leaves 10 min
    before the 21:20Z settlement check, and the two checks are independent;
  - no double post, and no failed transaction beyond the benign race in `keeper.rs`.
- Primary reinstalled afterwards, and it reports itself back.
- *Done when* `docs/devnet-log.md` has the times, PIDs, signatures, and alert messages, and the
  per-keeper transaction counts.

## R6. Dashboard section (M; needs varswap's format)

- **Format**: `section.json` version 1, on varswap's `main`: schema
  `docs/dashboard/section-1.schema.json` at `324e161` (#48), shell at `ec30175` (#49), including
  per-table `href_columns` for explorer links. Our URL:
  `https://joaquinbejar.github.io/metalbook/metalbook/section.json`; varswap's `sections.json`
  lists it as `pending` until we publish, and we tell varswap when it is live. The schema is
  vendored at `324e161`; a test checks our output against it with `serde_json` only (no new
  dependency). Two shell rules apply: every `links[].href` is absolute `https://` or a relative
  path with no scheme, no leading `/` or `\`, and no `.` or `..` segment (encoded or before a
  query or fragment), and the shell marks the section `stale` when `generated_at` is older than
  48 h, whatever `status.level` says, which the daily run keeps clear.
- **`metalbook-admin dashboard --out <dir> [--records <dir>]`**: no key, no Hermes request,
  public RPC only (with backoff on 429, `getProgramAccounts` being the heavy read). Pure
  rendering from decoded accounts, so fixtures test it. RFC 3339 is formatted by hand: no new
  dependency. Amounts through `format_usd`, as decimal strings.
- **Content**:
  - **Tiles**: live series per metal, next expiry, last settlement price per metal, contracts
    open, void count, payer SOL.
  - **Series**: settlement price per expiry, XAU and XAG, with `quality` per point.
  - **Table `series`**: the live ones and the most recent 2 expiries (address, metal, kind,
    strike, cap, oz, state, open, slots filled of 30). At most about 60 rows; the rest are behind
    a link to the records.
  - **Table `prints`**: per expiry and metal, slots filled, samples, `window_changed`. This is the
    count and the average, because the per-slot tape is not on chain (see "Facts"), and the
    section says so in a note.
  - **Table `settlements`**: per expiry and metal, price, method, samples, series settled.
  - **Table `voids`**: series, deadline, whether it has been closed.
  - **`quality`**: `full-window`, `partial`, `next-print`, `fallback`, `void`, `from-log`.
  - **`status`**: `warn` when `watch` has a settle-overdue or void-pending series; `stale` when
    there is no live series for the next expiry while the market is open.
- **Records** (r2, codex): since `reclaim` deletes accounts, every settled or voided series gets
  an append-only **outcome record**, `records/<expiry>/<series>.json`. It holds only fields that
  can no longer change once the series is settled or voided, never the whole `series_json`:
  - **terms** (`show-series` names): `address`, `underlying`, `kind`, `strike_usd`, `cap_usd`,
    `multiplier_milli_oz`, `expiry`, `long_mint`, `vault`, and the rent payer from the decoded
    account;
  - **outcome**: `state` (`settled` or `voided`), `settle_method`, `settlement_price_usd`,
    `settlement_samples`, `payout_atoms`, and `filled_slots` and `sample_sum` from the decoded
    account (`series_json` does not print them today);
  - **provenance**: the RPC slot and time of the read, and the generator's commit.

  `open`, `longs_outstanding`, `writers_unclaimed`, `vault_owed_atoms` and `residual_atoms`
  can keep changing after
  settlement (each `redeem` and `claim_writer`), so they are excluded. When the ledger at close
  is wanted, the closing job writes a **separate**, equally append-only
  `records/<expiry>/<series>.final.json` (the ledger read just before the close, which is all
  zeros by construction, plus slot and signature), and never touches the outcome record.
  - A run that would change an existing record fails with no deploy, as varswap's.
  - **Closing depends on publication**: `reclaim --published-records <base-url>` (R1's step 8)
    and any automated `close-series` (the roller, a script, a workflow) close a series only if
    its published outcome record exists and its outcome and terms equal the chain's, compared
    field by field on that stable subset (never the whole series). A missing, different or
    unreachable record means no close, which costs only the rent in the meantime. A manual
    `reclaim` or `close-series` without the flag keeps today's behaviour and is explicitly
    outside this guarantee; the runbooks say so.
  - Oct 30's 23 closed series: seeded once from `docs/devnet-log.md`, `quality: from-log`.
  - **The Nov 6 sheet saves `show-series` output for each void series before `close-series`**,
    since those are closed before R6 exists; R6 derives their outcome records from those files
    (provenance: the saved read, `quality: void`). And **no `reclaim` runs from Nov 6 until
    records are published**.
- **Publication**: `.github/workflows/dashboard.yml` on GitHub-hosted runners, daily at 22:30Z
  (after Friday's settlement) and by hand, with Pages actions pinned by SHA and permissions
  `pages: write`, `id-token: write`, `contents: read`; no secret. It restores the published
  records, generates and deploys `metalbook/section.json` and `records/`.
  It adds no required check. Monthly, a normal PR copies new records to `docs/settlements/`.
- **Fallback, Pages down**: the command also works locally; nothing closes, per the gate above.
- **Tests**: fixtures for each state and each `quality`; schema validation; a closed series is
  still shown from its record; a changed record fails; each gate case for `reclaim` and the
  automated `close-series` (published and equal, missing, different, unreachable). **Lifecycle
  test**: write the outcome record at settlement, then `redeem` and `claim_writer` (the ledger
  fields change), then reclaim: the gate passes, the outcome record is byte-identical to the
  one written at settlement, and only `.final.json` is new. `make dashboard-check` joins `pre-push` and
  `ci.yml`'s want-list.
- *Done when* the workflow has run on schedule twice, varswap's shell renders our section from
  its Pages URL, one reclaim has closed a series only after its record was published, and the
  section's numbers equal `show-series` on the same slot.

## R7. Phase report (S)

`docs/devnet-log.md` brought up to date, `docs/mainnet-readiness.md` updated for anything this
phase found, and the final report to SOLANA-PROJECTS with the open items.

## Order and dates

| When | What | Blocks |
| --- | --- | --- |
| Nov 6 | The drill (`docs/PLAN-improvements.md`, P3), with the void `show-series` saved first | Everything below |
| Nov 7 to 9 | optfix 0.3 bump, H3 part 2 (the keys R1 and R4 use) | R1, R4 |
| Nov 9 to 12 | R1, installed with `--min-expiry` Nov 20 | Nov 20 series |
| On request | R2 transfer, when D5 starts | quotebook D5 |
| **Nov 13, 20:25Z** | R3 | P1 closed |
| By Nov 19 | R4 | R5 |
| **Nov 20, 20:25Z** | R5 | P2 closed |
| From Nov 16 | R6 once varswap's format has both ACKs | Reclaim of settled series |
| Before Dec 4 | Calendar extended by hand | Roller past Dec 18 |
| After R2, R5, R6 | R7 | |

Reclaim is held until R6 publishes: the cost is the rent of about 24 series a week, around 0.11
SOL at the Oct 30 rate (23 series returned 0.109), which is small against the payer's balance.

## Questions for SOLANA-PROJECTS

1. **Calendar after Dec 18**: Fridays 16:00 New York, skipping Dec 25 and Jan 1 as Nov 27 was
   skipped, through the end of March 2027? (Proposed; I proceed on this unless told otherwise.)
2. **GitHub Pages on `metalbook`**: I assume varswap's decision (bus 51643) applies to this repo
   too, and I would enable Pages in its settings at R6.

## Not in this plan

Any on-chain change; XAU or put trading for the demo (a funding question for quotebook's D5); a
per-slot print tape, which would need a keeper log export; a second host for the keepers, still a
pre-mainnet gate in `docs/mainnet-readiness.md`; not sampling unwritten series to save SOL, to be
reconsidered only with R3's measured cost.
