<div align="center">

# MetalBook

**Cash-settled European options on gold and silver on Solana**

</div>

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://github.com/joaquinbejar/metalbook/blob/main/LICENSE)
[![Stars](https://img.shields.io/github/stars/joaquinbejar/metalbook.svg)](https://github.com/joaquinbejar/metalbook/stargazers)
[![Issues](https://img.shields.io/github/issues/joaquinbejar/metalbook.svg)](https://github.com/joaquinbejar/metalbook/issues)
[![PRs](https://img.shields.io/github/issues-pr/joaquinbejar/metalbook.svg)](https://github.com/joaquinbejar/metalbook/pulls)
[![Solana](https://img.shields.io/badge/Solana-Pinocchio%200.11-9945FF)](https://github.com/anza-xyz/pinocchio)

## MetalBook v0.1.0: Fully Collateralized Metal Options

### Table of Contents
1. [Introduction](#introduction)
2. [Status](#status)
3. [Features](#features)
4. [Architecture](#architecture)
5. [Collateral Model](#collateral-model)
6. [Lifecycle](#lifecycle)
7. [Settlement and its Trust Assumption](#settlement-and-its-trust-assumption)
8. [Market Hours and Expiry](#market-hours-and-expiry)
9. [On devnet](#on-devnet)
10. [Setup Instructions](#setup-instructions)
11. [Usage Examples](#usage-examples)
12. [Testing and Benchmarks](#testing-and-benchmarks)
13. [Error Codes](#error-codes)
14. [Contribution and Contact](#contribution-and-contact)
15. [License](#️-license)
16. [Related projects](#related-projects)

### Introduction

MetalBook lists cash-settled European options on gold (XAU/USD) and silver (XAG/USD) on
Solana. Every position is fully collateralized in USDC and settled from Pyth prices: a
**writer** deposits the maximum payout of a series and receives **long tokens** (an SPL mint,
0 decimals, one per contract) that can be held or sold; at expiry the series fixes a
settlement price from a 30 minute window of canonical Pyth prints, each long token pays its
payoff in USDC and each writer recovers the rest of its collateral.

The on-chain program is written with [Pinocchio](https://github.com/anza-xyz/pinocchio)
(not Anchor): `no_std`, no allocator, zero-copy `bytemuck` accounts, integer arithmetic only,
checked everywhere, with every rounding direction explicit. Marks are computed off-chain with
[optfix](https://github.com/joaquinbejar/optfix), long tokens trade through
[QuoteBook](https://github.com/joaquinbejar/quotebook), and Pyth updates are decoded with
varswap's `pyth-lite`.

### Status

Deployed on **devnet** (see [On devnet](#on-devnet)); **not audited, not on mainnet.**
Implemented and reviewed:

- the domain math, settlement rules and account layouts (`metalbook-core`);
- the on-chain program (`metalbook-program`): listing, writing, settlement from Pyth,
  redemption, writer claims and rent recovery;
- the pricing adapter (`metalbook-maker`) priced by optfix, with a LiteSVM demo trading a
  long token through QuoteBook;
- the settlement keeper (`metalbook-keeper`) with its Hermes price source and Pyth receiver
  posting through Wormhole (M7a, M7b);
- the client SDK (`metalbook-sdk`) and the admin CLI (`metalbook-cli`, binary `metalbook-admin`).

Pending ([`docs/PLAN.md`](docs/PLAN.md), milestone 7): the keeper binary (M7c, in review)
and the first settlement on devnet, the Oct 30 series at 20:00Z.

### Features

#### 1. **Products**
- **Puts** collateralized by strike: payoff `max(K − S, 0)`, collateral `K` per ounce
- **Capped calls** (economically a `K / cap` call spread), caps on a configured grid
- Two underlyings: XAU/USD and XAG/USD, each with its own strike tick, multiplier and feed

#### 2. **Safety by construction**
- Full collateralization: no margin, no liquidations, nothing on chain depends on a mark
- Collateral rounded up, payouts rounded down; the writer owns the dust and the vault empties exactly
- Each series snapshots every config value it uses; config changes affect only later listings
- Every account re-derived from its seeds with its stored canonical bump; a negative test with a forged account for every validation rule
- Workspace lints deny panics, `unwrap`, indexing, unchecked and float arithmetic

#### 3. **Oracle settlement**
- 30 slot window of canonical Pyth prints, unique per slot through `prev_publish_time`
- A settlement cascade for halts: window, next print chain, partial candidates, fallback, void
- Permissionless sampling and settlement; a challenge delay `D` lets an honest keeper improve any candidate

#### 4. **Tooling**
- Rust SDK: a builder for every instruction, PDA helpers and account decoders
- Admin CLI over JSON-RPC with `--dry-run` and JSON reads
- Honest settlement keeper with a Hermes price source
- Deterministic compute unit benchmark (`make bench`)

## Architecture

### Program

`metalbook-program` is a Pinocchio 0.11 `cdylib`. Each handler validates every account,
restores the core types from account data (rejecting unreachable states), applies the
transition, writes state, and only then makes the token CPIs (SPL Token and the associated
token program, through `pinocchio-token` and `pinocchio-associated-token-account`).

### Accounts

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

### Calendar

A `Calendar` per metal holds up to 64 strictly increasing expiries, set by the admin with
`set_calendar` (the list replaces the previous one). `create_series` rejects any expiry not in
the calendar or sooner than `min_listing_s`. See [Market Hours and Expiry](#market-hours-and-expiry).

### Instructions

Instruction data is a one byte tag followed by little endian fields. Each handler documents
its accounts and fields in `metalbook-program/src/processor/`; `metalbook-sdk` builds all of
them (`metalbook_sdk::ix`).

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

### Keeper

`metalbook-keeper` (off-chain, std) implements the honest keeper of the trust assumption.
Each `Keeper::tick` reads a series with the program's own layouts, posts the canonical print
of every empty window slot, every missing chain link in order and the latest fallback, then
calls `settle` as soon as the cascade allows. Every candidate is first checked with
`metalbook_core::accept_print`, the same rule the program applies: an ineligible slot is
skipped, an ineligible chain link ends the chain there, and the fallback steps back to the
latest eligible print. A failed post is benign only if the series, re-read right after,
already holds the candidate (another keeper won the race); any other failure ends the tick
with an error and **without settling**. Run `tick` more often than `D`. Prices come from a
`PriceSource` trait (`HermesSource` in production, `TapeSource` in tests) and transactions
from a `Chain` trait (LiteSVM in tests; the RPC implementation is pending).

### Hermes client

`HermesSource` implements `PriceSource` over Hermes `GET /v2/updates/price/{t}` (the first
update with `publish_time >= t`) and `/v2/updates/price/latest` as the anchor. HTTP is one
blocking crate, `ureq` 3.5 with rustls (no native TLS), behind an `HttpGet` trait so tests
replay fixtures. The API key is read **only** from the environment variable
`METALBOOK_HERMES_API_KEY` (`HermesConfig::from_env`); it is never read from a file, `Debug`
redacts it, and errors never carry response content. The auth scheme defaults to
`Authorization: Bearer <key>`; `METALBOOK_HERMES_AUTH=header:<name>` switches to a named
header, and `METALBOOK_HERMES_URL` overrides the base URL. The scheme is unverified until the
first live call.

### SDK

`metalbook-sdk` builds every instruction (`metalbook_sdk::ix`), derives every PDA
(`metalbook_sdk::pda`: `config`, `calendar`, `series`, `long_mint`, `position`, `ata`,
`program_data`) and decodes every account (`metalbook_sdk::decode`) through
`metalbook_core::accounts`. LiteSVM tests check each builder byte for byte against an
independent encoding and run a full lifecycle using only the SDK.

### CLI

`metalbook-admin` (crate `metalbook-cli`) is a thin admin CLI over the SDK: `init-config`,
`set-config`, `set-calendar` and `create-series`, signed with a Solana keypair file
(`--keypair` or `METALBOOK_KEYPAIR`) and sent over JSON-RPC (`--url` or `SOLANA_URL`), plus
`show-config`, `show-calendar`, `show-series` (decoded, as JSON) and an offline
`series-address`. `--dry-run` prints the instruction instead of sending it. The RPC URL never
appears in errors, `Debug` or `--help`, and keypair files are never printed.

### Quoting through QuoteBook

Long tokens trade on [QuoteBook](https://github.com/joaquinbejar/quotebook) as a long mint /
USDC series (one long token per base lot). A maker marks a series with `metalbook-maker`
(`mark`: a put, or a capped call as the call spread `C(K) − C(cap)`, scaled by the
multiplier; no marks while the market is closed or once the settlement window has started),
turns the mark into a bid and an ask rounded toward itself (`quote_atoms`, exact integer
tick math), and builds quotebook quote terms (`quote_terms`). A taker fills through
quotebook's signed-quote `settle`. `metalbook-tests/tests/quotebook_demo.rs` runs the whole
path in LiteSVM, from minting long tokens to the quotebook fill, the Pyth settlement and the
redemption.

### Workspace crates

| Crate | Role |
| --- | --- |
| `metalbook-core` | `no_std`, integers only, never panics: unit newtypes, series terms, payoff and collateral, the vault ledger (`SeriesLedger`), the settlement rules and `accept_print`, and the on-chain account layouts shared by the program, the keeper and the SDK. Property tested against independent references. |
| `metalbook-program` | The Pinocchio program (`cdylib`). |
| `metalbook-tests` | LiteSVM tests of the program, the QuoteBook demo, the CLI tests and the CU benchmark. |
| `metalbook-sdk` | Instruction builders, PDA helpers and account decoders. |
| `metalbook-cli` | `metalbook-admin`, the admin CLI. Devnet parameters: `metalbook-cli/devnet-params.json`. |
| `metalbook-maker` | Off-chain pricing adapter: payoff aware marks behind a `VanillaPricer` trait (`OptfixPricer` over optfix), two sided quotes rounded toward the maker, and quotebook quote terms. |
| `metalbook-keeper` | Off-chain settlement keeper and Hermes price source. |
| `lint-control` | Failing control proving the workspace lints reject panicking macros (shared with optfix, quotebook and varswap). |

## Collateral Model

A **series** is one option: underlying (XAU or XAG), kind (call or put), strike `K`, expiry,
contract multiplier and, for calls, a cap.

| Kind | Payoff per ounce | Collateral per ounce |
| --- | --- | --- |
| Put | `max(K − S, 0)` | `K` |
| Capped call | `min(max(S − K, 0), cap − K)` | `cap − K` |

**Puts are collateralized by strike**: the worst case is `S = 0`, so `K` per ounce covers it.
**Calls are capped**: a plain cash-settled call has no upper bound and could not be fully
collateralized, so a call pays at most `cap − K`; it is economically a call spread `K / cap`,
and the interface should present it as such. Caps sit on a grid of `K × (1 + x%)` set in the
config (for example 10%, 25%, 50%).

Amounts are integers in USDC atoms (6 decimals); multipliers are in milli-ounces. Collateral
per contract is the exact maximum payoff times the multiplier, **rounded up**; the payout per
long token is the exact payoff, **rounded down**; the writer receives the difference, so the
rounding dust belongs to the writer and the vault empties exactly.

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
Every posted print must pass Pyth's verification (owned by the Pyth receiver, `Full`, the
series' feed id) and the series' own rule, `metalbook_core::accept_print`: a positive price,
the series' pinned exponent, `conf / price` within the series' bound, and
`prev_publish_time > 0`.

**Trust assumption.** Canonicity removes the choice of price *within* each slot, link and
fallback, not the choice of *which* candidates exist when `settle` runs. With nobody else
posting, an adversary could post a favorable subset of the window, a partial chain or an older
fallback, wait `D` and settle on it. Each candidate is monotonically improvable, and each
improvement restarts its `D` (a complete window settles at once and a complete chain from
expiry `+ D`, since neither leaves a subset to choose). MetalBook is therefore correct under
this assumption: **at least one honest keeper submits, within `D` of any change, every
available candidate of higher priority than the one that would otherwise settle**: every
window slot with a canonical print, every chain link and the latest fallback. The default
keeper is run by the protocol (`metalbook-keeper`, milestone 7) and posts every canonical
print from Hermes as soon as it exists; anyone else can run one, since every instruction it
needs is permissionless.

## Market Hours and Expiry

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

## On devnet

Program [`FPPuou5kgSp6tbwQRwKPBNhrMyf1Xb8n2ib3Yqpbsuwg`](https://explorer.solana.com/address/FPPuou5kgSp6tbwQRwKPBNhrMyf1Xb8n2ib3Yqpbsuwg?cluster=devnet),
deployed on 2026-10-30 from commit `93f175a`, collateral in Circle's devnet USDC
(`4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU`). Every address and signature is in
[`docs/devnet-log.md`](docs/devnet-log.md); the runbook is [`docs/devnet.md`](docs/devnet.md).

| Account | Address |
| --- | --- |
| Config | [`CgJYkT8Q3KnsAMj8gPJpwUfabiu66ezLQ4NjxVsjMZwx`](https://explorer.solana.com/address/CgJYkT8Q3KnsAMj8gPJpwUfabiu66ezLQ4NjxVsjMZwx?cluster=devnet) |
| XAU calendar | [`2p6wAvNefHwGsTDkMMLU5JpSE2iPZyekEfBLXcstFpwh`](https://explorer.solana.com/address/2p6wAvNefHwGsTDkMMLU5JpSE2iPZyekEfBLXcstFpwh?cluster=devnet) |
| XAG calendar | [`Cy6qBFFPRymJNmQwDkhaNoggf3vk2WyDCBbD9e5K3FUY`](https://explorer.solana.com/address/Cy6qBFFPRymJNmQwDkhaNoggf3vk2WyDCBbD9e5K3FUY?cluster=devnet) |

Calendars: weekly Fridays at 16:00 New York from Oct 30 to Dec 18 (Nov 27 and Dec 25 left
out). Series: strikes at the money ±5% in $50 (XAU) and $1 (XAG) steps, each a put and a
capped call with the cap 10% above the strike. Examples:

| Series | Terms | Account |
| --- | --- | --- |
| XAU put | strike 3800, Oct 30, 1 oz | [`2CpwYjJVt33VQ6b8NBHSiqYxJASnk2ZtWvv3gznpswU5`](https://explorer.solana.com/address/2CpwYjJVt33VQ6b8NBHSiqYxJASnk2ZtWvv3gznpswU5?cluster=devnet) |
| XAU capped call | 3800 to 4180, Oct 30, 1 oz | [`BvhmabWKVnDWacexfQw58U1uGbU3HLDV4AfHKquuaBbi`](https://explorer.solana.com/address/BvhmabWKVnDWacexfQw58U1uGbU3HLDV4AfHKquuaBbi?cluster=devnet) |
| XAG capped call, written | 48 to 52.8, Oct 30, 1 oz (devnet faucet sizing) | [`9d1C6cKzUMCTh7Uj5jLQmF1FNLBpbmX5xfsgTQVJRVLK`](https://explorer.solana.com/address/9d1C6cKzUMCTh7Uj5jLQmF1FNLBpbmX5xfsgTQVJRVLK?cluster=devnet) |

Example transactions: `init_config` [`3dWFtHCV…`](https://explorer.solana.com/tx/3dWFtHCVHUwB6pP9nMx1XMujMvbHgXG6ybeidaqML5npGLEUUtKuJAxJoVUBoR9sahes3tsfHnax6axYjFjLD7iQ?cluster=devnet),
`create_series` [`415veQYs…`](https://explorer.solana.com/tx/415veQYsijvk6sSHRiyMDCAUvwMvtnkWRFnpvFUiUF42URCyT8yYNuzk7BAg5oXiJyx4WTHUEMN6xa8MyNon8FU5?cluster=devnet),
`mint` [`4uLd6mpH…`](https://explorer.solana.com/tx/4uLd6mpHy7r9tJJDLsYr8Hw42ozJ8tBafnX6Y9D72fVN37pA1CNzeBPDfnp9GQtjTyovjfFGcL5fdQrFjaytDD9v?cluster=devnet), and a Pyth print posted through Wormhole and the receiver and sampled
[`268dv4W1…`](https://explorer.solana.com/tx/268dv4W1C7ioRiipd7waXEVGjSqnMqDR9KAv5AmScfY5XVAnWZ8s1GPKdAwA8NYbfC6cN1m3f12QUYApuZQtWSBn?cluster=devnet)
then [`sMLMjZd7…`](https://explorer.solana.com/tx/sMLMjZd7xpmXebmYBJ9bVDMXw8uXFFJBZ7jXw5RYj246M8ykW1Yph2wj6xFhhcwYevMyg7huWhWFVks6uFxGhVA?cluster=devnet).
Settlement signatures are added to the log after the Oct 30 expiry.

### Reproduce it

Against `https://api.devnet.solana.com`, with a funded keypair outside the repository:

```sh
export SOLANA_URL=https://api.devnet.solana.com
export METALBOOK_KEYPAIR=~/keys/devnet-authority.json      # pays and becomes admin
make pre-push && make program
solana-keygen new -o ~/keys/metalbook-program.json          # never commit it
solana program deploy target/deploy/metalbook_program.so --url "$SOLANA_URL" \
  --program-id ~/keys/metalbook-program.json --upgrade-authority "$METALBOOK_KEYPAIR"
export METALBOOK_PROGRAM_ID=$(solana-keygen pubkey ~/keys/metalbook-program.json)
alias metalbook-admin='cargo run -q --release -p metalbook-cli --bin metalbook-admin --'

metalbook-admin init-config --usdc-mint 4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU \
  --params metalbook-cli/devnet-params.json
metalbook-admin set-calendar --underlying xau --expiries 1793390400,1793998800
metalbook-admin create-series --underlying xau --kind put --strike 3800 --expiry 1793390400
METALBOOK_KEYPAIR=~/keys/writer.json metalbook-admin mint --series <SERIES> --contracts 1

# Settlement: the keeper reads Pyth prints from Hermes. The key comes only from the
# environment and is never printed or written to a file.
export METALBOOK_HERMES_API_KEY=...
cargo run --release -p metalbook-cli --bin metalbook-keeper
```

## Setup Instructions

Requirements: stable Rust (`rust-toolchain.toml`) and the Agave toolchain (`cargo build-sbf`).
MetalBook is not published on crates.io; use it from git.

```sh
git clone https://github.com/joaquinbejar/metalbook.git
cd metalbook
make program            # target/deploy/metalbook_program.so (fails on SBF frames above 4 KiB)
make quotebook-program  # quotebook's program at the pinned rev, for the demo
make test
```

As a dependency (pin a rev, as the sibling repos do):

```toml
[dependencies]
metalbook-sdk = { git = "https://github.com/joaquinbejar/metalbook", rev = "<COMMIT>" }
```

## Usage Examples

### Admin CLI

```sh
export SOLANA_URL=https://api.devnet.solana.com
export METALBOOK_PROGRAM_ID=<PROGRAM_ID>
export METALBOOK_KEYPAIR=~/keys/metalbook-devnet-authority.json   # outside the repo
alias metalbook-admin='cargo run -q -p metalbook-cli --bin metalbook-admin --'

# Config, signed by the program's upgrade authority (who becomes admin).
metalbook-admin init-config --usdc-mint <USDC_MINT> --params metalbook-cli/devnet-params.json --dry-run
metalbook-admin init-config --usdc-mint <USDC_MINT> --params metalbook-cli/devnet-params.json
metalbook-admin show-config

# Calendar and a series.
metalbook-admin set-calendar --underlying xau --expiries <T1>,<T2>
metalbook-admin show-calendar --underlying xau
metalbook-admin create-series --underlying xau --kind put --strike 2400 --expiry <T1>
metalbook-admin create-series --underlying xau --kind call --strike 2400 --cap 3000 --expiry <T1>
metalbook-admin show-series <SERIES>

# Offline: the address of some terms.
metalbook-admin series-address --underlying xau --kind put --strike 2400 --expiry <T1> --series-multiplier 1000
```

Later config changes: `show-config`, edit its `params` into a file, then
`set-config --params <FILE>` (add `--new-admin <KEY>` to hand admin over).

### SDK: a writer mints contracts

```rust
use metalbook_sdk::{ix, pda};
use metalbook_sdk::pda::SeriesSpec;

// An XAU put, strike 2400 USD, 1 oz contracts.
let spec = SeriesSpec {
    underlying: 0,              // XAU
    kind: 1,                    // put
    strike: 2_400_000_000,      // USDC atoms per ounce
    expiry,                     // a calendar expiry, Unix seconds
    cap: 0,                     // puts have no cap
    multiplier: 1_000,          // milli-ounces
};
let (series, _bump) = pda::series(&program_id, &spec);
let instruction = ix::mint(&program_id, &writer, &series, &usdc_mint, 5);
// Sign and send with any Solana client; the writer deposits 5 × max_payout.
```

`ix::close`, `ix::redeem`, `ix::claim_writer`, `ix::close_series` and `ix::close_position`
cover the rest of the writer and holder flows; `metalbook_sdk::decode::series` reads a series
account back.

### Hermes check

```sh
METALBOOK_HERMES_API_KEY=<your key> cargo run -p metalbook-keeper --example hermes -- --live
```

Fetches one XAU print and exits with status 2 and a clear message when the key is not set.

## Testing and Benchmarks

```sh
make pre-push   # fmt check, clippy -D warnings, lint and program controls, SBF build, all tests, docs
make test       # unit, property and LiteSVM tests (needs both programs built first)
make bench      # regenerate docs/benchmarks.md
```

Tests: proptest for the core math (collateral covers the maximum payout and the vault empties
exactly across random histories, prices and claim orders), LiteSVM for the program with a
negative test per validation rule, the QuoteBook demo (`metalbook-tests/tests/quotebook_demo.rs`),
the CLI against the program (`metalbook-tests/tests/cli.rs`), and Hermes fixtures (no live
calls in tests).

Compute units, measured by `make bench` in LiteSVM (sBPF v3, CPIs included, deterministic);
source: [`docs/benchmarks.md`](docs/benchmarks.md).

| Instruction | Case | CU |
| --- | --- | ---: |
| `init_config` | creates the config | 6749 |
| `set_config` | same admin | 942 |
| `set_calendar` | creates the calendar, 2 expiries | 6582 |
| `set_calendar` | replaces the list, 64 expiries | 1311 |
| `create_series` | put | 29600 |
| `create_series` | capped call | 34218 |
| `mint` | first mint, creates the position | 9246 |
| `mint` | existing position | 4829 |
| `close` | partial | 4793 |
| `close` | to zero | 4793 |
| `sample_settlement` | first print: slot 0 and fallback, expires the series | 2616 |
| `sample_settlement` | window slot and fallback | 2621 |
| `sample_settlement` | next-print link | 2505 |
| `settle` | full window (30 slots) | 3176 |
| `redeem` | 1 long token, in the money | 4062 |
| `claim_writer` | residual of 1 contract | 3190 |
| `close_position` | claimed position, rent to the writer | 356 |
| `close_series` | vault and series closed, rent to the payer | 2880 |
| `settle` | void | 2248 |

Marks and quotes are computed off-chain, so no instruction prices an option.

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

## Contribution and Contact

We welcome contributions to this project! If you would like to contribute, please follow these steps:

1. Fork the repository.
2. Create a new branch for your feature or bug fix.
3. Follow the project rules in [`rules/global_rules.md`](https://github.com/joaquinbejar/metalbook/blob/main/rules/global_rules.md)
   and run `make pre-push`, which must pass with zero warnings.
4. Make your changes and ensure that the project still builds and all tests pass.
5. Commit your changes and push your branch to your forked repository.
6. Submit a pull request to the main repository.

If you have any questions, issues, or would like to provide feedback, please feel free to contact the project maintainer:


### **Contact Information**

- **Author**: Joaquín Béjar García <https://joaquinbejar.capitaldelta.co/>
- **Email**: jb@taunais.com
- **Telegram**: [@joaquin_bejar](https://t.me/joaquin_bejar)
- **Repository**: <https://github.com/joaquinbejar/metalbook>
- **Documentation**: <https://github.com/joaquinbejar/metalbook/tree/main/docs>

We appreciate your interest and look forward to your contributions!

## ✍️ License

Licensed under **MIT** license

<!-- related-projects:start -->
## Related projects

Repositories by the same author that this project depends on, and repositories that depend on it.

### Depends on

| Repository | Description |
|------------|-------------|
| [OptionStratLib](https://github.com/joaquinbejar/OptionStratLib) · [crates.io](https://crates.io/crates/optionstratlib) | Rust library for options trading and strategy development; metalbook-maker uses it as the independent pricing reference in tests. |
| [optfix](https://github.com/joaquinbejar/optfix) | Fixed-point option pricing for Solana programs: no_std, allocation-free Black-Scholes, greeks and implied volatility, measured in compute units. |
| [quotebook](https://github.com/joaquinbejar/quotebook) | RFQ on Solana: maker quotes signed off-chain with ed25519, verified and settled atomically on-chain by a Pinocchio program. |
| [varswap](https://github.com/joaquinbejar/varswap) | Realized variance accumulated on-chain from Pyth prices, and variance swaps settled against it, on Solana; metalbook reuses its pyth-lite crate. |

### Used by

None yet.

<!-- related-projects:end -->
