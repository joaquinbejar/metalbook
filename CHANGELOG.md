# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

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
- `metalbook-keeper`: the honest settlement keeper, its Hermes price source (rate-limit aware,
  cached) and Pyth receiver posting through Wormhole (`receiver`).
- `metalbook-cli`: `metalbook-admin` (config, calendars, listing, minting, reads) and
  `metalbook-keeper` (the keeper over RPC).
- Devnet deployment (program `FPPuou5kgSp6tbwQRwKPBNhrMyf1Xb8n2ib3Yqpbsuwg`), recorded in
  `docs/devnet-log.md`.

[0.1.0]: https://github.com/joaquinbejar/metalbook/releases/tag/v0.1.0
