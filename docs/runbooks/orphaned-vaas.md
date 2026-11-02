# Runbook: orphaned encoded VAAs

Each post creates an encoded VAA account (the Wormhole message, about 0.003 SOL of rent) in
the Pyth receiver, verifies it, posts the price update and closes the VAA. A post interrupted
after creating it (a failed transaction, an uncertain outcome, a keeper stopped mid-post)
leaves it open: the rent stays locked, nothing else is at risk.

## Symptoms

- Keeper: `closing N orphaned encoded VAA(s)` (the leftover scan found and closed them).
- `metalbook-watch --payer <PAYER>`: `ALERT payer <PAYER>: N leftover encoded VAA(s)`, then
  `RESOLVED leftovers:<PAYER>` after the keeper closes them.
- The payer's balance drifting down by about 0.003 SOL per orphan
  (docs/runbooks/low-balance.md).

## How they are closed

The keeper scans for encoded VAAs whose write authority is its payer at startup and every 10
passes. A VAA is closed only after two scans at least 300 blocks apart have seen it unchanged,
so an in-flight post of the same payer is never closed under it. With the default
`--interval-s 60`, expect `closing N orphaned encoded VAA(s)` about 10 minutes after the
orphan appears, then the rent back on the payer.

## What to do

1. **Usually nothing**: keep the keeper running and check the alert resolves within about 20
   minutes.
2. **The keeper is stopped for good**: start it on any series list (an explicit `--series`
   with a settled series is enough) and let it run for at least two scans (about 20 minutes at
   `--interval-s 60`), then stop it by PID (docs/runbooks/keeper-restart.md). `--once` runs one
   scan only, which never closes anything.
3. **Two keepers sharing one payer** close each other's in-flight VAAs (seen on devnet
   2026-10-30: three posts failed with the receiver's `WrongVaaOwner` and were retried). The
   payer lock refuses a second keeper on the same machine; across machines, give each keeper
   its own payer (`docs/keys.md`).
4. **The count keeps growing**: posts are failing after creating their VAA. Read the keeper's
   errors (`<SERIES>: …`) for the cause, usually an RPC or balance problem
   (docs/runbooks/rpc-outage.md, docs/runbooks/low-balance.md).

Record the number closed and the rent recovered in `docs/devnet-log.md` when it follows an
incident.
