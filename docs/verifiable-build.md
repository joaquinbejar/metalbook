# Verifiable build

The deployed metalbook program is built by `solana-verify` in a Docker image pinned by digest,
so anyone can rebuild it from a commit and compare the hash with the program on chain
(docs/PLAN-improvements.md, milestone H2).

## Evidence

| Item | Value |
| --- | --- |
| solana-verify | 0.5.2 (`cargo install --locked solana-verify --version 0.5.2`) |
| Image | `solanafoundation/solana-verifiable-build:4.3.0@sha256:12fd4c0a0790f0fc41ef74b0cdb6bccc167ba6137eb1adb749bac649481c86bd` (linux/amd64; `4.3.0` is the newest tag on Docker Hub, checked 2026-11-02) |
| Toolchain inside the image | `cargo-build-sbf 4.3.0`, `platform-tools v1.57`, `rustc 1.95.0` (checked by CI on every run) |
| Command | `solana-verify build --library-name metalbook_program --arch v3 --base-image <image> <repo>` (`make verifiable`) |
| Commit | pending: the first CI run of this PR |

Program hash: pending

`make program` (and so `make bench` and the LiteSVM tests) uses `cargo-build-sbf 4.5.0` from
the Agave 4.4.0 release with the same platform-tools v1.57 and SBF Rust 1.95.0. The verifiable
binary is the one deployed, so its compute units are measured on their own by
`make bench-verifiable` into `docs/benchmarks-verifiable.md`.

## What CI checks

`.github/workflows/verifiable.yml` runs on every PR and on `main`:

1. prints `cargo build-sbf --version` inside the pinned image and fails on another toolchain;
2. builds the program twice with `make verifiable`, each time in a clean clone of the commit at
   a different path, and fails unless the two hashes are equal;
3. uploads the binary as the artifact `metalbook_program-verifiable`;
4. fails unless the hash equals the `Program hash` line above (`make verifiable-check`);
5. measures the binary's CU and fails unless `docs/benchmarks-verifiable.md` is the committed
   one.

A change to the program, its dependencies or `Cargo.lock` that changes the binary therefore
updates the hash here and the table in `docs/benchmarks-verifiable.md` in the same PR.

## Reproduce

```sh
cargo install --locked solana-verify --version 0.5.2
make verifiable          # prints the hash of target/verifiable/metalbook_program.so
make verifiable-check    # compares it with the Program hash above
make bench-verifiable    # rewrites docs/benchmarks-verifiable.md
```

The image is linux/amd64 only. On an ARM Mac Docker runs it under emulation, where the build
hung for three hours at 0% CPU (2026-11-02), so the reference build is CI's, on a native amd64
runner.

## Check the deployed program

```sh
solana-verify get-program-hash -u devnet FPPuou5kgSp6tbwQRwKPBNhrMyf1Xb8n2ib3Yqpbsuwg
solana-verify verify-from-repo -ud --program-id FPPuou5kgSp6tbwQRwKPBNhrMyf1Xb8n2ib3Yqpbsuwg \
  --library-name metalbook_program --commit-hash <commit> https://github.com/joaquinbejar/metalbook
```

The first prints the on-chain hash, which must equal the Program hash above once devnet runs the
verifiable binary. `verify-from-repo` without `--remote` rebuilds locally and compares. The
devnet upgrade waits until after the 2026-11-06 drill; uploading the verification to the public
OtterSec API (`--remote`, which also writes the on-chain verify PDA) waits for the
coordinator's go.

| Date | Devnet program hash | Equals the Program hash |
| --- | --- | --- |
| 2026-11-02 | `62718c46df5e6c9f9d73269b205f22ef1877765e45a170b382c5bec12facde9c` | No: it is the `make program` build (cargo-build-sbf 4.5.0), whose hash it equals at `67088a3` |
