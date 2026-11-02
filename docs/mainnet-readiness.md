# Mainnet readiness

Items that gate a mainnet deployment, with where each stands. Nothing here is done on
mainnet without SOLANA-PROJECTS' go; devnet results are recorded in `docs/devnet-log.md`.

| Item | Gate | Status |
| --- | --- | --- |
| Keeper host independence | The second keeper runs on another machine (or a cloud host) than the primary, with its own payer and RPC endpoint. Until then the two keepers are process-independent only: two LaunchAgents on the Studio share its power, network and clock. | Open. Devnet runs both on the Studio (coordinator, bus 44325). |
| Key separation | Upgrade authority and `Config` admin behind a multisig; keeper payers hold no authority (`docs/PLAN-improvements.md` H3). | Open (H3). |
| Verifiable build | The deployed program's hash equals a `solana-verify` build of the tagged commit (H2). | Devnet done (H2, 2026-11-02): devnet runs `62718c46…`, the hash CI reproduces (`docs/verifiable-build.md`); the public OtterSec record waits for a go. Mainnet: build from the release tag. |
| Audit | Scope in `docs/audit-scope.md` (H4), findings resolved. | Open. |
| Keeper drills | Void, restart and second-keeper drills passed on devnet (P2, P3). | Open (Nov 6 drill). |
| RPC | A paid RPC endpoint for keepers and the watcher: public endpoints rate-limit `getProgramAccounts` (seen on devnet, 2026-11-02), which discovery and the leftover scan use. | Open. |
