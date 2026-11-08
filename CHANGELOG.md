# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- A test of the devnet key separation: after the admin is handed over with the parameters
  unchanged and the upgrade authority moves to a third key, the old key is rejected by
  `set_config` and `set_calendar`, the upgrade key is not the admin, and the new admin works.
- `metalbook-admin roll`, the weekly series roller (`docs/PLAN-phase2.md`, R1): for each metal,
  the `list-week` shape for the first calendar expiry at least `--min-lead-s` ahead (default 3
  days, never closer than `min_listing_s`, not before `--min-expiry`), creating only the
  missing series, nearest spot first, at most `--max` (28) a run and `--max-per-expiry` (18)
  per metal and expiry. Nothing within 2 h of any calendar expiry (before any Hermes request);
  refused unless the config multiplier is `--multiplier-milli-oz` (1 oz); a stale print or a
  busy Hermes budget is a skip, any other read error fails that metal with nothing sent.
  `scripts/series-roll.sh run|install|uninstall|status <name>` runs it as a LaunchAgent every
  6 h, with the Hermes key from `~/.zshrc` only, the binary copied out of the checkout and the
  log rotated at 10 MB (5 kept); `scripts/tests/series-roll.sh` tests it offline
  (`make scripts-test`, now part of `make pre-push`). Runbook: `docs/runbooks/series-roll.md`.
- A shared Hermes limiter for every metalbook process on a host
  (`metalbook_keeper::HermesLimiter` and `LimitedHttp`, ported from varswap's I6 limiter): at
  most 7 requests in any 60 s window, files `metalbook-hermes-<hash of the key>` apart from
  varswap's, a 429 blocking all of them for 61 s, slots held by a kernel `flock` and freed only
  when released or when their owner dies. The keeper and `list-week` wait for a slot, the
  roller only tries (`KeeperError::BudgetBusy`).
- `metalbook-watch` alerts `NoNextSeries` (while the market is open, a metal has no active 1 oz
  series expiring at least 24 h ahead) and `CalendarLow` (fewer than 2 future expiries), when
  it discovers every series.

### Changed

- Devnet keys (H3, part 2): one key per role since 2026-11-07 (upgrade authority, admin,
  keeper payer, lister), recorded in `docs/keys.md` and `docs/devnet-log.md`; the keeper restart
  and low balance runbooks use the new keys.
- metalbook-maker uses optfix 0.3 (0.3.0 is `max_stable_version` on crates.io, checked
  2026-11-07; bus 47754, after the Nov 6 drill). Its breaking changes, `rho` in `Greeks` and
  `Greeks` and `Priced` becoming `#[non_exhaustive]`, need no code change: the maker reads
  their fields and never builds them, and it uses neither `implied_vol` (the only result
  that may move) nor rho. `price_and_greeks` and `price_error_bound` are bit-identical to
  0.2, so marks and quotes are unchanged. No crates.io release: `metalbook-core` and
  `metalbook-sdk` do not depend on optfix.

## [0.1.1] - 2026-11-07

Patch release. `metalbook-core` and `metalbook-sdk` have no code change since 0.1.0; their
crates.io page now shows the current README. Everything below is in the repository's binaries
and documentation. The program is unchanged and not redeployed.

### Added

- Runbooks (H4): Hermes outage, RPC outage, low payer balance, orphaned encoded VAAs, a
  stuck settlement (the cascade step by step), program upgrade and rollback, and a compromised
  key of each role, with an index in `docs/runbooks/README.md`; `docs/audit-scope.md` (files
  with line counts, trust assumptions, invariants, off-chain scope, known limitations).
  `show-series` prints a `candidates` block (window slots, next-print links, fallback, their
  change times, the window end and the hard deadline), and `metalbook-watch` alerts name the
  matching page (low balance, leftovers, an overdue settle).
- `docs/keys.md` (H3, part 1): the four roles, the devnet separation steps that run after the
  Nov 6 drill, and the mainnet procedure (Squads v4 multisig with a time lock, hot keeper
  payers, the final step). Tests: after an admin rotation `set_calendar` rejects the old admin,
  and a forged `Config` is rejected by `set_calendar`.
- Verifiable build (H2): `make verifiable` builds the program with solana-verify 0.5.2 in the
  verifiable-build image pinned by digest; `make verifiable-check` compares the hash with
  `docs/verifiable-build.md`, and `make bench-verifiable` measures that binary into
  `docs/benchmarks-verifiable.md`. The build's log goes through the stack-frame check of
  `make program`, and `make verifiable-control` proves it and the exit status are enforced. The
  CI workflow `verifiable.yml` builds it twice in clean clones, checks the hash and the CU
  table, and uploads the binary. Devnet already runs that binary (hash `62718c46…`).
- cargo-fuzz targets `settlement_model`, `sample_settlement` and `ledger_sequence` (`fuzz/`,
  its own workspace, nightly), with an independent settlement and payoff model and a
  `PriceUpdateV2` decoder as acceptance oracle; committed regressions replayed on stable by
  `metalbook-tests/tests/fuzz_regressions.rs`; deterministic fault controls
  (`make fuzz-control`); a nightly fuzz workflow. `arbitrary` 1.5 is a new dev-dependency of
  metalbook-tests.
- `metalbook-watch`, chain-only alerts against the schedule: overdue slots (oracle
  availability unknown), settle overdue, void pending, low payer balance, leftover encoded
  VAAs, silent keepers, an unreadable heartbeat channel. Each new alert goes to the ACS
  channel `metalbook-keepers` and to the coordinator by DM, and each resolved one to the
  channel. Keeper heartbeats (`--heartbeat --keeper-name`) are posted on state change and
  otherwise every 10 minutes. `docs/mainnet-readiness.md` lists the pre-mainnet gates, host
  independence among them.
- Batched keeper passes (`Keeper::pass`; the keeper's default, `--no-batch` for the old path).
  Every series in its window is planned, the candidates are grouped by exact signed print and
  revalidated with the program's own rule just before sending. Each print is posted once for
  up to 10 series, the most the post's last transaction carries (`RpcChain::batch_capacity`,
  measured: 1,213 bytes and 60,380 CU at 10 series). Uncertain posts block only their series'
  settle for the pass; rejected chunks are resent once without the members that already hold
  their candidate, then posted one by one. Settles are batched too.
- `metalbook-keeper --exclude <SERIES>` (repeatable) and `--hermes-interval-s` (default and
  floor 12 s: metalbook's share of the Hermes key is 7 requests a minute across its
  processes).
- `metalbook-admin settle --series` and `close-series --series` (the void drill: `close-series`
  creates the rent payer's USDC account first, as the program requires it).
- `metalbook-admin list-week` (a put and a capped call at every grid strike within ±5% of a
  Pyth print at most 5 minutes old, for one calendar expiry at least `min_listing_s` ahead;
  existing series skipped; at most `--max` new; dry run unless `--execute`) and `reclaim`
  (`close-series` on every settled or voided series that owes nothing; dry run unless
  `--execute`).
- Per-pass keeper metrics: transaction attempts (confirmed, failed), attempted serialized
  bytes and the pass's largest, Hermes requests by status (`metalbook_cli::metrics`).

### Changed

- `metalbook-keeper` no longer exits when the series list or the clock cannot be read (a
  rate-limited `getProgramAccounts`, an RPC timeout): it prints `pass N: skipped, the chain
  could not be read` and retries at the next pass. `--once` still exits with the error.
- The keeper's Hermes pacing moves from 10 s to 12 s by default.

- The keeper uses pyth-post 0.1.1: its payer lock is `pyth_post::PayerLock`
  (`payer-<address>.lock` in `$PYTH_POST_LOCK_DIR`, default `~/.local/state/pyth-post`), shared
  with every pyth-post poster on the machine, instead of a metalbook lock in the temporary
  directory; and its orphan scan is `pyth_post::LeftoverScanner`, which closes an encoded VAA
  only after two scans at least 300 blocks apart have seen it unchanged.
- metalbook-maker uses optfix 0.2 (0.2.0 is `max_stable_version` on crates.io, checked
  2026-11-01). Its only breaking change, `PricingError` becoming `#[non_exhaustive]`, needs no
  code change: the maker maps every `PricingError` to `MakerError` without matching variants.
  Marks and quotes are unchanged. No crates.io release: `metalbook-core` and `metalbook-sdk` do
  not depend on optfix.
- README: the Nov 6 void and settlement on devnet, the keeper's batching and RPC chain, every
  `metalbook-admin` command, `metalbook-watch`, the verifiable build and fuzzing; the Hermes
  bearer scheme is recorded as verified (also in `docs/devnet.md`).

## [0.1.0] - 2026-10-30

First release. The crates.io packages are `metalbook-core` and `metalbook-sdk`; the program,
the pricing adapter, the keeper and the CLI are used from the repository.

### Added

- `metalbook-core`: `no_std`, integer-only domain model for cash-settled European options on
  XAU/USD and XAG/USD settled in USDC: unit newtypes, series terms, payoff and collateral (puts
  by strike, calls capped), the vault ledger of a series, the settlement rules (canonical
  window slots, the next-print chain, the fallback and the cascade with challenge delay `D`),
  `accept_print`, and the on-chain account layouts shared by every client.
- `metalbook-program`: the Pinocchio 0.11 program: config, calendars, listing, mint and close,
  settlement sampling from Pyth `PriceUpdateV2` accounts, settle and void, redeem, writer
  claims, and rent recovery for series and positions.
- `metalbook-sdk`: instruction builders for every instruction, PDA helpers and account
  decoders.
- `metalbook-maker`: payoff-aware marks and two-sided quotes behind a `VanillaPricer` trait,
  priced by optfix (`OptfixPricer`), and quotebook quote terms.
- `metalbook-keeper`: the honest settlement keeper and its Hermes price source (cached,
  paced, rate-limit aware; a miss holds only while nothing newer exists).
- `metalbook-cli`: `metalbook-admin` (config, calendars, listing, minting, redeem, writer
  claims, reads) and `metalbook-keeper` (the keeper over RPC, posting through Wormhole and the
  Pyth receiver with `pyth-post`, one process per payer).
- Devnet deployment (program `FPPuou5kgSp6tbwQRwKPBNhrMyf1Xb8n2ib3Yqpbsuwg`), recorded in
  `docs/devnet-log.md`.

[0.1.1]: https://github.com/joaquinbejar/metalbook/releases/tag/v0.1.1
[0.1.0]: https://github.com/joaquinbejar/metalbook/releases/tag/v0.1.0
