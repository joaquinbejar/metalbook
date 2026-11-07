# Runbook: low payer balance

A keeper payer pays every post: about 0.000025 SOL in fees (5 signatures) and temporary rent
for the encoded VAA and the price update, returned when the post closes them. A lister pays
each series' rent (returned at `close_series`). The alarm is 0.5 SOL.

## Symptoms

- `metalbook-watch --payer <PAYER>`: `ALERT payer <PAYER>: <N> lamports, under the alarm`
  (`--low-balance`, default 500000000 lamports), then `RESOLVED balance:<PAYER>` once funded.
- Keeper, with an empty payer: `<SERIES>: chain operation failed: …` on every post, with the
  RPC's insufficient-funds message; nothing posts, so the series settle only if someone else
  posts (docs/runbooks/hermes-outage.md, effect on settlement).

## Check

```sh
solana balance -u devnet <PAYER>
```

Expect at least `0.5 SOL`. A balance falling faster than about 0.0001 SOL per post means rent
is not coming back: look for leftovers (docs/runbooks/orphaned-vaas.md).

## Fund

From a funded key that holds no role it would lose by paying (on devnet, the old key `77ie…`,
which holds no role since 2026-11-07, `docs/keys.md`; announce on the bus before signing with
it, since other projects share it):

```sh
solana transfer -u devnet --from <FUNDED.json> <PAYER> 2 --allow-unfunded-recipient
solana balance -u devnet <PAYER>
```

Expect `Signature: …` and the new balance. The devnet faucet (`solana airdrop`) is
rate-limited and often empty; a transfer is the reliable path. The keeper needs no restart: the
next pass posts again. Record the transfer's signature in `docs/devnet-log.md`.

Never fund a keeper payer from the upgrade authority or the admin key (`docs/keys.md`): those
keys pay only their own fees.
