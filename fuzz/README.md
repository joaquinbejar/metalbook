# Fuzz targets

cargo-fuzz targets for the settlement and ledger paths (`docs/PLAN-improvements.md`, milestone
H5). This crate is its own workspace, so the repository workspace never builds it on stable; it
needs a nightly toolchain and `cargo install --locked cargo-fuzz`. The harness logic is in
`fuzz/harness/`, shared with the stable replay test `metalbook-tests/tests/fuzz_regressions.rs`.

| Target | Code under test | Invariants |
| --- | --- | --- |
| `settlement_model` | `metalbook_core::SettlementState::{add_print, decide, restore}` on arbitrary oracle terms and sequences of print offers and clock steps | `add_print` stores exactly what the independent model of `harness/model.rs` stores, or rejects exactly when it does; candidates only improve; each kind's change time (its `D`) moves only when that kind changed; `restore(parts)` is the identity; `decide` equals the model's cascade, never settles a lower step while a higher one exists, and its price lies within the bounds of the prints it averages |
| `sample_settlement` | the program's `sample_settlement` and `settle` in LiteSVM, on seeded valid `PriceUpdateV2` accounts, the same with one field mutated (verification level, feed, price, conf, exponent, times, discriminator, length) and raw bytes, owned by the receiver or forged owners, through the whole series life | an acceptance oracle (`harness/pyth.rs`, a decoder written from the layout, not pyth-lite, and `harness/model.rs`) predicts accept or reject before each transaction; any disagreement either way is a crash, so a program that rejects everything fails too; on accept the series equals the prediction byte for byte; on reject the error is exactly the predicted `MetalbookError` code and the series is byte-identical; the price update, vault and long mint never change; the fee payer pays exactly the fee |
| `ledger_sequence` | arbitrary `mint`, `close`, long transfers, vault donations, samples, `settle`, `redeem`, `claim_writer`, `close_series`, `close_position` with clock warps, three writers and two holders, on an XAU put, an XAU capped call or an XAG capped call at 50 oz; every case ends with a draining suffix (settle or void, redeem everything, claim everything, close the series and the positions) | rollback: a failing transaction leaves every account it can touch byte-identical, the fee payer down by exactly the fee; conservation while open: vault balance = `vault_owed` + donations, long supply = `longs_outstanding`, positions add up to `writers_unclaimed`, USDC neither created nor destroyed; redeem pays `n × payout` and claim `written × residual` from the payoff model; `close_series` sweeps exactly the donations and returns exactly the rent; after the suffix every owed atom is paid and nothing stays open |

```bash
make program                          # the LiteSVM targets load target/deploy/metalbook_program.so
make fuzz FUZZ_SECONDS=120            # every target on its corpus (fuzz/corpus/<target>)
cargo +nightly fuzz run -O ledger_sequence -- -max_total_time=3600
make fuzz-control                     # the deterministic fault controls
cargo test -p metalbook-tests --test fuzz_regressions   # the committed regressions, on stable
```

The harness asserts by panicking: it is test code. The fee model is LiteSVM's: 5,000 lamports
per signature, charged on failed transactions too.

## Regressions

`fuzz/regressions/<target>/` holds committed inputs; `make test` replays every one on stable
(decoded with `arbitrary_take_rest`, as libFuzzer's typed entry does), together with
hand-built inputs that make each harness take its accepting paths (a full window and a
settlement through the program, every cascade step, each series lifecycle). A crash input
found by fuzzing goes there with its fix, together with `<name>.debug`, the `{:#?}` rendering
of what it decodes to (write it with `METALBOOK_FUZZ_BLESS=1 cargo test -p metalbook-tests
--test fuzz_regressions`). `arbitrary` decodes any byte string into some input, so without the
rendering a change to the input types would quietly turn a committed crash into a harmless
input; with it, the replay fails with the file name. Blessing after such a change shows, in
the diff, what each regression now means: re-minimize the crash if it no longer reproduces.

## Fault controls

`fuzz/controls/<name>.patch` plants one bug and `<name>.input` is a committed input that
exposes it (found by fuzzing the patched code). `make fuzz-control` (`fuzz/controls/run.sh`)
copies the working tree to `target/fuzz-control/run`, runs each input on the unpatched code (it
must pass), applies the patch (rebuilding the program) and runs the input again (it must
panic).

| Control | Target | Planted bug |
| --- | --- | --- |
| `core_accepts_non_advancing_link` | `settlement_model` | the next-print chain accepts a link whose `publish_time` does not advance past the last one |
| `program_skips_confidence` | `sample_settlement` | the program passes a zero confidence to the series' bound, so a print wider than `max_conf_bps` is accepted |
| `close_series_skips_donation_sweep` | `ledger_sequence` | `close_series` does not sweep donations out of the vault (closing the vault then fails) |

## Measured (Studio, Apple silicon, 2026-11-02)

One hour per target (`cargo +nightly fuzz run -O <target> -- -max_total_time=3600`, nightly
2026-10-29, cargo-fuzz 0.13.2), all three at once (the first two shared the machine with the
third):

| Target | Executions | Executions/s | Coverage (edges) | Corpus | Outcome |
| --- | ---: | ---: | ---: | ---: | --- |
| `settlement_model` | 165,858,707 | 46,059 | 577 | 737 | no finding |
| `sample_settlement` | 1,044,324 | 290 | 8,922 | 1,061 | no finding |
| `ledger_sequence` | 136,343 | 37 | 9,202 | 1,123 | no finding in the program; one harness bug (below) |

`ledger_sequence` is slow by design: each case clones the LiteSVM, runs up to 24 operations
and then the whole draining suffix (up to 30 window samples, a settle, every redeem and claim,
two closes). Its first hour stopped after 7,861 runs on a harness bug: two equal donations
were the same transaction twice for LiteSVM (`AlreadyProcessed`). The harness now takes a fresh
blockhash per donation, the input is in `fuzz/regressions/ledger_sequence/equal-donations`,
and the figures above are the second, clean hour.
