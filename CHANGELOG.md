# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Verifiable build (H2): `make verifiable` builds the program with solana-verify 0.5.2 in the
  verifiable-build image pinned by digest; `make verifiable-check` compares the hash with
  `docs/verifiable-build.md`, and `make bench-verifiable` measures that binary into
  `docs/benchmarks-verifiable.md`. The CI workflow `verifiable.yml` builds it twice in clean
  clones, checks the hash and the CU table, and uploads the binary.
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

[0.1.0]: https://github.com/joaquinbejar/metalbook/releases/tag/v0.1.0
