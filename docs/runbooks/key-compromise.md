# Runbook: a compromised key

One section per role of `docs/keys.md`. First, for any of them:

1. **Stop what signs with it**, by PID (`ps -axo pid,command | grep metalbook`, then
   `kill <PID>`; never by name).
2. **Tell the coordinator** (`joaquin/s-1836f1d6a089`): which key, since when, what it holds.
3. **Never paste the key, its file or a seed phrase** anywhere while handling it, the bus
   included; public keys and signatures only.
4. **Record** every step and signature in `docs/devnet-log.md`.

## Upgrade authority (worst case)

What it can do: replace the program's code, and with it move every vault's USDC.

- **You still hold it**: hand it to a new key at once, the new key signing
  (`docs/keys.md`, devnet step 2):

  ```sh
  solana-keygen new --silent --no-bip39-passphrase -o ~/.config/solana/metalbook-devnet-upgrade-2.json
  solana program set-upgrade-authority -u devnet FPPuou5kgSp6tbwQRwKPBNhrMyf1Xb8n2ib3Yqpbsuwg \
    --upgrade-authority <OLD.json> --new-upgrade-authority ~/.config/solana/metalbook-devnet-upgrade-2.json
  solana program show -u devnet FPPuou5kgSp6tbwQRwKPBNhrMyf1Xb8n2ib3Yqpbsuwg
  ```

  Expect `Authority: <NEW PUBKEY>`. Then check the program's hash is still the recorded one
  (`solana-verify get-program-hash`, `docs/verifiable-build.md`): a different hash means an
  upgrade already happened.
- **The attacker changed it first, or the hash differs**: the program is no longer ours. Stop
  listing (`list-week`), stop the maker quoting, stop every keeper, and announce that no one
  should mint into metalbook. Nothing on chain can undo it. On mainnet the multisig and its
  time lock exist to prevent exactly this (`docs/keys.md`).

## Admin

What it can do: `set_config` (every parameter for series listed afterwards, including the
receiver program and feed ids, and handing admin over) and `set_calendar` (which expiries can
be listed). It cannot touch a live series: every term is copied into the series at listing
(`docs/PLAN.md` 4.5).

- **You still hold it**: rotate admin to a new key with the on-chain parameters unchanged
  (`docs/keys.md`, devnet step 3), then `metalbook-admin show-config` and check `admin` and
  every parameter.
- **The attacker rotated it, or changed parameters**: treat every series listed after the
  compromise as untrusted (its oracle terms may point at a forged receiver); do not quote,
  mint into or keep a keeper on them (`--exclude`). Series listed before it are unaffected.
  Getting admin back needs a program upgrade by the upgrade authority.

## Lister

What it can do: spend its own SOL on listings, and receive the rent of the series it listed
when they close. No user funds.

```sh
solana transfer -u devnet --from <LISTER.json> <NEW_LISTER_PUBKEY> ALL --allow-unfunded-recipient
```

List from the new key from then on. Rent of series listed by the old key still returns to the
old key at `close_series`; that rent is lost if the attacker drains it, nothing else.

## Keeper payer

What it can do: spend its SOL, and post canonical prints (the program accepts only canonical
Pyth prints and settles only through the cascade, so a payer cannot choose a price or settle
early). No user funds.

1. Stop the keeper by PID.
2. Move what is left: `solana transfer -u devnet --from <PAYER.json> <NEW_PAYER_PUBKEY> ALL
   --allow-unfunded-recipient`.
3. Start the keeper with the new payer (`--keypair <NEW.json>`,
   docs/runbooks/keeper-restart.md) and `metalbook-watch --payer <NEW_PUBKEY>`.
4. Encoded VAAs left open by the old payer keep their rent on the old key; only the old key can
   close them, so that rent is written off.

## Hermes API key

What it can do: spend the key's Hermes budget (shared with varswap). No chain access.

The coordinator owns the key: report it, and after the rotation reload the environment
(`source ~/.zshrc`) and restart every metalbook process that reads Hermes, by PID
(docs/runbooks/keeper-restart.md). The key is read only from `METALBOOK_HERMES_API_KEY` and
never printed; check that no log or file holds it:

```sh
test -n "$METALBOOK_HERMES_API_KEY" && \
  grep -rlF -f <(printenv METALBOOK_HERMES_API_KEY) ~/tmp/worktrees/metalbook 2>/dev/null | wc -l
```

Expect `0`. The key reaches `grep` through a file descriptor, not its command line, so it never
shows in the process list, and the command prints a count, never the key.

## Maker key (QuoteBook)

Out of the protocol's scope (`docs/audit-scope.md`): it holds the maker's own inventory. Stop
the maker, cancel its quotes, move its USDC and long tokens to a new key.
