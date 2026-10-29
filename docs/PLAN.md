# metalbook: plan

Status: approved by Joaquin on 2026-10-29. Changes after approval are recorded as decisions.

## 1. Goal and non-goals

### Goal

Cash-settled European calls and puts on gold (XAU/USD) and silver (XAG/USD) on Solana,
settled in USDC from Pyth at expiry. It integrates the three earlier projects:

- **optfix** for marks and for the writer collateral checks that need a price.
- **quotebook** for trading: an option token / USDC pair is a quotebook series.
- **varswap's `pyth-lite`** and sampling patterns for the oracle and the settlement window.
- Optionally a **MatchBook** market per option token for a public order book.

metalbook itself owns option series, the long-position mints, writer collateral vaults and
the expiry flow.

### Non-goals (v1)

- Partial collateral, portfolio margin, liquidations: v1 is fully collateralized.
- American exercise, physical delivery, other underlyings.
- An on-chain volatility surface. Marks use a volatility set per series by a keeper (open
  question 4), never used for settlement.

## 2. Domain model

`metalbook-core` (`no_std`, integers only). It does not depend on optfix: the payoff and
collateral math are exact integer formulas in USDC atoms, and optfix enters only for marks
(milestone 5). Decided at the start of M1 (coordinator, 2026-10-29).

```rust
#[repr(u8)] pub enum Underlying { Xau = 0, Xag = 1 }
#[repr(u8)] pub enum OptionKind { Call = 0, Put = 1 }   // own type, not optfix's
pub struct UsdcAtoms(pub u64);               // 6 decimals
pub struct StrikeUsd(pub u64);               // strike in USDC atoms per troy ounce
pub struct Contracts(pub u64);               // number of option tokens (decimals 0)
pub struct OuncesPerContract(pub u32);       // multiplier in milli-ounces: 1_000 = 1 oz
pub struct UnixTimestamp(pub i64);

pub struct SeriesKey { pub underlying: Underlying, pub kind: OptionKind,
                       pub strike: StrikeUsd, pub expiry: UnixTimestamp }

pub struct SeriesTerms {
    pub key: SeriesKey,
    pub multiplier: OuncesPerContract,
    pub cap: StrikeUsd,           // calls: payoff per ounce is min(S − K, cap − K), cap > K;
                                  // puts: must be 0, any other value is rejected. See 4.1.
}
// cap and multiplier are part of the Series PDA seeds, so two series that differ only in
// cap or multiplier never collide, and a later Config change cannot alias a live series.

#[repr(u8)]
pub enum SeriesState { Active, Expired /* past expiry, awaiting price */, Settled, Voided }

pub struct SettlementPrice { pub price: StrikeUsd, pub samples: u16, pub method: SettleMethod }
#[repr(u8)] pub enum SettleMethod { Window = 0, NextPrint = 1, PartialNextPrint = 2,
                                   PartialWindow = 3, Fallback = 4, Void = 5 }
```

Invariants:

- Per series: `vault_balance >= total_written × max_payout_per_contract` at all times before
  settlement; after settlement `vault_balance >= unredeemed_longs × payout + writers' residual`.
- `long_mint.supply == Σ writer.written − burned by close` (each long token is backed by one
  written contract).
- `max_payout_per_contract`: put `K × mult`, capped call `(cap − K) × mult`, both in USDC atoms,
  rounded up (toward the protocol: more collateral).
- Payout per contract after settlement rounded down (toward the protocol, against the long);
  the writer's residual is the remainder, so the vault empties exactly when everyone claims.

```rust
#[repr(u32)]
pub enum MetalbookError {
    InvalidInstruction = 100, InvalidInstructionData, MissingSignature, InvalidAccountOwner,
    InvalidPda, InvalidMint, InvalidVault, InvalidSysvar, NotAuthority,
    SeriesNotListed, ExpiryNotInCalendar, StrikeOffGrid, InvalidCap,
    SeriesExpired, SeriesNotExpired, SeriesNotSettled, SeriesAlreadySettled,
    InsufficientCollateral, InsufficientLongTokens,
    OracleOwnerMismatch, OracleNotFullyVerified, OracleFeedMismatch, OracleStale,
    OracleConfidenceTooWide, SampleOutsideWindow, NotEnoughSamples,
    Overflow, Pricing /* optfix error mapped */,
}
```

## 3. Accounts and instructions

### 3.1 Accounts

| Account | Seeds | Owner | Size | Written by |
| --- | --- | --- | --- | --- |
| `Config` | `["config"]` | metalbook | ~200 | admin: USDC mint, feed ids, oracle params, keeper key |
| `Calendar` | `["calendar", underlying]` | metalbook | 8 + 8 × 64 | admin: allowed expiry timestamps |
| `Series` | `["series", underlying, kind, strike_le, expiry_le, cap_le, multiplier_le]` | metalbook | ~200 | `create_series`, `mint`, `close`, sampling, `settle` |
| Long mint | `["long", series]` | SPL Token | 82 | mint authority = series PDA |
| Collateral vault | ATA(series PDA, USDC) | SPL Token | 165 | token CPIs |
| `Writer` | `["writer", series, owner]` | metalbook | ~80 | `mint`, `close`, `claim_writer` |

```rust
struct Series {
    hdr: Hdr, terms: SeriesTerms, state: SeriesState,
    long_mint: Address, vault: Address,
    total_written: u64, total_redeemed: u64,
    // settlement accumulator (same rules as varswap sampling)
    sample_sum: u128, filled_slots: u32,   // bitmap of the 30 window slots (4.2)
    next_print: [i64; 5], next_print_px: [u64; 5],   // canonical NextPrint chain (4.3 step 2)
    fallback_time: i64, fallback_px: u64,            // largest publish_time <= expiry seen
    window_changed: i64, next_print_changed: i64, fallback_changed: i64, // challenge delay D (4.3)
    settlement: SettlementPrice,
    mark_vol: Fixed, mark_vol_time: i64,   // keeper-set, display and quoting only
}
struct Writer { hdr: Hdr, series: Address, owner: Address, written: u64, claimed: u8 }
```

### 3.2 Instructions

| # | Instruction | Signers | Purpose |
| --- | --- | --- | --- |
| 0 | `init_config` / `set_config` | admin | parameters |
| 1 | `set_calendar` | admin | allowed expiries per metal |
| 2 | `create_series` | anyone (payer) | expiry must be in the calendar, strike on the grid |
| 3 | `mint` | writer | deposit `n × max_payout` USDC, receive `n` long tokens |
| 4 | `close` | writer | before expiry: burn `n` long tokens held by the writer, withdraw collateral |
| 5 | `sample_settlement` | anyone | add a Pyth print as a window slot, chain link or fallback (tag 6) |
| 6 | `settle` | anyone | after the window: fix the settlement price, or void (4.3) (tag 7) |
| 7 | `redeem` | long holder | burn long tokens, receive `payout × n` (tag 8) |
| 8 | `claim_writer` | writer | receive `written × (max_payout − payout)` (tag 9) |
| 9 | `set_mark_vol` | keeper | update `mark_vol` (no effect on settlement) |
| 10 | `void_series` | | folded into `settle` (milestone 3) |

Quotebook integration: a series' long mint and USDC form a quotebook series (`create_series`
on quotebook, by the admin). The maker service prices through a payoff-aware adapter: a put is
one optfix put; a capped call is a call spread `C(K) − C(cap)`, value and every greek, scaled
by `multiplier / 1000`, both error bounds combined, converted to USDC ticks with directed
rounding. A vanilla call would overvalue the token at high spot (above the maximum payout).
The settlement-window mean makes the European mark an approximation; documented. Tested:
spot above cap, non-unit multipliers, value never above the maximum payout. Collateral never
depends on a mark: always the contractual maximum payout.
MatchBook: optional `create_market` with the long mint as base, done off-chain by the admin.

### 3.3 Security checks

- `init_config`: only the program's upgrade authority. The instruction takes the program's
  `ProgramData` account, checks that it is owned by the upgradeable loader and sits at the
  PDA of `[program_id]` under that loader, and requires its `upgrade_authority` to be present
  and to be the signing admin, so nobody can front-run the deployment. Negative tests with
  forged `ProgramData`: wrong owner, wrong address, a different authority, no authority.
  The LiteSVM harness loads the program through the upgradeable loader.
- `create_series`: series PDA from the key; expiry present in the `Calendar` PDA for that
  underlying and in the future by at least `min_listing_time`; strike positive and on the
  configured grid; capped call: `cap > strike`; long mint PDA created with series PDA as mint
  authority, no freeze authority; vault is the ATA of the series PDA with `Config.usdc_mint`.
- `mint` / `close`: writer signs; series `Active` and `now < expiry`; vault and long mint match
  the series; writer's USDC account mint is USDC and owned by the writer; `Writer` PDA from
  (series, writer); collateral computed on-chain with directed rounding; `close` burns from an
  account owned by the writer and cannot exceed `writer.written`.
- `sample_settlement`: price update owned by the receiver program id in the series snapshot,
  `Full`, `feed_id` equals the series' snapshot feed, confidence within bound, and the print fits one of three
  slots: a window slot `i` (fixed `t_i = window_start + i × 60 s`, canonical print per 4.2,
  `slot_tolerance_s` 5 to 10 s, slot not yet filled); the next link of the canonical
  NextPrint chain (4.3 step 2); or the
  fallback candidate (`expiry − X <= publish_time <= expiry`, replacing it if later). The first call after
  `expiry` moves the series from `Active` to `Expired`; `mint` and `close` also check the
  clock, so the transition being lazy never lets a mint through after expiry.
- `settle`: the per-step earliest times and the challenge delay `D` of 4.3; idempotent
  (second call fails). Test: sampling and settling in one transaction fails.
- `redeem` / `claim_writer`: series `Settled`; burn from the holder's account; destination
  owned by the signer; writer claims once.
- Forged-account negative test for every rule above.

## 4. Key design decisions

### 4.1 Full collateral for calls

A cash-settled call has unbounded payoff, so "fully collateralized" needs a bound. Options:

- (a) **Capped call** (economically a call spread `K / cap`): collateral `(cap − K) × mult`.
- (b) Collateral in tokenized gold (e.g. a gold-backed SPL token), payoff converted at
  settlement: adds an asset and its own oracle and redemption risk.
- (c) Partial collateral with liquidation: out of v1 scope.

**Recommended: (a).** Puts are naturally bounded by `K`. The UI presents a capped call as what
it is. Cap per series on a grid of `K × (1 + x%)` (for example 10%, 25%, 50%): a cap of `2K` would
make a writer post about one spot price of collateral per ounce, while a tighter cap is
cheaper and plainly a call spread.

### 4.2 Settlement price capture

Options: single print (first update with `publish_time >= expiry`): cheap, easy to game with a
selective push; **window average**: mean of grid-sampled prints over `[expiry − W, expiry]`
(proposed `W = 30 min`, one slot per minute); TWAP from Pyth's EMA: not a defined window,
opaque. **Recommended: arithmetic mean of window samples**, requiring at least `min_samples`
(proposed 20 of 30).

Each slot takes the **canonical** print for its boundary `t_i`: `prev_publish_time < t_i <=
publish_time <= t_i + slot_tolerance_s`, the same rule as varswap 4.1. The upper bound
matters: without it the first print after a halt would be canonical for every slot in the
gap and could reach `min_samples` alone (test: a print after a 20 min gap fills at most one
slot). A pusher therefore has no choice of price within a slot, and late backfill from
Hermes is safe. Same-second prints: see varswap 4.1's milestone 2 checks, which metalbook
reuses.

Uniqueness gate (milestone 3). Pyth documents the rule (`pythnet_sdk/src/messages.rs`,
`PriceFeedMessage::prev_publish_time`) and two exceptions: an update Pythnet never sent leaves
a moment with no canonical update (liveness, not choice), and failed aggregations and later
aggregates of the same second repeat `prev_publish_time == publish_time`, an empty interval
that never matches. varswap's milestone 2 proved uniqueness from the aggregation source
(varswap `docs/PLAN.md` 4.1, verified facts "Canonical print"); metalbook relies on that proof
and adds `prev_publish_time > 0` (the first aggregate after a feed (re)initialization has
`0`, and its interval would overlap earlier prints). Milestone 3 tests it on synthetic Pyth
tapes (`metalbook-core/tests/settlement_props.rs`: same-second repeats, failed aggregations,
halts of 30 s to 15 min): the candidates are computed independently from their definitions,
posting the whole tape in any order gives exactly them, and posting any subset in any order
never stores anything else. So no first-write-wins fallback is needed. Mainnet tapes replace
the synthetic ones once a Hermes key exists (verified facts). LiteSVM tests also run one real
`PriceUpdateV2` (XAU/USD, captured by varswap) through `sample_settlement`.

### 4.3 Missing or stale oracle at expiry

The program cannot tell that a print was "the first" or "the last" one, so every candidate
is monotonically improvable until finalization: anyone may replace a stored candidate with a
better one (3.3). Improvement needs time, so every step that can settle on a subset waits a
challenge delay `D` (proposed 10 min) after the last change to its own candidates (one
timestamp per kind: `window_changed`, `next_print_changed`, `fallback_changed`, so a stream
of later fallback prints cannot delay a Window or NextPrint settle); otherwise an attacker could backfill a favorable
subset from Hermes and call `settle` in the same transaction. Each step also has its own
earliest settle time, so a short halt waits for the market to reopen instead of settling at
a partial or stale price:

1. `Window`: immediately from `window_end` if all 30 slots are filled; otherwise from
   `max(window_end, window_changed) + D` with at least `min_samples` slots: mean of
   the window. (Backfilled slots can still arrive after `window_end`, see 4.2.)
2. `NextPrint`: canonical and chained, so there is no choice of price. A complete chain
   settles from `window_end + D` (decided in milestone 3): canonical next prints exist
   seconds after `window_end`, so without the delay anyone could settle on the chain before
   keepers backfill the window, skipping step 1. The "next print" after a time `t` is the canonical update that strictly
   advances past it: `prev_publish_time <= t < publish_time`. Candidate 1 is the next print
   after `window_end`; candidate `j + 1` is the next print after `publish_time(candidate j)`,
   so it must satisfy both `prev_publish_time == publish_time(candidate j)` and
   `publish_time > publish_time(candidate j)`. The second condition is required: Pyth allows
   `prev_publish_time == publish_time` for an unsuccessful aggregation (see 4.2), and without
   it one such non-advancing update would match the next link again and again, fill all `k`
   and settle at once. Every stored link updates `next_print_changed`. Once `k = 5` are
   stored: their mean. If Pyth skips sending an update (4.2), the chain stalls and step 3
   applies. Planned tests: replaying the same non-advancing print (`prev_publish_time ==
   publish_time == publish_time(candidate j)`) is rejected and the chain stays at `j`; two
   distinct messages with the same `publish_time` and different prices, submitted in both
   orders, extend the chain only with the one whose `prev_publish_time` equals the last link.
3. From the hard deadline only (proposed 7 days after expiry), each with its own delay: a
   partial `NextPrint` chain with 1 to `k − 1` links (their mean) from
   `max(hard_deadline, next_print_changed) + D`, so honest keepers can still extend a chain
   someone stopped early; else the partial window mean, if at least one slot is filled, from
   `max(hard_deadline, window_changed) + D`.
4. `Fallback`, from `max(hard_deadline, fallback_changed) + D`: the latest submitted print in `[expiry − X, expiry]`
   (proposed `X = 24 h`).
5. Void, from `hard_deadline + D`, only if all of these are empty: collateral goes back to
   writers, since longs have no price. Never void silently: the method used is stored.

Trust assumption (codex, on #9). Canonicity removes the choice of price for each slot, link
and fallback, but not the choice of *which* candidates exist when `settle` runs: with no one
else posting, an adversary can post a favorable subset of the window (at least
`min_samples`, or any subset after the hard deadline), a partial chain, or an older fallback
(say `expiry − 3000` at 1000 while `expiry − 2000` at 2000 exists), wait `D` and settle on
it. Every candidate is monotonically improvable, and an improvement to a candidate that can
settle on a subset restarts that candidate's `D`: more slots for a window that is not yet
complete, the next link for a partial chain, a later print for the fallback. Two cases do not
wait on a restarted `D`, because they leave no subset to choose: a **complete window** (every
slot filled) settles at once from `window_end`, and a **complete chain** (`k` links) settles
from `window_end + D` whenever its last link arrived. The protocol is correct under this
assumption: **at least one honest keeper submits, within `D` of any change, every available
candidate of higher priority than the one that would otherwise settle**: every window slot
that has a canonical print, every next-print link that exists, and the latest fallback.
Posting only one kind is not enough: the latest fallback does not protect against an
attacker's partial-window mean, which has priority over it, and during an outage a complete
window may not exist, so the keeper posts the slots and links that do. The default honest
keeper is the protocol-run keeper of milestone 7, which posts every canonical print from
Hermes as soon as it exists; anyone else may run one. Tested: a later fallback and extra
window slots posted during `D` replace the worse candidate and restart `D`, and completing
the window settles at once.

Priority (codex, after #3): a candidate that exists but is still inside its delay is
pending, not absent, so `settle` returns `SettlementNotReady` instead of falling through to a
lower step. The fallback is the latest *first-of-its-second* print (`prev_publish_time <
publish_time`) in `[expiry − X, expiry]`: later aggregates of the same second repeat
`prev == publish`, so this leaves no same-second choice.

### 4.4 Metals market hours

Verified: Pyth's XAU/USD and XAG/USD feeds carry a schedule (America/New_York): Monday to
Thursday open except 17:00 to 18:00, Friday until 17:00, closed Saturday, opens Sunday 18:00,
plus holiday closures and early ends. Prices do not update while closed. Consequences:

- **Expiries** come only from an admin-maintained `Calendar`, set to times when the market is
  open with margin on both sides of the window (proposed: Wednesdays and the last Friday of
  the month at 15:00 New York, never on early-close days). The program cannot know holidays,
  so the calendar is the control; `create_series` rejects anything else.
- **Settlement window** inside trading hours by construction; the 4.3 fallback covers an
  unexpected halt.
- **Marks** (display, maker quoting) use the last print with its age shown; while the market
  is closed, `time to expiry` keeps running in calendar time (optfix `Years::from_seconds`),
  and the maker service widens or stops quoting. Because v1 is fully collateralized, no
  on-chain safety depends on a mark, so a closed market never blocks minting, closing,
  redeeming or claiming.

### 4.5 Live series are immutable

`set_calendar`, strike and cap grid changes, and oracle parameter changes in `Config` apply
only to series created afterwards: every term a live series needs is copied into it at
creation, and every instruction reads only the copy:

```rust
struct SeriesSnapshot {
    usdc_mint: Address, token_program: Address,
    receiver_program: Address, feed_id: [u8; 32],
    max_conf_bps: u16, slot_tolerance_s: u16, min_samples: u16, k_next_print: u8,
    expo: i8,                         // pinned Pyth exponent; a mismatch is rejected
    window_s: u32, slot_s: u32, challenge_delay_s: u32, fallback_lookback_s: u32,
    hard_deadline_s: u32, redeem_deadline_s: u32,
}
```

Validated at `create_series`: `slot_s` divides `window_s`, `window_s / slot_s <= 32` (u32
bitmap), `min_samples <= slots`, `k_next_print <= 5` (array size), `slot_tolerance_s <
slot_s`. `sample_sum` adds prices in normalized units (USDC atoms per ounce) using the pinned
`expo`.

Tested: change each `Config` field mid-life and check sampling, settlement, redeem and claim
are unchanged.

### 4.6 Unredeemed longs and unclaimed residuals (open question 7, decided)

**Decided (Joaquin, 2026-10-29): claimable forever.** There is no deadline and no sweep:
`redeem` and `claim_writer` stay open indefinitely, and no instruction ever moves USDC to
anyone but the holder or writer it is owed to. Once nothing is owed, anyone may close the
accounts and return their rent to whoever paid it (implemented after milestone 4: tags 10
and 11, `metalbook-program/src/processor/close.rs`):

- `close_series`: the series account and its USDC vault, when the series is `Settled` or
  `Voided` and its ledger owes nothing: every long token redeemed (after a void too, for
  zero, so `redeem` accepts a voided series), every writer position claimed, owed vault 0.
  Owed amounts come from the ledger, never from the token balance. Any balance the vault
  still holds was never owed to anyone (a donation): it goes to the payer's USDC account
  first, so a one-atom transfer cannot block the close. Rent goes to the `create_series`
  payer, recorded in the series (layout version 3, which also drops `redeem_deadline_s`).
- `close_position`: a writer position, once claimed (or with nothing written); rent goes to
  the writer. It does not read the series, so it works after `close_series`.

Tests (`metalbook-tests/tests/close.rs`, 8 LiteSVM tests): rent returned exactly, a one-atom
donation goes to the payer, close rejected with one unredeemed long, with an unclaimed writer
and before settlement, a voided series closing once its longs burn for zero, forged and
wrong payer, payer USDC account, vault, series, mint and program, read-only accounts, and
`close_position` before and after its claim, with a wrong rent destination and forged
positions.

The long mint cannot be closed: SPL Token (unlike Token-2022's mint close authority) has no
instruction to close a mint, so its rent (about 0.0015 SOL) stays locked. Out-of-the-money
long tokens may remain in wallets after the series is closed; they were worth nothing.
Options considered and rejected: a sweep to a treasury after a deadline (a privileged
destination for user money, late users lose their payout, the protocol looks custodial); a
sweep with claim-back (the most complex, still a privileged key); giving unredeemed long
payouts to writers (moves value from holders to writers, and can be gamed). The snapshot
field `redeem_deadline_s` is unused and goes away in the next layout version.

### 4.7 Quoting during the settlement window

From `window_start` part of the settlement average is known, so the maker service stops
quoting (or widens to the known part's bounds) at `window_start`. Listed in the maker config.

### 4.8 Contract size and units

Multiplier per series in milli-ounces (proposed 1 oz for XAU, 50 oz for XAG at typical
prices). Long token decimals 0. Strikes in USDC atoms per ounce on a configured grid.

## 5. Milestones

1. **Domain and payoff math.** *Done when* `metalbook-core` proptests show collateral ≥ max
   payout and exact vault conservation across random settlement prices and claim orders,
   and the Cargo workspace enforces the shared lint control (same as optfix):
   - `lint-control/` (`Cargo.toml`, `src/lib.rs`, `check.sh`) copied from optfix and listed in
     the workspace members;
   - `cfg(lint_control)` added to `check-cfg`;
   - `disallowed_macros = "deny"` in `[workspace.lints.clippy]`, backed by the shared
     `clippy.toml`;
   - the lint allowed explicitly in tests, benches and the entrypoint module (Pinocchio's
     `no_allocator!` expands to `assert!`);
   - `lint-control` run by `make pre-push`.

   Delivered (M1): `metalbook-core` with the unit newtypes, `SeriesKey` and `SeriesTerms`
   (cap rule and a representable maximum payout checked at construction), the payoff and
   collateral formulas on `SeriesTerms`, the vault accounting `SeriesLedger`
   (mint, close, expire, settle, redeem, claim; every call atomic, exact identity checked by
   `check_invariants`) and the library error `CoreError` (`#[repr(u8)]`, mapped into
   `MetalbookError` in M2). Rounding: collateral per contract is the exact maximum payoff
   rounded up, the payout the exact payoff rounded down, both within one atom, and the
   writer's residual is the difference, so the writer owns the dust. Property tests
   (`tests/payoff_props.rs`, `tests/ledger_props.rs`, fixed seeds) check these bounds against
   an independent `u128` reference over the whole `u64` domain, and that random mint/close
   histories, settlement prices and claim orders empty the vault exactly.
2. **Series, mint, close.** *Done when* LiteSVM tests and a negative test per rule in 3.3 pass.

   Delivered (M2): `metalbook-program` (Pinocchio 0.11, `cdylib`) with `init_config` (tag 0),
   `set_config` (1), `set_calendar` (2), `create_series` (3), `mint` (4) and `close` (5);
   instruction data is the tag then little-endian fields, documented on each handler. Account
   layouts (`#[repr(C)]`, `Pod`, tag and version bytes first): `Config` 144 bytes, `Calendar`
   520, `Series` 216, `Writer` 80; milestone 3 extends `Series` with the settlement fields and
   bumps the version. Decisions made while implementing:
   - `init_config` is accepted only from the program's upgrade authority (read from its
     `ProgramData`), so nobody can front-run the deployment and take the admin role.
     `set_config` can rotate the admin; the USDC mint and token program never change.
   - Token program: SPL Token only (Token-2022 is rejected), copied into each series.
   - Cap grid: `Config.cap_bps` (up to 4 offsets, at most 60_000 bps); a call's cap must
     satisfy `cap × 10_000 == K × (10_000 + bps)` exactly. Strike grid: one tick per metal.
     Multiplier: one value per metal. Listing: `expiry >= now + min_listing_s`.
   - Canonical bumps: every PDA is found with `find_program_address` when it is created and
     its bump stored; later instructions re-derive it with the stored bump (`sol_sha256`).
   - A pre-funded PDA (someone sent lamports to it first) is topped up, allocated and
     assigned instead of `CreateAccount`, so creation cannot be blocked.
   - The vault is created with the associated token program's `CreateIdempotent` (anyone can
     create a valid associated account for the series first) and then checked: SPL Token,
     USDC mint, owner the series, initialized, no delegate, no close authority (codex
     review of #7).
   - Collateral, payout and the vault accounting come from `metalbook-core`
     (`SeriesLedger::restore` validates the stored fields on every load); state is written
     before every CPI. `close` is limited to the writer's own `written`, even if the writer
     holds more long tokens.
   - SBF stack: handlers are `#[inline(never)]`, and `make program` fails if
     `cargo-build-sbf` fails or reports a frame above 4 KiB (it only warns, and the overflow
     silently corrupts locals; found while building M2). `make program-control`, part of
     `make pre-push`, proves both failure paths.
   Tests (`metalbook-tests/tests/program.rs`, now 33 LiteSVM tests): happy paths for puts and
   capped calls on both metals, and negative tests for every rule of 3.3 that applies to
   these instructions, with forged accounts (genuine bytes under another owner or at another
   address) for the config, program data, calendar, series, positions, USDC mint and token
   accounts. CU per instruction: `docs/benchmarks.md` (`make bench`, deterministic keys).
3. **Settlement.** Sampling, window mean, next print, fallback/void. *Done when* tests replay
   XAU fixtures captured from mainnet including a closed-market gap, and every branch of 4.3
   is covered.

   Delivered (M3), with synthetic tapes until a Hermes key exists (the mainnet tape replay
   is the remaining part of this criterion):
   - `metalbook-core::settlement`: `OracleParams` (validated), `Print`
     (`prev_publish_time > 0`), `SettlementState::add_print` (canonical slot, chain link,
     fallback; a print may be several at once), `decide` (the cascade above), and `to_atoms`
     (`sum × 10^expo / count` in USDC atoms, one rounding to nearest, ties up: the fixing is
     neutral between longs and writers, and the payout derived from it is still rounded down).
     `SeriesLedger::void` returns all collateral to writers.
   - Program: `sample_settlement` (tag 6; accounts `[series, price_update]`) decodes the
     `PriceUpdateV2` with varswap's `pyth-lite` (git, rev `5cf95aa`) and checks owner =
     snapshot receiver, `Full`, feed id, pinned exponent, positive price and confidence; no
     staleness bound (canonical prints can be backfilled). `settle` (tag 7; `[series]`) runs
     the cascade and also performs the void, so the planned `void_series` (10) is not a
     separate instruction. The first `sample_settlement` or `settle` at or after expiry moves
     `Active` to `Expired`.
   - `Config` (280 bytes) gains the oracle terms: receiver program, feed id and pinned
     exponent per metal (XAU −3, XAG −5, read from mainnet accounts), `max_conf_bps`, slot
     tolerance, `min_samples`, `k`, window, slot, `D`, fallback lookback, hard deadline,
     redeem deadline. They are validated through `OracleParams` when set, and every
     `create_series` copies them into the series (`Series`, 472 bytes, layout version 2), so
     live series never read the config again (tested by changing `min_samples`, `D` and the
     feed id mid-life).
   - Writer positions now check their owner explicitly (codex note on #7).
   - Tests: core unit tests and the synthetic-tape properties above; 12 LiteSVM settlement
     tests (full window, `min_samples` with `D`, sample-and-settle in one transaction, slot
     rules and a 20-minute gap, canonical vs same-second repeat in both orders, the chain with
     non-advancing and equal-time messages, pending window blocking the chain, every
     hard-deadline step and the void, every oracle rejection with forged updates, the
     snapshot, the real XAU update, the writer owner check).
4. **Redeem and claim.** *Done when* end-to-end ITM, ATM, OTM for both kinds and both metals
   empty the vault exactly.

   Delivered (M4): `redeem` (tag 8; `[holder, series, holder_long, holder_usdc, vault,
   usdc_mint, long_mint, token_program]`, data `contracts: u64`) burns the holder's own long
   tokens and pays `n × payout` to the holder's own USDC account, on a `Settled` series (and,
   since the close-when-empty item, on a `Voided` one for zero, see 4.6). `claim_writer` (tag 9; `[writer, series,
   position, writer_usdc, vault, usdc_mint, token_program]`) pays `written × (max_payout −
   payout)` once per position (`AlreadyClaimed`, 144), on a `Settled` or `Voided` series, so
   a void returns every writer's whole collateral. State is written before each CPI; a zero
   amount skips the transfer. Unclaimed funds stay claimable forever (4.6, decided). Tests (`metalbook-tests/tests/claims.rs`, 9 LiteSVM
   tests): end to end for put and capped call on XAU and XAG, in, at and out of the money
   and a call above its cap, with two writers (one partly closed), long tokens moved to two
   holders, redeems in chunks and claims before or after them: the vault ends at exactly 0,
   each actor receives exactly its share, and the totals equal the deposits. A 1.333-oz
   multiplier and a mean of 3995.000033 USD show the writer keeping the rounding dust. Every
   account rule has a negative test with forged accounts (series, positions, token
   accounts), plus wrong vault, mints and program, read-only accounts, unsigned signers,
   double claims, over-redeeming and claims before settlement.
5. **Pricing and quoting.** optfix marks, quotebook series, maker config. *Done when* a LiteSVM
   demo quotes and settles an option token through quotebook at an optfix price.

   Split (coordinator, 2026-10-29): the LiteSVM demo waits for quotebook-sdk (quotebook
   milestone 5) and the optfix pricer. Delivered now (M5a): `metalbook-maker` (off-chain,
   `rust_decimal`), the payoff-aware adapter of 3.2 behind a `VanillaPricer` trait:
   - `mark`: a put is one vanilla put; a capped call is `C(K) − C(cap)` for value and every
     greek; the error bound is the **sum** of both legs' bounds (tested where the legs'
     errors cancel); everything is scaled by `multiplier / 1000`; time to expiry is 365-day
     calendar time; the value is clamped to `[0, max payout]`, and a pricer whose value leaves
     that range by more than its own bound is rejected; no marks while the market is closed
     or from `window_start` (4.4, 4.7);
   - `quote_atoms`: bid `value − error − half_spread` rounded down to the tick, ask
     `value + error + half_spread` rounded up, the ask never above the maximum payout;
   - `quote_terms`: quotebook `QuoteTerms` for a long token from `quotebook-core` types only
     (git, rev `269e3d9`), the price in quote lots, exact, or `OffLotGrid`.
   Tests use OptionStratLib 0.23 (dev-only) as the independent Black-Scholes reference:
   put and call-spread marks equal the reference legs, the spread stays within its payoff
   bounds and is monotone in spot (property test), and deep in the money it approaches the
   discounted maximum. optfix becomes one more `VanillaPricer`.

   M5b (quotebook side): `metalbook-tests/tests/quotebook_demo.rs` runs the whole path in
   LiteSVM with quotebook-sdk and quotebook-core pinned at `578314c` (quotebook's M5 taker
   SDK; `make quotebook-program` builds its program from the same checkout). A writer mints 10
   long tokens and acts as maker; the long mint / USDC quotebook series has one long token
   per base lot and a 0.01 USD tick; the maker marks the put with `metalbook-maker`, asks
   with `quote_atoms` and `quote_terms`, signs; a taker buys 2 through the ed25519 precompile
   and `settle`, every quotebook instruction built by the SDK; the series settles from Pyth
   prints and the taker redeems 100 USD per token; the vault empties. **Remaining for the
   done-when:** the price comes from OptionStratLib as a stand-in `VanillaPricer` because
   optfix has no Black-Scholes kernel yet (optfix main is at its M3, exp and ln); swapping
   in optfix is one `VanillaPricer` implementation.
6. **Benchmarks and docs.** *Done when* `docs/benchmarks.md` lists CU for every instruction and
   README explains the capped call and the market-hours rules.

   Delivered (M6, for milestones 1 to 4; milestone 5 adds its own rows): `docs/benchmarks.md`
   lists every instruction, including each settlement path; the README covers the product
   and the capped call, the architecture, accounts, instructions, lifecycle, the settlement
   cascade with its trust assumption (4.3), the metals market-hours rules (4.4), the error
   code table and the make targets. Open question 7 was assessed in 4.6 and decided.
7. **Devnet.** *Done when* a full series lifecycle runs on devnet against live Pyth XAU.

   Runbook (not executed): [`docs/devnet.md`](devnet.md), with what is needed from Joaquin
   (upgrade-authority keypair, devnet SOL, Hermes key, the USDC mint and calendar choices) and
   the remaining gaps (M7b-2 receiver posting, an RPC `Chain` and a keeper binary). The
   instruction builders exist: `metalbook-sdk` builds every instruction (tags, accounts and
   flags as the handlers document them), derives every PDA and decodes every account through
   `metalbook_core::accounts`; LiteSVM tests check each builder byte for byte against the
   harness's independent encodings and run a full lifecycle using only the SDK.

   M7b-1 (Hermes source, no live calls in tests): `HermesSource` implements `PriceSource`
   over Hermes `GET /v2/updates/price/{t}`, which returns the **first** update with
   `publish_time >= t` (verified in the Hermes source, `RequestTime::FirstAfter`), and
   `/v2/updates/price/latest` as the anchor when nothing at or after `t` is published yet.
   `canonical_at(t)` is the first update at or after `t` when within tolerance and
   `prev < t`; `next_after(t)` the first at or after `t + 1` with `prev <= t`;
   `latest_at_or_before(t)` follows `prev_publish_time` back from the anchor (at most three
   requests: the update at or after `t` or `latest`, then one step back). Errors never carry
   response content: a malformed body is `KeeperError::MalformedResponse { status, byte_len,
   what }` with a fixed category (serde messages can echo field values, which could reflect
   the key), and the configured key is scrubbed from any error text as a backstop (codex,
   #17). The parsed response (`price` and `conf` as strings, `expo`, `publish_time`,
   `metadata.prev_publish_time`) becomes a `PrintMsg`, and the base64 binary update is kept
   for the receiver posting of M7b-2. HTTP is one blocking crate, `ureq` 3.5 with rustls
   (no native TLS), behind an `HttpGet` trait so tests replay fixtures. The API key comes only
   from `METALBOOK_HERMES_API_KEY`. **UNVERIFIED until the first live call: the auth scheme.**
   The default is `Authorization: Bearer <key>` (the only source found is Mysten Labs'
   deepbook SDK documentation; Pyth's documentation does not state it);
   `METALBOOK_HERMES_AUTH=header:<name>` switches to a named header. A 401 or 403 says so.
   `examples/hermes.rs --live` fetches one XAU print and exits with status 2 and a clear
   message when the key is not set. Tests: a recorded-shape fixture, the request URL and
   header, status handling (404 is "nothing yet", 401/403/500 are errors), and, over a
   synthetic tape served with Hermes' semantics, answers identical to `TapeSource` for every
   query (including the `latest` anchor). M7b-2, the Pyth receiver posting (Wormhole
   encoded VAA, `verify_encoded_vaa_v1`, then the receiver's `post_update` for a `Full`
   `PriceUpdateV2`; `post_update_atomic` only gives `Partial`), waits for a Hermes key to
   record a guardian-signed update for LiteSVM tests against the mainnet programs.

   Delivered first (M7a, no devnet yet): `metalbook-keeper` (off-chain, std), the honest
   keeper of the 4.3 trust assumption. `Keeper::tick` reads a series, restores its oracle
   terms and candidates with `metalbook-core` (so keeper and program apply the same rules),
   posts the canonical print of every empty window slot, every missing next-print link in
   order, and the latest fallback if it is later than the stored one, then settles as soon as
   the cascade allows. A failed post is benign only if the series, re-read right after,
   already holds the intended candidate (a lost race); any other failure ends the tick with
   an error and **without settling** (codex, #14: otherwise a receiver or RPC outage would let
   the keeper settle a subset it knew how to complete). Before posting, every candidate is
   checked with `metalbook_core::accept_print`, the same rule the program applies after
   Pyth's verification (positive price, pinned exponent, `conf / price <= max_conf_bps`,
   `prev_publish_time > 0`): an ineligible canonical slot is skipped and stays empty, an
   ineligible chain link ends the chain there, and the fallback steps back to the latest
   eligible print; only failures on eligible candidates abort the tick (codex, #14: one
   ineligible slot used to abort every tick and block all later slots). Prices come from a `PriceSource` trait (canonical print of a boundary,
   next strictly advancing print, latest first-of-second print; `TapeSource` replays a tape)
   and transactions from a `Chain` trait. The Hermes API key is read only from
   `METALBOOK_HERMES_API_KEY` (`HermesConfig::from_env`), never from a file, and `Debug`
   redacts it; the Hermes HTTP client and the Pyth receiver posting arrive with devnet
   (M7b). The account layouts (`Config`, `Calendar`, `Series`, `Writer`, the loaders and the series'
   accessors) moved from the program into `metalbook_core::accounts`, as quotebook did with
   quotebook-core, so the program, the keeper and any SDK decode from one source; off-chain
   readers use `accounts::read`, which copies from unaligned buffers. The LiteSVM tests keep
   their own restated offsets, so a layout change still has to be made twice on purpose. LiteSVM tests (`metalbook-tests/tests/keeper.rs`)
   with synthetic tapes: a keeper alone settles at the full-window mean; against an adversary
   who posted the 20 lowest canonical slots, one keeper tick within `D` posts the other 10 and
   settles at the full-window mean; with a 25-minute halt over the window it settles on the
   next-print chain from `expiry + D`; ticks are idempotent; with the adversary's 20 slots on
   chain and every post failing, the tick errors and does not settle, and the next working
   tick settles the full window; a post reported failed whose candidate landed is benign. Run `tick` more often than `D`.

## Verified facts (checked 2026-10-28)

- Pyth publishes both feeds (Hermes `/v2/price_feeds?query=XAU&asset_type=metal` and `XAG`):
  `Metal.XAU/USD` `765d2ba906dbc32ca17cc11f5310a89e9ee1f6420508c63861f2f8ba4ee34bb2`,
  `Metal.XAG/USD` `f2fb02c32b055c805e7238d628e5e9dadef274376114eb1f012337cabe93871e`.
  Their `schedule` attribute is as summarized in 4.4.
- They are available through the pull oracle (Hermes update, posted by the Pyth receiver as a
  `PriceUpdateV2`). Pyth's market-hours documentation page is checked in milestone 3.
- Hermes `/v2/updates/price/*` and Benchmarks `/v1/updates/price/*` return 401 without an
  API key (varswap, 2026-10-29); `/v2/price_feeds` is still open. Mainnet settlement tapes
  (milestone 3) and keepers (milestone 7) need a Hermes key, read from an environment
  variable and never committed; until then tests use synthetic tapes.
- Sponsored push feeds are not usable for settlement (varswap M2, 2026-10-29): the XAG/USD
  push account had not updated since 2026-07-03, while XAU/USD and SOL/USD were current;
  the XAU/USD exponent is −3. So settlement accepts any `Full`-verified `PriceUpdateV2` for
  the series' feed id, posted by anyone (a keeper pulling from Hermes), never a fixed push
  account; and the exponent is pinned per series (4.5), not assumed.
- `PriceUpdateV2` layout and crate versions: see varswap's plan.

## Dependencies (latest stable on crates.io, checked 2026-10-29)

Rule from `rules/global_rules.md`: every dependency at the latest stable version, written
`MAJOR.MINOR`, re-checked via `https://crates.io/api/v1/crates/<name>` (`max_stable_version`)
whenever it is added or touched.

| Crate | Latest stable | Requirement |
| --- | --- | --- |
| `pinocchio` | 0.12.0 (0.11.2 used) | `0.11`, decided exception below |
| `bytemuck` | 1.25.2 | `1.25` |
| `proptest` (dev) | 1.12.0 | `1.12` |
| `litesvm` (dev) | 0.18.1 | `0.18` |
| `mollusk-svm` (dev, optional) | 0.16.4 | `0.16` |
| `solana-address` | 2.9.0 | `2.9` |
| `pinocchio-system` | 0.7.0 (0.6.1 used) | `0.6`, decided exception below |
| `pinocchio-token` | 0.7.0 | `0.7`, see conflict below |
| `pinocchio-associated-token-account` | 0.4.0 | `0.4`, see conflict below |
| `optfix` | unpublished | git, pinned `rev` |
| `pyth-lite` (varswap) | unpublished | git, `rev = 5cf95aa` |
| `quotebook-core`, `quotebook-sdk` | unpublished | git, `rev = 578314c` (quotebook M5 SDK) |
| `rust_decimal` (maker) | 1.43.0 | `1.43` |
| `thiserror` (maker) | 2.0.21 | `2.0` |
| `optionstratlib` (maker, dev) | 0.23.0 | `0.23` |
| `rust_decimal_macros` (maker, dev) | 1.40.0 | `1.40` |
| `ureq` (keeper, rustls only) | 3.5.0 | `3.5` |
| `serde` (keeper) | 1.0.229 | `1.0` |
| `serde_json` (keeper) | 1.0.154 | `1.0` |
| `base64` (keeper) | 0.23.1 | `0.23` |
| `litesvm-token` (dev) | 0.18.1 | `0.18` |
| `solana-account` (dev) | pinned by LiteSVM | `4.3` |
| `solana-clock` (dev) | pinned by LiteSVM (`~3.2.0`) | `3.2` |
| `pyth-solana-receiver-sdk` (dev only) | 2.0.0 | `2.0` |

**Version decision (Joaquin, 2026-10-29): option (b), the Pinocchio 0.11 generation.**
`pinocchio-token` 0.7.0 and `pinocchio-associated-token-account` 0.4.0 are built on
`solana-account-view` 2 (Pinocchio 0.11), while `pinocchio` 0.12.0 uses 3, so the latest
stable versions cannot be combined. All four repos and matchbook therefore use `pinocchio`
0.11 (0.11.2) and `pinocchio-system` 0.6 (0.6.1), the newest releases compatible with the
token crates. This is the documented exception to the latest-stable rule; move all four to
the 0.12 generation together once 0.12-compatible token crates are released.

## 6. Open questions

1. Capped calls (proposed, the metalbook session agrees) acceptable, or physically-backed
   collateral?
2. The settlement cascade in 4.3 (proposed, from the metalbook session's review): OK?
3. Expiry calendar: which days and times? Monthly only, or weeklies too?
4. `mark_vol` from a keeper (proposed) or derived on-chain from a varswap window's realized
   variance (interesting, but realized is not implied)?
5. Series listing permissionless within the calendar and grid (proposed) or admin-only?
6. Use metals' lease rate as optfix carry `q` (depends on optfix question 4)?
7. Decided (Joaquin, 2026-10-29): unredeemed and unclaimed USDC stays claimable forever, with
   permissionless account closing to recover rent once nothing is owed (4.6).
