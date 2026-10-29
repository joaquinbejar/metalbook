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

`metalbook-core` (`no_std`), on optfix types.

```rust
#[repr(u8)] pub enum Underlying { Xau = 0, Xag = 1 }
pub use optfix::OptionKind;                  // Call, Put
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
#[repr(u8)] pub enum SettleMethod { Window = 0, NextPrint = 1, Fallback = 2 }
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
| 5 | `sample_settlement` | anyone | add a Pyth print inside the settlement window |
| 6 | `settle` | anyone | after the window: fix the settlement price (4.2) |
| 7 | `redeem` | long holder | burn long tokens, receive `payout × n` |
| 8 | `claim_writer` | writer | receive `written × (max_payout − payout)` |
| 9 | `set_mark_vol` | keeper | update `mark_vol` (no effect on settlement) |
| 10 | `void_series` | anyone after hard deadline, if 4.3 says void | unwind |

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

Uniqueness is not proven yet; it is a milestone 3 gate. Pyth documents the rule
(`pythnet_sdk/src/messages.rs`, `PriceFeedMessage::prev_publish_time`) but also two
exceptions: during the message-sending migration some updates may not reach other chains, so
a moment can have no canonical update; and `prev_publish_time` may equal `publish_time` when
the aggregation was unsuccessful. Milestone 3 checks the rule against mainnet fixtures
captured from Hermes (including same-second and failed-aggregation messages) before relying
on it. If uniqueness cannot be shown, the fallback is first-write-wins with the residual
selection risk quantified and accepted by you (codex review). Test: two valid, different
prices for the same slot in both orders; only the canonical one is accepted.

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
2. `NextPrint`: canonical and chained, so there is no choice and, with all `k` links, no
   challenge delay. The "next print" after a time `t` is the canonical update that strictly
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
5. `void_series` only if all of these are empty at the hard deadline: collateral goes back
   to writers, since longs have no price. Never void silently: the method used is stored and
   logged.

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

### 4.6 Unredeemed longs

After a redeem deadline (proposed 1 year after settlement) anyone can sweep both unclaimed
shares to a treasury: `unredeemed_longs × payout + Σ unclaimed writers × (max_payout −
payout)`, so the vault still empties exactly. Until then the USDC stays locked in the series
vault. Open question 7.

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
2. **Series, mint, close.** *Done when* LiteSVM tests and a negative test per rule in 3.3 pass.
3. **Settlement.** Sampling, window mean, next print, fallback/void. *Done when* tests replay
   XAU fixtures captured from mainnet including a closed-market gap, and every branch of 4.3
   is covered.
4. **Redeem and claim.** *Done when* end-to-end ITM, ATM, OTM for both kinds and both metals
   empty the vault exactly.
5. **Pricing and quoting.** optfix marks, quotebook series, maker config. *Done when* a LiteSVM
   demo quotes and settles an option token through quotebook at an optfix price.
6. **Benchmarks and docs.** *Done when* `docs/benchmarks.md` lists CU for every instruction and
   README explains the capped call and the market-hours rules.
7. **Devnet.** *Done when* a full series lifecycle runs on devnet against live Pyth XAU.

## Verified facts (checked 2026-10-28)

- Pyth publishes both feeds (Hermes `/v2/price_feeds?query=XAU&asset_type=metal` and `XAG`):
  `Metal.XAU/USD` `765d2ba906dbc32ca17cc11f5310a89e9ee1f6420508c63861f2f8ba4ee34bb2`,
  `Metal.XAG/USD` `f2fb02c32b055c805e7238d628e5e9dadef274376114eb1f012337cabe93871e`.
  Their `schedule` attribute is as summarized in 4.4.
- They are available through the pull oracle (Hermes update, posted by the Pyth receiver as a
  `PriceUpdateV2`). Not verified: whether sponsored push accounts exist for them on Solana,
  and Pyth's market-hours documentation page; both checked in milestone 3.
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
| `litesvm-token` (dev) | 0.18.1 | `0.18` |
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
7. Sweep unredeemed USDC to a treasury after a deadline (proposed), or keep it locked forever?
