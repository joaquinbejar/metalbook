# Audit scope

What an audit of metalbook covers, what it may assume, and what it should try to break.
Written for milestone H4 of `docs/PLAN-improvements.md`; the design is `docs/PLAN.md`.

## In scope: on-chain code

Line counts at commit `8cab18a` (2026-11-02); "code" leaves out the `#[cfg(test)]` modules
at the end of a file.

| File | Lines | Code | What it does |
| --- | ---: | ---: | --- |
| `metalbook-program/src/lib.rs` | 115 | 115 | Entrypoint and instruction dispatch |
| `metalbook-program/src/data.rs` | 129 | 104 | Instruction data parsing |
| `metalbook-program/src/validate.rs` | 178 | 178 | Account checks: owner, PDA, signer, writable, mint, vault, token accounts |
| `metalbook-program/src/pda.rs` | 107 | 107 | Canonical PDAs and bumps |
| `metalbook-program/src/state.rs` | 40 | 40 | Account loading |
| `metalbook-program/src/error.rs` | 185 | 165 | Custom error codes 100 to 147 |
| `metalbook-program/src/processor/admin.rs` | 357 | 357 | `init_config` (against `ProgramData`), `set_config`, `set_calendar` |
| `metalbook-program/src/processor/series.rs` | 313 | 293 | `create_series`: snapshot of every term, long mint, vault |
| `metalbook-program/src/processor/position.rs` | 322 | 322 | `mint` and `close` (collateral in, long tokens out, and back) |
| `metalbook-program/src/processor/settlement.rs` | 165 | 165 | `sample_settlement` (Pyth `PriceUpdateV2` through pyth-lite) and `settle` |
| `metalbook-program/src/processor/claims.rs` | 159 | 159 | `redeem` and `claim_writer` |
| `metalbook-program/src/processor/close.rs` | 163 | 163 | `close_series` (donation sweep, rent to the recorded payer) and `close_position` |
| `metalbook-program/src/processor/mod.rs` | 8 | 8 | |
| `metalbook-core/src/accounts.rs` | 530 | 511 | Account layouts (`Config`, `Calendar`, `Series`, `Position`) |
| `metalbook-core/src/ledger.rs` | 555 | 381 | Vault accounting and its exact identity |
| `metalbook-core/src/settlement.rs` | 1217 | 745 | Candidate acceptance (slots, next-print chain, fallback) and the settle cascade |
| `metalbook-core/src/payoff.rs` | 274 | 148 | Put and capped-call payoff, maximum payout |
| `metalbook-core/src/series.rs` | 311 | 218 | Series terms and their validation |
| `metalbook-core/src/units.rs` | 255 | 201 | Price and USDC unit conversions |
| `metalbook-core/src/error.rs` | 127 | 79 | Core errors |
| `metalbook-core/src/lib.rs` | 41 | 41 | |
| `metalbook-sdk/src/ix.rs` | 392 | 392 | Instruction builders (account order and data the program expects) |
| `metalbook-sdk/src/pda.rs` | 78 | 78 | Client PDAs, which must match the program's |
| `metalbook-sdk/src/decode.rs` | 63 | 63 | Account decoding for clients |
| `metalbook-sdk/src/lib.rs` | 32 | 32 | |

Dependencies the program trusts, reviewed as used rather than audited: pinocchio 0.11,
pinocchio-system 0.6, pinocchio-token 0.7, pinocchio-associated-token-account 0.4, and
pyth-lite 0.1 (the `PriceUpdateV2` decoder; `sample_settlement` checks the account's owner is
the configured receiver and the update is fully verified). The deployed binary is the
verifiable build of `docs/verifiable-build.md`.

## Trust assumptions

- **An honest keeper within `D`** (`docs/PLAN.md` 4.3): at least one keeper posts, within the
  challenge delay of any change, every available candidate of higher priority than the one
  that would otherwise settle. Without it, anyone may post a favorable subset (at least
  `min_samples` slots, or any subset after the hard deadline), wait `D` and settle on it.
- **Pyth and the Wormhole guardians**: a `PriceUpdateV2` owned by the configured receiver and
  fully verified carries a price the guardian set signed. The program checks the owner, the
  verification level, the feed id, the pinned exponent, the confidence bound and the
  canonical-print rules; it cannot check that the price is right.
- **The admin's calendar and parameters**: the admin decides which expiries can be listed and
  the terms of series listed afterwards (feed ids and receiver included). A live series reads
  only the copy taken at listing (`docs/PLAN.md` 4.5), so the admin cannot change it.
- **The upgrade authority** can replace the code; on mainnet it sits behind a multisig with a
  time lock (`docs/keys.md`).

## Invariants to break

- **The ledger identity** (`metalbook-core/src/ledger.rs`): before settlement the owed vault
  equals open contracts times the maximum payout per contract; after it, longs outstanding
  times the payout plus unclaimed writer contracts times the residual. The vault's token
  balance is never below the owed amount, and `close_series` closes only at zero owed.
- **Settlement only through the cascade** (`docs/PLAN.md` 4.3): a series settles on a full
  window, a partial window of at least `min_samples`, a complete next-print chain, then after
  the hard deadline a partial chain, a partial window, the fallback, or void, each at its own
  earliest time and each restarting its `D` on a new candidate. No candidate that is not the
  canonical print of its slot, link or lookback can be stored, and no step can be skipped
  while a higher one is pending.
- **No payout above the maximum**: a long token's payout plus a writer contract's residual
  equals the maximum payout per contract (the strike for a put, cap minus strike for a capped
  call), so no settlement pays out more than was deposited.
- **Only owners move their money**: `redeem` pays the holder who burns, `claim_writer` the
  writer of the position, `close_series` sends only a donation (balance above the owed
  amount) to the recorded rent payer; no instruction has a privileged destination.

The fuzz targets of `fuzz/` (H5) and the LiteSVM tests encode these; a counterexample to any
of them is in scope.

## Off-chain components, scoped separately

- **Keeper and CLI** (`metalbook-keeper`, `metalbook-cli`): they cannot move user funds through
  the program; the program checks every candidate and every settle. They do sign with hot
  keys that hold SOL and, on devnet today, every authority (`docs/keys.md`, H3). A bug or a
  compromise can spend the payer's SOL, leak the Hermes key (read only from the environment
  and redacted from every error) or leave orphaned encoded VAAs. A keeper that stops posting
  lets a subset settle under the trust assumption above.
- **Maker** (`metalbook-maker`): it sizes quotes and signs QuoteBook fills from its own wallet;
  its risk is the maker's inventory, not the protocol's.
- These are reviewed for key handling and redaction, not audited as on-chain code. Their
  operations are in `docs/runbooks/`.

## Known limitations

- **The long mint's rent** (about 0.0015 SOL per series) stays locked: SPL Token cannot close a
  mint (`docs/PLAN.md` 4.6).
- **Quoting during the window**: from the window's start part of the settlement average is
  known, so the maker stops quoting or widens to the known part's bounds (`docs/PLAN.md` 4.7);
  a third-party maker that keeps quoting gives that information away.
- **Claimable forever**: `redeem` and `claim_writer` have no deadline, so a series whose last
  holder never redeems is never closed and its rent stays with it (`docs/PLAN.md` 4.6).
