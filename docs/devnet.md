# Devnet runbook (milestone 7)

Status: **steps 1 to 4 executed on 2026-10-30** (deploy, config, calendars, 52 series); the
record is [`devnet-log.md`](devnet-log.md). Writing, trading and settlement wait for devnet
USDC and a Hermes key (see the log). Steps marked **GAP** need code that does not exist yet,
listed in [What is missing](#what-is-missing).

Done when (`docs/PLAN.md`, milestone 7): a full series lifecycle runs on devnet against live Pyth
XAU/USD.

## What is needed from Joaquin

| Item | Why | Notes |
| --- | --- | --- |
| Upgrade-authority keypair | Deploys and upgrades the program, and is the only key `init_config` accepts (PLAN 3.3) | Ideally a hardware wallet or a dedicated key, never committed. It is also the first config admin; `set_config` can hand admin to another key afterwards. |
| Program id keypair | Fixes the program address | `cargo build-sbf` generates `target/deploy/metalbook_program-keypair.json` (git-ignored). Keep a copy outside the repo; losing it does not lose the program, but keeping it lets the address be reproduced. |
| Devnet SOL, about 5 SOL | Program deployment and account rent | The 91 KB program needs about 1.3 SOL of rent for its ProgramData account with the default 2x buffer, plus a temporary buffer during `deploy`. Config, calendars, series, mints and fees are small. The devnet faucet is rate-limited, so the amount may need to be requested in several steps. |
| Hermes API key | The keeper reads Pyth prints from Hermes; every price endpoint (`hermes`, `hermes-beta`, `benchmarks`) answers 401 without a key (checked 2026-10-30; only `/v2/price_feeds` metadata is public) | Set `METALBOOK_HERMES_API_KEY` in the environment only, never in a file. The auth header scheme is unverified (PLAN M7b-1): the first live call confirms it. |
| A decision on the USDC mint | Collateral and payouts | Recommended: our own 6-decimal test mint on devnet, so we can mint test USDC to writers. Circle's devnet USDC is the alternative, but its supply is faucet-limited. **Decided: Circle devnet USDC `4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU`.** |
| Calendar times | Only calendar expiries can be listed (PLAN 4.4) | Times when XAU trades, with margin around the 30-minute settlement window, for example Wednesday 15:00 New York. **Decided: weekly Fridays 16:00 New York; strikes at the money ±5% in $50 (XAU) and $1 (XAG) steps.** |
| A host for the keeper | `tick` must run more often than `D` (10 minutes) | Any machine with the key in its environment. One minute is a good interval. |

## Facts to rely on (checked 2026-10-29)

- The Pyth receiver program `rec5EKMGg6MxZYaMdyBfgwp4d5rB9T1VQH5pJv5LtFJ` exists on devnet, owned by
  the upgradeable loader and executable (checked through `getAccountInfo` on `api.devnet.solana.com`). Feed
  ids are the same on every network: XAU/USD
  `765d2ba906dbc32ca17cc11f5310a89e9ee1f6420508c63861f2f8ba4ee34bb2`, XAG/USD
  `f2fb02c32b055c805e7238d628e5e9dadef274376114eb1f012337cabe93871e`.
- Pinned exponents: XAU −3, XAG −5 (read from mainnet `PriceUpdateV2` accounts).
- Sponsored push feeds are not used (the XAG push account was stale): the keeper posts updates
  itself (PLAN, verified facts).

## Environment

```sh
export SOLANA_URL=https://api.devnet.solana.com
export METALBOOK_UPGRADE_AUTHORITY=~/keys/metalbook-devnet-authority.json   # outside the repo
export METALBOOK_PROGRAM_KEYPAIR=~/keys/metalbook-devnet-program.json       # outside the repo
export METALBOOK_HERMES_API_KEY=...                                        # never in a file
# metalbook-admin (after step 2): the program id and the signer of each command.
export METALBOOK_PROGRAM_ID=<PROGRAM_ID>
export METALBOOK_KEYPAIR="$METALBOOK_UPGRADE_AUTHORITY"                    # admin steps
alias metalbook-admin='cargo run -q -p metalbook-cli --bin metalbook-admin --'   # reads SOLANA_URL
# export METALBOOK_HERMES_URL=https://hermes.pyth.network                  # default
# export METALBOOK_HERMES_AUTH=bearer                                      # default; or header:<name>
```

## Steps

### 1. Build and record what is deployed

```sh
git switch main && git pull
make pre-push                       # must be clean
make program                        # target/deploy/metalbook_program.so
shasum -a 256 target/deploy/metalbook_program.so
git rev-parse HEAD
```

Record the commit and the `.so` hash in the deployment log at the end of this file.

### 2. Deploy with the upgradeable loader

```sh
solana config set --url "$SOLANA_URL" --keypair "$METALBOOK_UPGRADE_AUTHORITY"
solana balance
solana program deploy target/deploy/metalbook_program.so \
  --program-id "$METALBOOK_PROGRAM_KEYPAIR" \
  --upgrade-authority "$METALBOOK_UPGRADE_AUTHORITY"
solana program show <PROGRAM_ID>
```

Check that `solana program show` lists the upgradeable loader as owner and our key as
`Authority`: `init_config` reads exactly that field of the ProgramData account. Never make the
program immutable (`--final`) on devnet; `init_config` rejects a program with no upgrade
authority.

### 3. Initialize the config (`init_config`, tag 0)

Signed by the upgrade authority, who becomes admin. Accounts: `[admin (signer, writable),
config PDA ["config"], ProgramData PDA ([program_id] under the upgradeable loader), USDC mint,
system program]`. Parameters (161 bytes, layout in `metalbook-program/src/processor/admin.rs`),
devnet values:

| Parameter | Value |
| --- | --- |
| `min_listing_s` | 3_600 |
| `strike_tick` | XAU 50 USD (50_000_000), XAG 1 USD (1_000_000): the approved strike steps |
| `multiplier` | XAU 1_000 (1 oz), XAG 50_000 (50 oz) |
| `cap_bps` | 1_000, 2_500, 5_000, 0 |
| `receiver_program` | `rec5EKMGg6MxZYaMdyBfgwp4d5rB9T1VQH5pJv5LtFJ` |
| `feed_id` | XAU and XAG ids above |
| `expo` | −3, −5 |
| `max_conf_bps` | 100 (1%) |
| `slot_tolerance_s`, `min_samples`, `k_next_print` | 10, 20, 5 |
| `window_s`, `slot_s` | 1_800, 60 |
| `challenge_delay_s` (`D`) | 600 |
| `fallback_lookback_s`, `hard_deadline_s` | 86_400, 604_800 (devnet may shorten the hard deadline to test the void path) |

These values are committed as `metalbook-cli/devnet-params.json` (edit the hard deadline there
first if the void path is to be tested). Signed by the upgrade authority:

```sh
metalbook-admin init-config --usdc-mint <USDC_MINT> --params metalbook-cli/devnet-params.json --dry-run
metalbook-admin init-config --usdc-mint <USDC_MINT> --params metalbook-cli/devnet-params.json
metalbook-admin show-config        # the same parameters back, plus admin and mint
```

`--dry-run` prints the accounts and data without signing. Later changes go through
`show-config`, editing its `params` into a file, and `set-config --params <file>` (add
`--new-admin <KEY>` to hand admin over).

### 4. Calendar and a series

- `set_calendar` (tag 2), as admin, for XAU with the chosen expiries (Unix seconds, strictly
  increasing, up to 64; the list replaces the calendar):
  `metalbook-admin set-calendar --underlying xau --expiries <T1>,<T2>`, then
  `metalbook-admin show-calendar --underlying xau`.
- `create_series` (tag 3), as any payer: for example an XAU put with a strike near spot on the 5
  USD tick, expiry from the calendar at least an hour away; the multiplier defaults to the
  config's (1_000 for XAU). This creates the series, its long mint and its USDC vault:
  `metalbook-admin create-series --underlying xau --kind put --strike 2400 --expiry <T1>`
  prints the series address, `metalbook-admin show-series <SERIES>` reads it back. A call
  takes `--cap <USD>` on the cap grid.
- Mint test USDC to a writer and have the writer `mint` (tag 4) a few contracts
  (`metalbook_sdk::ix::mint` with any Solana client; the CLI covers only admin and listing).

### 5. Run the keeper

The keeper posts every eligible canonical slot, chain link and the latest fallback, and settles
once the cascade allows (PLAN 4.3).

```sh
METALBOOK_HERMES_API_KEY=... cargo run -p metalbook-keeper --example hermes -- --live   # confirms the key and auth scheme
```

If that returns 401 or 403, try `METALBOOK_HERMES_AUTH=header:<name>` and record the scheme that
works in PLAN, removing the UNVERIFIED mark.

**GAP: posting and the keeper binary.**
- Each print must become a `Full` `PriceUpdateV2` through Wormhole's encoded VAA and the
  receiver's `post_update` (M7b-2).
- The keeper needs an RPC `Chain` implementation and a binary that runs `tick` every minute for
  each live series.

### 6. Exercise the lifecycle

Within one expiry:
1. Writers `mint`, one writer `close`s part.
2. Long tokens move to a holder.
3. The keeper samples during the window and settles after expiry; record the method, price and sample count.
4. Holders `redeem` and writers `claim_writer`.
5. `close_position` and `close_series` once nothing is owed; check that rent returns to the payers.

Optionally, list a second series and let it void (no posts, shortened hard deadline) to see writers
recover their collateral.

Every transaction signature goes into the log below.

### 7. Upgrades and teardown

- Upgrade: `solana program deploy target/deploy/metalbook_program.so --program-id <PROGRAM_ID>
  --upgrade-authority "$METALBOOK_UPGRADE_AUTHORITY"`. Account layouts carry a version byte;
  an upgrade that changes a layout needs a migration or fresh series.
- Teardown, devnet only: `solana program close <PROGRAM_ID> --bypass-warning` returns the
  ProgramData rent to the authority.

## What is missing

1. ~~Instruction builders and an admin CLI~~: done, `metalbook-sdk` (every instruction, PDA
   helpers, decoders) and `metalbook-admin` (`metalbook-cli`) for steps 3 and 4. Writer and
   holder steps (`mint`, `close`, `redeem`, `claim_writer`, the closes) use the SDK with any
   Solana client.
2. **M7b-2: Pyth receiver posting** (Wormhole encoded VAA, `verify_encoded_vaa_v1`, receiver
   `post_update` for a `Full` update). Blocked on the Hermes key, needed to record a
   guardian-signed update for tests.
3. **An RPC `Chain` for the keeper and a keeper binary.** It depends on item 2 for `post`; reading
   accounts and `settle` do not.

## Deployment log

| Date | Commit | `.so` sha256 | Program id | Notes |
| --- | --- | --- | --- | --- |
| 2026-10-30 | `93f175a` | `4b58cb50…54127` | `FPPuou5kgSp6tbwQRwKPBNhrMyf1Xb8n2ib3Yqpbsuwg` | Steps 1 to 4; details and every signature in [`devnet-log.md`](devnet-log.md) |
