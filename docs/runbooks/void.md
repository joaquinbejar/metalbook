# Runbook: void a series

A series is voided when nothing can price it: no window slot, no next-print link and no
fallback candidate was ever stored (`docs/PLAN.md` 4.3, step 5). Then longs redeem for zero
and every writer claims its whole collateral back. The program never voids silently: `settle`
stores the method.

## When

From `expiry + hard_deadline_s + challenge_delay_s` (devnet params: 7 days and 600 s, so
**expiry + 605,400 s**), and only if every candidate is empty. Before that, `settle` returns
`SettlementNotReady` (custom error **141**). If any candidate exists, the same `settle`
settles on it instead (partial window, partial chain or fallback): that is the cascade working,
not a failure of this runbook.

Keep every keeper off a series meant to void: a keeper ticking it posts a fallback (or the
window slots) and it settles instead. Start keepers with `--exclude <SERIES>` for each one,
or with an explicit `--series` list that leaves them out.

## Commands

Environment (devnet), reading nothing secret:

```sh
export SOLANA_URL=https://api.devnet.solana.com
export METALBOOK_PROGRAM_ID=FPPuou5kgSp6tbwQRwKPBNhrMyf1Xb8n2ib3Yqpbsuwg
alias metalbook-admin='cargo run -q --release -p metalbook-cli --bin metalbook-admin --'
```

1. **Check the series** is still unsettled and nothing is owed to holders beyond zero:

   ```sh
   metalbook-admin show-series <SERIES>
   ```

   Expect `"state": "active"` or `"expired"`. `open` is the number of contracts written. The
   ledger's `longs_outstanding` and `writers_unclaimed` say who must act after the void.

2. **Void it**, from the time above, signed by any funded key (it pays one fee):

   ```sh
   metalbook-admin --keypair <PAYER.json> settle --series <SERIES>
   ```

   Expect `confirmed <SIGNATURE>`. Custom error 141 means it is too early (or a candidate is
   still inside its challenge delay); 116 (`SeriesAlreadySettled`) means someone settled it
   first. Then `show-series` prints `"state": "voided"` and `"settle_method": "void"`.

3. **Holders and writers** (only if `open > 0`):
   - each holder: `metalbook-admin --keypair <HOLDER.json> redeem --series <SERIES>
     --contracts <N>` burns the long tokens for zero;
   - each writer: `metalbook-admin --keypair <WRITER.json> claim-writer --series <SERIES>`
     returns that writer's whole collateral (its own written contracts × the maximum payout
     per contract).

   Claims never expire (`docs/PLAN.md` 4.6); nobody can take them on the holders' or
   writers' behalf.

4. **Close the series** once `longs_outstanding` and `writers_unclaimed` are both 0:

   ```sh
   metalbook-admin --keypair <PAYER.json> close-series --series <SERIES>
   ```

   It creates the rent payer's USDC account if missing (paid by the signer), then
   `close_series` sends any donation in the vault there and returns the series and vault rent
   to the payer recorded at `create_series`. Expect `confirmed <SIGNATURE>`; custom error 145
   (`SeriesNotEmpty`) means a holder or writer has not acted yet. `show-series` then fails
   with "account not found". The long mint stays (SPL Token cannot close a mint).

5. **Record** each signature, the rent returned and the time in `docs/devnet-log.md`.

## Drill: the ten Oct 30 XAG 50 oz series (2026-11-06)

Expiry 1793390400 (Oct 30, 20:00Z), so the void opens at **1793995800, 2026-11-06 20:10Z**.
None was written (`open` 0), so step 3 is empty. Series:

```text
7WStW2SFVLeeTYrBCMszTZLgMkgDH2YJRABEHR7iiTao  put 48
7EUNY3M7579s2M4UnmDS7QscJK61a5rrpPN9WQNn5wVw  call 48 cap 52.8
7gDF4kJGSQMzHcqoWB3FVqEvV8AuHH53JC5r1jBRN377  put 49
HbovhXFHPTJDj3EkCiEyzaSKsXCocxH5ot5djtcGERLW  call 49 cap 53.9
4F2hdKE6gJ7TvMBq14pWgsBfKb3VxhTNA96CbYkqsWHn  put 50
84uyQjzN6MkRi7XCx8q1xEqqhQFYavRAt119jMXXn4BY  call 50 cap 55
GZz1xtQKoUjAj7v3x63PBgCDv1UexqdmK25HuyDqzmWJ  put 51
Ew94BcV7MYXiNx77yooBXteEe4XsxykZodU7Xg5MkpPD  call 51 cap 56.1
6SYM398CRzH6j2u9gikyGCrSeRtdR3QK3pCCkrMFik31  put 52
Df5oVmx94PJM5BQEmk2QeWr7DEgaHaZ8TpBwzyDfaMX  call 52 cap 57.2
```

The Nov 6 keeper runs with `--exclude` for all ten.
