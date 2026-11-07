# Keys and roles

metalbook has four roles. On devnet each has its own key since 2026-11-07 (milestone H3 of
`docs/PLAN-improvements.md`); this page also writes down the mainnet procedure, where nothing is
done yet.

| Role | What it can do | Checked by |
| --- | --- | --- |
| Upgrade authority | Replace the program's code (`solana program deploy` or `upgrade`), and sign `init_config` once | The BPF upgradeable loader; `init_config` reads `ProgramData` |
| `Config` admin | `set_config` (parameters, hand admin over) and `set_calendar` (listable expiries) | The program: the signer must equal `Config.admin` (`NotAuthority`) |
| Lister | `create_series` on a listed expiry; pays the series' rent and gets it back at `close_series` | Anyone; the rent goes back to the payer recorded in the series |
| Keeper payer | Fees and the Pyth receiver's rent for posts; `sample_settlement` and `settle` | Anyone; holds no authority over metalbook |

## Devnet today

| Key | Address | Role |
| --- | --- | --- |
| `~/.config/solana/metalbook-devnet-upgrade.json` | `74jZtwqQ1n15ggR4rS3K94a2gZMRtochfPceYHVPF6sJ` | upgrade authority |
| `~/.config/solana/metalbook-devnet-admin.json` | `5fvjUpFU7faq8rFRcSrPrPb6f6qRGhJzJcWZJk2As7Tx` | `Config` admin |
| `~/.config/solana/metalbook-devnet-keeper.json` | `DuxGX5znvbiHFHk4rpKmP238zXf5eXTK4xNLN44ecSY2` | keeper payer |
| `~/.config/solana/metalbook-devnet-lister.json` | `QALJUrTtz1GJDT8oiW9V4DRfPW1BZzFJjQ7kbd9zND8` | lister |
| `~/.config/solana/id.json` | `77ieYLKspRnGzAt1AK6ZVsttnTY6T7EY2k7CpTZeCPPV` | none: rent payer of the series listed before 2026-11-07, and the funding key |

`id.json` is shared with other projects on the same machine; every transaction metalbook signs
with it is announced on the bus first. Until 2026-11-07 it held all four roles.

## Devnet separation (done 2026-11-07)

Run on 2026-11-07, after the Nov 6 drill; the signatures are in `docs/devnet-log.md` (Key
separation). Keypair files live under `~/.config/solana/`, outside every
repository and outside the keeper's run directory. Seed phrases and private keys never enter a
terminal log, a commit, the bus or `docs/devnet-log.md`: only public keys and signatures do.

1. **New keys.** `solana-keygen new --silent --no-bip39-passphrase -o <FILE>` for each
   (`--silent` keeps the seed phrase off the terminal; `solana-keygen pubkey <FILE>` prints the
   address):
   `metalbook-devnet-upgrade.json` (upgrade authority), `metalbook-devnet-admin.json` (admin),
   `metalbook-devnet-keeper.json` (keeper payer) and `metalbook-devnet-lister.json` (lister).
   The upgrade-authority and admin keys get only enough SOL for their own fees; the upgrade
   key never pays a keeper or listing fee.
2. **Upgrade authority.** `solana program set-upgrade-authority -u devnet
   FPPuou5kgSp6tbwQRwKPBNhrMyf1Xb8n2ib3Yqpbsuwg --upgrade-authority ~/.config/solana/id.json
   --new-upgrade-authority ~/.config/solana/metalbook-devnet-upgrade.json`. The new authority is
   given as its keypair file, so it signs the change and a mistyped address cannot lock the
   program (never `--skip-new-upgrade-authority-signer-check`). Check with
   `solana program show`.
3. **Admin.** `metalbook-admin show-config` and copy its `params` into a file, unchanged
   (the on-chain values, not `metalbook-cli/devnet-params.json`, which may differ), then
   `metalbook-admin --keypair ~/.config/solana/id.json set-config --params <FILE> --new-admin
   <ADMIN_PUBKEY> --dry-run`, the same without `--dry-run`, and `show-config` again: `admin`
   is the new key and every parameter is unchanged.
4. **Payers.** Keepers run with `--keypair metalbook-devnet-keeper.json` (a second keeper gets
   its own key; two keepers never share a payer, `docs/devnet-log.md`). `list-week` and
   `create-series` run with the lister key. Series listed before this step keep
   `77ie…` as their rent payer; `close_series` still returns their rent there.
5. **Check.** No key holds two roles: `solana program show` names the upgrade key, `show-config`
   names the admin key, and the keeper and lister keys appear in neither.

The old key stays funded (it receives the earlier series' rent) and holds no role afterwards.

## Mainnet procedure (plan only)

Nothing below is done; it is the procedure the mainnet launch follows.

- **Upgrade authority and admin behind a Squads v4 multisig.** Members and threshold are the
  owner's decision. The program is deployed by a deploy key, which then hands the upgrade
  authority to the multisig's vault. The vault is a PDA and cannot sign an ordinary CLI
  transaction, so the handover is not the devnet step 2 (where the new authority is a keypair
  that signs): it is either Squads' Safe Authority Transfer (SAT), a transaction the deploy key
  (the current authority) and the vault both sign, the vault through a multisig proposal that
  meets the threshold, or a plain transfer to the vault address, which the vault does not sign
  and so must be checked by hand first. SAT is the one to use
  (https://docs.squads.so/main/navigating-your-squad/developers-assets/programs). The admin:
  `init_config` signed by the deploy key while it is still the upgrade authority, then
  `set_config --new-admin <VAULT>` signed by it. `set_config` does not need the new admin's
  signature, so the vault address is checked before sending and with `show-config` after;
  from then on every admin instruction is a multisig proposal. Both handovers happen before
  any series is listed.
- **Time lock.** The multisig delays every upgrade, so holders get notice of a pending upgrade
  before it lands. Notice only: an open position in a cash-settled series may not be closable
  before the upgrade at a fair price, or at all, so the time lock is not an exit guarantee.
- **Upgrades** go through a buffer (`solana program write-buffer`, buffer authority set to the
  vault), a multisig proposal and the time lock. Each signer checks the buffer's hash against
  the verifiable build (`docs/verifiable-build.md`) before approving.
- **Keeper payers** are hot keys, one per keeper, holding only fee and receiver-rent SOL, with
  the `LowBalance` alert of `metalbook-watch`. They hold no authority, so a leaked keeper key
  costs at most its balance.
- **Listing** runs from a lister key that holds the listing rent and nothing else.
- **Final step.** After the audit, either drop the upgrade authority
  (`solana program set-upgrade-authority --final`, through the multisig) or keep it under the
  multisig and time lock. The choice and its reasons go in `docs/PLAN.md`.
