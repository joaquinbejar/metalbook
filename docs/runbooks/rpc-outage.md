# Runbook: RPC outage

Every metalbook process talks to one JSON-RPC endpoint (`--url`, or `SOLANA_URL`; never
printed, so a provider key in it stays private). Devnet uses the public
`https://api.devnet.solana.com`, which rate-limits `getProgramAccounts` (HTTP 429).

## Symptoms

| Where | Line | Meaning |
| --- | --- | --- |
| Keeper | `pass N: skipped, the chain could not be read: …` | The series list or the clock could not be read; the pass is skipped and the next one retries (#52; before it, the keeper exited with `error: RPC: …`). |
| Keeper | `<SERIES>: chain operation failed: …` | One series' read or transaction failed; the other series go on. |
| Keeper | `orphan scan skipped: …` | The leftover scan could not list the payer's encoded VAAs; it runs again 10 passes later. |
| Watch | `clock: …` or `series: …` | `metalbook-watch` cannot evaluate any alert this round and retries at the next; under `--once` it exits with `error: RPC: …`. |
| Admin | `error: RPC: …` | One command failed; nothing was sent, or the result is unknown (below). |

## What to do

1. **Tell an outage from a rate limit**, without printing the URL if it carries a key:

   ```sh
   curl -s -o /dev/null -w '%{http_code}\n' -H 'content-type: application/json' \
     -d '{"jsonrpc":"2.0","id":1,"method":"getSlot"}' https://api.devnet.solana.com
   ```

   `200` with the keeper still failing on `getProgramAccounts` is the rate limit: restart the
   keeper with an explicit `--series` list (one `--series <SERIES>` per series in a window),
   which reads accounts one by one instead of listing the program, and run
   `metalbook-watch` with `--series` for the same reason. `429` or no answer is an outage.
2. **Outage**: leave the keeper running. It retries every pass and resumes posting where the
   chain left off; nothing is lost while it cannot send (see the effect below). If another
   endpoint is available, restart the keeper on it (docs/runbooks/keeper-restart.md) with the
   same flags and a new `--url`, stopping the old process by PID first: two keepers on one
   payer are refused by the payer lock.
3. **A transaction whose outcome is unknown** (a timeout after sending): check the account it
   was meant to change before sending again. `metalbook-admin show-series <SERIES>` shows
   whether a settle or close landed; the keeper does this itself (a post with an uncertain
   outcome blocks re-posting until its blockhash expires, then the next pass re-reads the
   series).

## Effect on settlement

The same as a Hermes outage (docs/runbooks/hermes-outage.md): slots whose canonical print
exists can be posted late, until someone settles. An outage longer than `D` (600 s on devnet)
during a window lets anyone with RPC access settle a subset of at least `min_samples` slots
(`docs/PLAN.md` 4.3, trust assumption). Record the outage and each affected series'
`settle_method` and `settlement_samples` in `docs/devnet-log.md`.
