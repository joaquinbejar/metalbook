# Runbook: a settlement that does not happen

`settle` is permissionless: the keeper calls it when a step of the cascade allows it, and
anyone else may. A series past its window and still not settled is either waiting, correctly,
for a step's earliest time, or missing candidates.

## Symptoms

- `metalbook-watch`: `ALERT <SERIES>: not settled, overdue since <T>` (past `window_end + D`
  plus 600 s of grace), or `ALERT <SERIES>: past hard_deadline + D since <T>, neither settled
  nor voided`.
- `metalbook-admin settle --series <SERIES>` fails with custom error **141**
  (`SettlementNotReady`).

## Read the series

```sh
metalbook-admin show-series <SERIES>
```

The `candidates` block holds what the cascade reads (times are Unix seconds; `date -u -r <T>`
prints one on macOS):

```json
"candidates": {
  "window_slots": 24, "window_slots_total": 30, "min_samples": 20, "window_changed": 1794000300,
  "next_print_links": 5, "k_next_print": 5, "next_print_changed": 1793999500,
  "fallback_time": 1793998795, "fallback_changed": 1793998800,
  "window_end": 1793998800, "challenge_delay_s": 600, "hard_deadline": 1794603600
}
```

## Which step applies, and from when

`settle` takes the first step whose candidates exist; a step whose candidates exist but whose
time has not come returns 141 rather than falling through to a lower step (`docs/PLAN.md`
4.3). With `D = challenge_delay_s`:

| Step | Needs | Settles from | `settle_method` |
| --- | --- | --- | --- |
| 1. Window, complete | `window_slots == window_slots_total` | `window_end` | `window` |
| 1. Window, partial | `window_slots >= min_samples` | `max(window_end, window_changed) + D` | `window` |
| 2. Next-print chain | `next_print_links == k_next_print` | `window_end + D` | `next_print` |
| 3. Partial chain | `1 <= next_print_links < k` | `max(hard_deadline, next_print_changed) + D` | `partial_next_print` |
| 3. Partial window | `1 <= window_slots < min_samples` | `max(hard_deadline, window_changed) + D` | `partial_window` |
| 4. Fallback | `fallback_time != 0` | `max(hard_deadline, fallback_changed) + D` | `fallback` |
| 5. Void | nothing above | `hard_deadline + D` | `void` (docs/runbooks/void.md) |

Each new slot, link or fallback restarts its own `D`: a keeper still backfilling keeps a
partial window from settling, by design, so honest keepers can complete it.

## What to do

1. **141 before the time in the table**: correct. Wait, or let the keeper settle it on its
   next pass after that time.
2. **141 after the time**: read `show-series` again; a candidate added since then restarted
   `D` (the matching `*_changed` moved). Settle after the new time.
3. **Window partial below `min_samples` and no chain**: the series waits for the hard
   deadline (7 days on devnet). Check that the keeper is running and posting (its log shows
   `slots +N` and `links +N`; docs/runbooks/keeper-restart.md). Slots whose canonical print
   exists can still be posted until someone settles; a slot with no canonical print (a market
   halt, docs/PLAN.md 4.4) stays empty.
4. **Settle by hand** once the time has come, signed by any funded key:

   ```sh
   metalbook-admin --keypair <PAYER.json> settle --series <SERIES>
   ```

   Expect `confirmed <SIGNATURE>`, then `show-series` with `"state": "settled"` (or
   `"voided"`), the `settle_method`, `settlement_price_usd` and `settlement_samples`. Error
   116 (`SeriesAlreadySettled`) means someone settled it first: read it with `show-series`.
5. **Record** in `docs/devnet-log.md` any series that settled on a step other than a complete
   `window`, with the reason.

Nobody can force a lower step early, including the admin: the cascade's times are copied into
the series at listing (`docs/PLAN.md` 4.5).
