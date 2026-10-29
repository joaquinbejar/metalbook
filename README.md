# MetalBook

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

Cash-settled European options on gold (XAU/USD) and silver (XAG/USD) on Solana, fully
collateralized and settled in USDC from Pyth prices.

**Status:** milestones 1 to 4 of [`docs/PLAN.md`](docs/PLAN.md) are implemented: the domain math
and settlement rules (`metalbook-core`) and the on-chain program (`metalbook-program`) for
listing, writing, settlement, redemption and writer claims. Pricing and quoting through optfix
and QuoteBook (milestone 5) and devnet (milestone 7) are next. Not audited; not deployed.

## Contents

- [The product](#the-product)
- [Architecture](#architecture)
- [Accounts](#accounts)
- [Instructions](#instructions)
- [Lifecycle](#lifecycle)
- [Settlement and its trust assumption](#settlement-and-its-trust-assumption)
- [Metals market hours](#metals-market-hours)
- [Error codes](#error-codes)
- [Building and testing](#building-and-testing)

## The product

A **series** is one option: underlying (XAU or XAG), kind (call or put), strike `K`, expiry,
contract multiplier and, for calls, a cap. A **writer** deposits USDC collateral and receives
**long tokens** (an SPL mint, 0 decimals), one per contract, which can be sold or held. At
expiry the series fixes a settlement price `S` from Pyth; each long token then pays its payoff
in USDC and each writer recovers the rest of its collateral.

Everything is fully collateralized, so no position can ever be undercollateralized and nothing
on chain depends on a mark price:

| Kind | Payoff per ounce | Collateral per ounce |
| --- | --- | --- |
| Put | `max(K − S, 0)` | `K` |
| Capped call | `min(max(S − K, 0), cap − K)` | `cap − K` |

A **capped call** is economically a call spread `K / cap`: a plain cash-settled call has no
upper bound and could not be fully collateralized (PLAN 4.1). Caps sit on a grid of
`K × (1 + x%)` set in the config (for example 10%, 25%, 50%), and the interface should present
the product as what it is.

Amounts are integers in USDC atoms (6 decimals). Collateral per contract is the exact maximum
payoff times the multiplier, **rounded up**; the payout per long token is the exact payoff,
**rounded down**; the writer receives the difference, so the rounding dust belongs to the
writer and the vault empties exactly.

## Architecture

| Crate | Role |
| --- | --- |
| `metalbook-core` | `no_std`, integers only, never panics. Unit newtypes, series terms, payoff and collateral, the vault ledger of a series (`SeriesLedger`), and the settlement rules: canonical window slots, the next-print chain, the fallback and the settlement cascade (`SettlementState`). Property-tested against independent references. |
| `metalbook-program` | Pinocchio 0.11 program (`cdylib`). Validates every account, restores the core types from account data (rejecting unreachable states), applies the transition, writes state, then makes the token CPIs. |
| `metalbook-tests` | LiteSVM tests of the program and the deterministic CU benchmark (`docs/benchmarks.md`). |
| `metalbook-maker` | Off-chain pricing adapter (milestone 5a): payoff-aware marks behind a `VanillaPricer` trait (a put; a capped call as `C(K) − C(cap)`, error bounds summed), two-sided quotes rounded toward the maker, and quotebook quote terms. |
| `lint-control` | Failing control proving that the workspace lints reject panicking macros (shared with optfix, quotebook and varswap). |

External pieces: SPL Token and the associated token program (CPI through `pinocchio-token`
and `pinocchio-associated-token-account`), and varswap's `pyth-lite` (git, pinned by rev),
which decodes Pyth `PriceUpdateV2` accounts.

## Accounts

| Account | Address | Owner | Size | Holds |
| --- | --- | --- | --- | --- |
| `Config` | PDA `["config"]` | metalbook | 280 B | admin; USDC mint and token program; strike tick, multiplier, feed id and Pyth exponent per metal; cap grid; listing time; oracle terms (window, slots, tolerance, `min_samples`, `k`, challenge delay `D`, fallback lookback, hard deadline) |
| `Calendar` | PDA `["calendar", [underlying]]` | metalbook | 520 B | up to 64 allowed expiries, strictly increasing |
| `Series` | PDA `["series", [underlying], [kind], strike, expiry, cap, multiplier]` (little endian) | metalbook | 504 B | terms; a **snapshot** of every config value it uses; long mint and vault; vault ledger; settlement candidates; settlement method, price and sample count; rent payer |
| Long mint | PDA `["long", series]` | SPL Token | 82 B | mint authority the series, no freeze authority |
| Vault | associated token account of the series for USDC | SPL Token | 165 B | the collateral |
| `Writer` | PDA `["writer", series, owner]` | metalbook | 80 B | contracts written and not closed; whether the residual was claimed |

Every program account starts with a tag and a layout version, is `#[repr(C)]` and `Pod`
without implicit padding, and is re-derived from its seeds with its stored canonical bump on
every use. A live series never reads the config again: changing the config affects only series
listed afterwards.

## Instructions

Instruction data is a one-byte tag followed by little-endian fields. Each handler documents
its accounts and fields in `metalbook-program/src/processor/`.

| Tag | Instruction | Signer | Effect |
| ---: | --- | --- | --- |
| 0 | `init_config` | program upgrade authority | creates the config; nobody else can, so deployment cannot be front-run |
| 1 | `set_config` | admin | new parameters (and possibly a new admin); USDC mint and token program are fixed |
| 2 | `set_calendar` | admin | creates or replaces a metal's expiry list |
| 3 | `create_series` | anyone (pays rent) | lists a series inside the calendar and the grids; creates the series, long mint and vault |
| 4 | `mint` | writer | deposits `n × max_payout`, receives `n` long tokens; creates the writer position on first use |
| 5 | `close` | writer | before expiry: burns `n` own long tokens, withdraws `n × max_payout`, at most what it wrote |
| 6 | `sample_settlement` | anyone | posts a Pyth `PriceUpdateV2` as a window slot, chain link or fallback |
| 7 | `settle` | anyone | runs the settlement cascade: fixes the price, or voids the series |
| 8 | `redeem` | long holder | burns `n` own long tokens, receives `n × payout` |
| 9 | `claim_writer` | writer | once: receives `written × (max_payout − payout)` |
| 10 | `close_series` | anyone | once nothing is owed: closes the vault and the series, rent (and any unowed vault balance) to the `create_series` payer |
| 11 | `close_position` | anyone | closes a claimed (or empty) writer position, rent to the writer |

Compute units of each: [`docs/benchmarks.md`](docs/benchmarks.md).

## Lifecycle

```text
 set_calendar ─► create_series ─► Active ──(mint / close until expiry)──┐
                                    │                                    │
                     sample_settlement during the window, at any time    │
                                    │                                    │
                           first sample or settle at or after expiry ◄──┘
                                    ▼
                                 Expired ──settle──► Settled ──► redeem, claim_writer ──► close_series
                                    │
                                    └──settle (no price by the hard deadline)──► Voided ──► claim_writer, redeem (0) ──► close_series
```

1. The admin publishes the expiry calendar of each metal (only times when the metal trades).
2. Anyone lists a series whose expiry is in the calendar and at least `min_listing_s` away, with
   a strike on the metal's tick, the configured multiplier and, for a call, a cap on the grid.
3. Until expiry, writers `mint` (deposit collateral, receive long tokens) and `close` (burn
   their own long tokens, withdraw collateral).
4. Anyone posts Pyth prints with `sample_settlement`. The first post or `settle` at or after
   expiry moves the series to `Expired`; minting and closing check the clock themselves.
5. `settle` fixes the settlement price (or voids the series if no price exists by the hard
   deadline). It can be called by anyone; a second call fails.
6. Holders `redeem`, writers `claim_writer`, in any order and with no deadline: payouts stay
   claimable forever, and nothing ever moves USDC to anyone but its owner (PLAN 4.6). When
   all have, the vault holds exactly zero. After a void, long tokens pay nothing and writers
   recover all their collateral.
7. Once nothing is owed, anyone can close the series and its vault (`close_series`) and the
   claimed writer positions (`close_position`), returning the rent to whoever paid it. A
   voided series closes once its long tokens are burned (for nothing). The SPL Token long
   mint cannot be closed.

## Settlement and its trust assumption

The settlement price is the mean of a 30-minute window before expiry, one **canonical** Pyth
print per minute: for each boundary `t_i`, the unique print with
`prev_publish_time < t_i <= publish_time <= t_i + tolerance` (PLAN 4.2). Pyth's
`prev_publish_time` makes that print unique, so whoever posts a slot cannot choose its price,
and slots can be backfilled later from Hermes. If the window is too thin (a market halt), the
cascade falls back, in order (PLAN 4.3):

1. **Window**: all 30 slots, at once from expiry; or at least `min_samples`, from
   `D` after the last new slot.
2. **Next print**: a complete chain of `k` canonical prints that strictly advance after expiry,
   from expiry `+ D`.
3. From the hard deadline (7 days): a partial chain, else a partial window, each `D` after its
   last change.
4. **Fallback**: the latest first-of-its-second print in the 24 h before expiry, `D` after its
   last change.
5. **Void**, from the hard deadline `+ D`, only if nothing at all was posted.

A candidate that exists but is still inside its challenge delay `D` blocks every lower step.

**Trust assumption.** Canonicity removes the choice of price *within* each slot, link and
fallback, not the choice of *which* candidates exist when `settle` runs. With nobody else
posting, an adversary could post a favorable subset of the window, a partial chain or an older
fallback, wait `D` and settle on it. Each candidate is monotonically improvable, and each
improvement restarts its `D` (a complete window settles at once and a complete chain from
expiry `+ D`, since neither leaves a subset to choose). MetalBook is therefore correct under
this assumption: **at least one honest keeper submits, within `D` of any change, every
available candidate of higher priority than the one that would otherwise settle**: every
window slot with a canonical print, every chain link and the latest fallback. The default
keeper is run by the protocol (milestone 7) and posts every canonical print from Hermes as
soon as it exists; anyone else can run one, since every instruction it needs is
permissionless.

## Metals market hours

Pyth's XAU/USD and XAG/USD feeds follow the metals' trading schedule (New York time: Sunday
18:00 to Friday 17:00, with a daily pause from 17:00 to 18:00 and holiday closures), and do
not update while the market is closed. Hence (PLAN 4.4):

- **Expiries only come from the admin calendar**, chosen when the market is open with margin on
  both sides of the settlement window (for example Wednesdays and the last Friday of the
  month at 15:00 New York, never on early-close days). The program cannot know holidays;
  the calendar is the control, and `create_series` rejects anything else.
- **The settlement window sits inside trading hours** by construction; the cascade above covers
  an unexpected halt.
- **Nothing on chain depends on a mark**: collateral is the contractual maximum payout, so a
  closed market never blocks minting, closing, redeeming or claiming. Marks and quoting
  (milestone 5) widen or stop while the market is closed and from the start of the
  settlement window.

## Error codes

Every failure is `ProgramError::Custom(code)`. Codes never change; new ones are appended.

| Code | Name | Meaning |
| ---: | --- | --- |
| 100 | `InvalidInstruction` | Empty instruction data or an unknown tag |
| 101 | `InvalidInstructionData` | Data of the wrong length, or a field out of range |
| 102 | `MissingSignature` | An account that must sign did not |
| 103 | `InvalidAccountOwner` | An account is not owned by the program that must own it |
| 104 | `InvalidPda` | An account is not the address its seeds give |
| 105 | `InvalidMint` | A mint is not the one the series or the config names |
| 106 | `InvalidVault` | Not the series' vault, or a vault with a delegate or close authority |
| 107 | `InvalidSysvar` | The clock could not be read |
| 108 | `NotAuthority` | Not the admin, or not the program's upgrade authority |
| 109 | `SeriesNotListed` | The series account does not exist |
| 110 | `ExpiryNotInCalendar` | The expiry is not in the metal's calendar |
| 111 | `StrikeOffGrid` | The strike is not a multiple of the metal's tick |
| 112 | `InvalidCap` | A call's cap is not on the grid or not above the strike, or a put's cap is not zero |
| 113 | `SeriesExpired` | Mint or close at or after expiry |
| 114 | `SeriesNotExpired` | Settle before expiry |
| 115 | `SeriesNotSettled` | Redeem, claim or close before settlement |
| 116 | `SeriesAlreadySettled` | Sample or settle after settlement or a void |
| 117 | `InsufficientCollateral` | Reserved; the token program rejects an underfunded deposit itself |
| 118 | `InsufficientLongTokens` | Redeem more long tokens than outstanding |
| 119 | `OracleOwnerMismatch` | The price update is not owned by the series' Pyth receiver |
| 120 | `OracleNotFullyVerified` | The price update is not `Full`-verified |
| 121 | `OracleFeedMismatch` | The price update is for another feed |
| 122 | `OracleStale` | Reserved; settlement bounds time itself |
| 123 | `OracleConfidenceTooWide` | `conf / price` above the series' bound |
| 124 | `SampleOutsideWindow` | The print qualifies as no slot, link or fallback |
| 125 | `NotEnoughSamples` | Reserved |
| 126 | `Overflow` | An amount does not fit its integer type |
| 127 | `Pricing` | Reserved (optfix, milestone 5) |
| 128 | `InvalidAccountData` | Wrong length, tag, version or layout |
| 129 | `InvalidProgram` | The token, system or associated token program is not the expected one |
| 130 | `InvalidTokenAccount` | A token account has the wrong mint or owner, or is not initialized |
| 131 | `AccountNotWritable` | An account that must be writable is not |
| 132 | `AlreadyInitialized` | The account to create already exists |
| 133 | `InvalidConfig` | Config parameters out of range |
| 134 | `InvalidMultiplier` | Not the multiplier configured for the metal |
| 135 | `ListingTooLate` | The expiry is sooner than the minimum listing time |
| 136 | `InsufficientWritten` | Close or claim beyond what this writer wrote, or no position |
| 137 | `ZeroAmount` | Zero contracts |
| 138 | `InvariantViolated` | Stored accounting is inconsistent; never expected |
| 139 | `InvalidStrike` | The strike is zero |
| 140 | `SeriesNotActive` | A writer operation on a series that is no longer active |
| 141 | `SettlementNotReady` | The cascade is waiting: a candidate is inside its delay, or the hard deadline has not passed |
| 142 | `OracleExponentMismatch` | The price update's exponent is not the series' pinned exponent |
| 143 | `InvalidPrint` | Non-positive price, or `prev_publish_time <= 0` |
| 144 | `AlreadyClaimed` | The writer position has already claimed |
| 145 | `SeriesNotEmpty` | Close while long tokens or writer claims are outstanding, or the vault owes USDC |
| 146 | `InvalidRentPayer` | The rent destination is not the account that paid it |
| 147 | `PositionNotClaimed` | Close a writer position that has not claimed |

## Building and testing

Requirements: stable Rust and the Agave toolchain (`cargo build-sbf`).

```sh
make pre-push   # fmt check, clippy -D warnings, lint and program controls, SBF build, all tests, docs
make program    # build target/deploy/metalbook_program.so (fails on SBF frames above 4 KiB)
make test       # unit, property and LiteSVM tests (needs `make program` first)
make bench      # regenerate docs/benchmarks.md
```

Project rules (no panics outside tests, latest stable dependencies, checked arithmetic,
explicit rounding, negative tests with forged accounts): [`rules/global_rules.md`](rules/global_rules.md).

## License

MIT
