# Runbook: the weekly series roller

The roller (`metalbook-admin roll`, `docs/PLAN-phase2.md` R1) lists, for each metal, a put and
a capped call at every grid strike within ±5% of a fresh Pyth print, for the first calendar
expiry at least 3 days ahead. It runs every 6 hours as the LaunchAgent
`com.metalbook.roll.<name>` (`scripts/series-roll.sh`). Every run is idempotent: it creates only
the series that are missing, so a missed, repeated or failed run is harmless and the next one
catches up.

`metalbook-watch` names this page for two alerts: `NoNextSeries` and `CalendarLow`.

## Environment

Devnet, reading nothing secret (the Hermes key comes from `~/.zshrc` and is never printed):

```sh
export SOLANA_URL=https://api.devnet.solana.com
export METALBOOK_PROGRAM_ID=FPPuou5kgSp6tbwQRwKPBNhrMyf1Xb8n2ib3Yqpbsuwg
export METALBOOK_KEYPAIR=$HOME/.config/solana/metalbook-devnet-lister.json   # docs/keys.md
```

## Install, status, logs

```sh
cargo build --release -p metalbook-cli
METALBOOK_ROLL_ARGS="--min-expiry 1795208400" sh scripts/series-roll.sh install devnet
sh scripts/series-roll.sh status devnet
tail -50 ~/.local/share/metalbook-roller/devnet/roll.log
```

`install` copies the script and the binary to `~/.local/share/metalbook-roller/devnet/`, runs
one dry run (expect a `dry run, nothing sent` line per metal, or a skip), then loads the agent.
`--min-expiry 1795208400` (Nov 20) kept the first install off the Nov 13 series measured by R3;
reinstall without it once Nov 13 has settled. `uninstall devnet` stops it; the log and the
binary stay.

A run's lines, by case:

| Line | Meaning | Action |
| --- | --- | --- |
| `quiet period: ... within 7200 s of expiry ...` | Within 2 h of an expiry: the keepers own the Hermes budget | None |
| `<metal>: ... N new` then `confirmed <signature>` per series | Listed | Record new expiries in `docs/devnet-log.md` |
| `<metal>: ... 0 new` | Everything already listed | None |
| `<metal>: hermes budget busy; skipped` | Every metalbook Hermes slot was taken (keepers backfilling) | None; the next run retries |
| `<metal>: the latest Pyth print is N s old ... skipped` | Market closed (Friday 17:00 to Sunday 18:00 New York, the daily pause, holidays) | None |
| `<metal>: no calendar expiry at least ... ahead` | The calendar ran out | Extend it (below) |
| `(bound: --max-per-expiry 18 reached ...)` | 18 series of that metal already at the expiry | None, unless spot moved far: then decide by hand |
| `roll failed (exit N)` with `Hermes read failed` | A 429 or a Hermes error | `docs/runbooks/hermes-outage.md` |
| `roll failed` with an RPC error | The RPC failed | `docs/runbooks/rpc-outage.md` |
| `error: ... config multiplier is ...` | `set_config` changed a metal's size | Nothing is listed until someone decides; reinstall with `--multiplier-milli-oz` only on purpose |

## `NoNextSeries`

A metal has no active 1 oz series expiring at least 24 h ahead while the market is open. A
maker has nothing to quote next week.

1. `sh scripts/series-roll.sh status devnet`: loaded, and the last exit code. If not loaded,
   install it (above).
2. Read the end of `roll.log` and act on the line (table above).
3. Run once by hand, dry run first:
   ```sh
   ~/.local/share/metalbook-roller/devnet/bin/metalbook-admin roll
   ~/.local/share/metalbook-roller/devnet/bin/metalbook-admin roll --execute
   ```
4. If the roller cannot run, `metalbook-admin list-week --underlying <xau|xag> --execute` lists
   the same shape by hand.

The alert resolves on the watcher's next check once a series is listed. It ignores holidays: on
a holiday it can fire with the market closed, and step 3 then skips with a stale print.

## `CalendarLow`

A metal's calendar has fewer than 2 future expiries. The roller never calls `set_calendar`:
the admin extends it by hand.

1. Choose the dates: Fridays 16:00 New York, skipping holidays and early closes (as Nov 27,
   Dec 25 and Jan 1). A calendar holds at most 64.
2. `set_calendar` **replaces** the list, so pass every expiry to keep: the future ones already
   there and the new ones, strictly increasing. Past expiries can be dropped, since listed
   series keep their own expiry.
3. With the admin key (`docs/keys.md`), dry run first, then send and read back:
   ```sh
   metalbook-admin --keypair ~/.config/solana/metalbook-devnet-admin.json set-calendar \
     --underlying xau --expiries <E1>,<E2>,... --dry-run
   metalbook-admin --keypair ~/.config/solana/metalbook-devnet-admin.json set-calendar \
     --underlying xau --expiries <E1>,<E2>,...
   metalbook-admin show-calendar --underlying xau
   ```
   The same for `xag`.
4. Record the signatures and the new list in `docs/devnet-log.md`.

## Cost

Each new series pays the rent of its `Series`, its vault and its long mint from the lister key;
`close_series` returns the first two to it once the series owes nothing. Keep the lister funded
as for any payer (`docs/runbooks/low-balance.md`).
