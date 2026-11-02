# Runbook: Hermes outage or 429 storm

The keeper reads every print from Hermes (`METALBOOK_HERMES_API_KEY`, environment only). When
Hermes fails, no new candidate reaches the chain; what is already stored stays.

## Symptoms

In the keeper log (each line names the series, or the pass):

| Line | Meaning |
| --- | --- |
| `price source failed: Hermes returned HTTP 429 (rate limited); cooling down` | The key's per-minute budget is spent. The source sends nothing for 61 s. |
| `price source failed: Hermes rate limit: cooling down, request not sent` | A request skipped during that cooldown. |
| `price source failed: Hermes rejected the API key (401)` (or 403) | The key is missing, wrong, or sent the wrong way (`METALBOOK_HERMES_AUTH`). |
| `price source failed: Hermes returned HTTP <5xx>` | Hermes itself is failing. |
| `price source failed: …` with a transport error | DNS, TLS or network. |
| `malformed price-source response (HTTP …)` | Hermes answered with a body the keeper cannot parse. |

The per-pass line counts them: `hermes N (200 a, 404 b, 429 c, other d)`. Batched (the
default), a source error stops posting for the rest of the pass and blocks any settle while a
sample it planned is missing, so the keeper never settles a series on a subset it skipped.
Unbatched (`--no-batch`), the pass stops at the series that hit the 429 and the next pass
starts there (docs/runbooks/keeper-restart.md, failure modes). `metalbook-watch` reports
`ALERT <SERIES>: N window slot(s) empty past tolerance and grace` once slots stay empty past
`slot_tolerance_s` plus 120 s; it cannot tell a Hermes outage from a Pyth one.

## What to do

1. **429s**: metalbook's share is at most 7 requests a minute across all its processes, on a
   key shared with varswap. List every metalbook process and its pacing:

   ```sh
   ps -axo pid,command | grep -E 'metalbook-(keeper|watch)' | grep -v grep
   ```

   Expect `--hermes-interval-s 12` on the primary keeper (the default) and 30 on a second
   keeper; `metalbook-watch` reads only the chain. Stop an extra process by its PID
   (`kill <PID>`), never by name. Persistent 429s with correct pacing mean another repository
   is over its share: tell the coordinator.
2. **401 or 403**: check the variable is set without printing it:

   ```sh
   test -n "$METALBOOK_HERMES_API_KEY" && echo set || echo missing
   ```

   If it is set, the key was rotated or revoked: the coordinator owns the key. Restart the
   keeper after `source ~/.zshrc` (docs/runbooks/keeper-restart.md).
3. **5xx or transport errors**: check Hermes from the same machine, without the key in the
   command line or the output:

   ```sh
   curl -s -o /dev/null -w '%{http_code}\n' 'https://hermes.pyth.network/v2/price_feeds?query=xau'
   ```

   `200` means Hermes' public metadata answers and the failure is in the price endpoints;
   anything else is a Hermes or network outage. Leave the keeper running: it retries every
   pass and backfills every slot whose canonical print exists once Hermes answers.

## Effect on settlement

A slot whose canonical print exists can be backfilled after the window, as long as no settle
happened (`docs/PLAN.md` 4.2, 4.3). With at least `min_samples` (20) slots and the rest
missing, anyone can settle on that subset from `max(window_end, window_changed) + D` (each new
slot restarts `D`); with fewer, the cascade moves to the next-print chain, then to the hard
deadline (docs/runbooks/stuck-settlement.md). Record the outage, its duration and each
series' `settle_method` and `settlement_samples` in `docs/devnet-log.md`.
