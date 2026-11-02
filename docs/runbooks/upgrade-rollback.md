# Runbook: program upgrade and rollback

The program is upgradeable by its upgrade authority (`docs/keys.md`). Every deployed binary is
the verifiable build of a commit on `main` (`docs/verifiable-build.md`): CI builds it, checks
its hash against the one recorded there and uploads it as the artifact
`metalbook_program-verifiable`.

## Before

1. **No settlement window open.** An upgrade during a window can stop posts or settles for the
   minutes it takes. Schedule it away from the expiries in `docs/devnet-log.md`'s calendar
   (each window is the 30 minutes before an expiry), and check that every series of the last
   expiry is settled: `metalbook-admin show-series <SERIES>` shows `"state": "settled"` or
   `"voided"`.
2. **Account layout.** A new binary must read every live account (`Config`, calendars,
   series, positions). A layout change needs its own migration plan; this runbook covers
   upgrades that keep the layouts.
3. **The binary.** Download the artifact of the commit's `main` run of the `Verifiable build`
   workflow and check its hash:

   ```sh
   BIN=$(mktemp -d)
   gh run download <RUN_ID> -n metalbook_program-verifiable -D "$BIN"
   solana-verify get-executable-hash "$BIN/metalbook_program.so"
   ```

   It must equal the `Program hash` in `docs/verifiable-build.md` at that commit.

## Upgrade

Through a buffer, so the bytes can be checked before they replace the program:

```sh
solana program write-buffer -u devnet --buffer-authority <UPGRADE.json> \
  "$BIN/metalbook_program.so" 2>&1 | grep -E '^(Buffer|Signature):'
solana-verify get-buffer-hash -u devnet <BUFFER>
solana program upgrade -u devnet --upgrade-authority <UPGRADE.json> \
  <BUFFER> FPPuou5kgSp6tbwQRwKPBNhrMyf1Xb8n2ib3Yqpbsuwg 2>&1 | grep -E '^Signature:'
solana-verify get-program-hash -u devnet FPPuou5kgSp6tbwQRwKPBNhrMyf1Xb8n2ib3Yqpbsuwg
```

- The `grep` allowlist matters: a failed `write-buffer` prints a recovery seed phrase for the
  buffer's keypair, which must never reach a log. If it fails, rerun it the same way; the
  partly written buffer can be closed with `solana program close -u devnet <BUFFER>
  --authority <UPGRADE.json> --recipient <PUBKEY>` (without `--recipient` the lamports go to
  the default configured keypair).
- The buffer hash and the program hash after the upgrade must both equal the recorded
  `Program hash`.
- A binary larger than the program account first needs `solana program extend -u devnet
  FPPuou5kgSp6tbwQRwKPBNhrMyf1Xb8n2ib3Yqpbsuwg <ADDITIONAL_BYTES>`; the error from `upgrade`
  says so.

Then run one read of each account kind (`show-config`, `show-calendar --underlying xau`,
`show-series <SERIES>`) and, at the next window, check the keeper posts and settles. Record
the commit, the hashes and the signatures in `docs/devnet-log.md`.

## Rollback

A rollback is an upgrade to the previous verifiable binary: the artifact of the commit that
was deployed before (its hash is the `Program hash` of `docs/verifiable-build.md` at that
commit, and the previous row of the deployment table). Same steps, same checks. Two limits:

- A rollback across an account layout change is a migration, not a rollback: accounts
  written by the new binary may not be readable by the old one.
- State changes made under the new binary (settlements, claims) stay; a rollback changes
  code, not history.

On mainnet, both go through the multisig and its time lock (`docs/keys.md`).
