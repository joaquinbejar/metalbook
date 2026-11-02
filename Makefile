# Makefile for the metalbook workspace. Targets mirror matchbook's; `pre-push` follows
# rules/global_rules.md (fmt check, clippy -D warnings, lint control, tests, docs -D warnings).

.PHONY: all
all: fmt-check lint test

# sBPF target version for the program (cargo-build-sbf's default is v3).
SBF_ARCH ?= v3

# Host toolchain for the explicit `+toolchain` calls (rustfmt): `stable` locally, the exact
# pin in CI (.github/workflows/ci.yml), where RUSTUP_TOOLCHAIN covers the plain `cargo` calls
# but cannot override an explicit `+stable`.
RUST_TOOLCHAIN ?= stable
# Extra flags for every cargo command; CI sets `--locked`.
CARGO_FLAGS ?=
export CARGO_FLAGS

# Build the workspace
.PHONY: build
build:
	cargo build $(CARGO_FLAGS) --workspace

# Build metalbook-program for SBF into target/deploy. cargo-build-sbf's exit status is kept (no
# pipe), and scripts/check-sbf-log.sh fails the build on a stack frame above 4 KiB, which
# cargo-build-sbf only warns about.
.PHONY: program
program:
	@mkdir -p target
	@cargo build-sbf --manifest-path metalbook-program/Cargo.toml --arch $(SBF_ARCH) -- $(CARGO_FLAGS) \
		> target/build-sbf.log 2>&1; status=$$?; cat target/build-sbf.log; \
		if [ $$status -ne 0 ]; then echo "program: cargo-build-sbf failed ($$status)"; exit $$status; fi
	@sh scripts/check-sbf-log.sh target/build-sbf.log

# Build the quotebook program of the quotebook-sdk release we depend on (crates.io 0.1.0, tag
# v0.1.0, pinned by commit) into target/deploy/quotebook_program.so, for the M5 LiteSVM demo.
# The published crates do not carry the program, so the tag is cloned into target/.
QUOTEBOOK_TAG := v0.1.0
QUOTEBOOK_COMMIT := 6417a0f236933428436eb1de744be13745106ae3
.PHONY: quotebook-program
quotebook-program:
	@mkdir -p target
	@if [ ! -d target/quotebook-src ]; then \
		git -c advice.detachedHead=false clone -q --depth 1 --branch $(QUOTEBOOK_TAG) https://github.com/joaquinbejar/quotebook.git target/quotebook-src; fi
	@test "$$(git -C target/quotebook-src rev-parse HEAD)" = "$(QUOTEBOOK_COMMIT)" || \
		{ echo "quotebook-program: target/quotebook-src is not $(QUOTEBOOK_TAG) ($(QUOTEBOOK_COMMIT))"; exit 1; }
	@CARGO_TARGET_DIR=$(CURDIR)/target/quotebook cargo build-sbf \
		--manifest-path target/quotebook-src/program/Cargo.toml \
		--arch $(SBF_ARCH) --sbf-out-dir $(CURDIR)/target/deploy -- $(CARGO_FLAGS) > target/build-quotebook.log 2>&1; status=$$?; \
		if [ $$status -ne 0 ]; then cat target/build-quotebook.log; echo "quotebook-program: build failed ($$status)"; exit $$status; fi
	@sh scripts/check-sbf-log.sh target/build-quotebook.log

# The Pyth receiver and the Wormhole core bridge as deployed on devnet (the milestone 7b-2 LiteSVM
# test posts a recorded Hermes update through them), dumped into target/deploy and checked
# against the hashes below; a redeployment changes the hash and fails here, not in the test. A
# file already present with the pinned hash (CI caches both by hash) is not dumped again.
PYTH_RPC ?= https://api.devnet.solana.com
PYTH_RECEIVER_SHA256 := 0d6bf9142c2eb1fdb8ead48054491369bed3a8d6846c5887e362844155599ab2
WORMHOLE_SHA256 := ca237355a04cc5ceeb788853833ae46c0cb8b0a728625e8f9bb9bc05d1e9c619
.PHONY: pyth-programs
pyth-programs:
	@mkdir -p target/deploy
	@echo "$(PYTH_RECEIVER_SHA256)  target/deploy/pyth_receiver.so" | shasum -a 256 -c - > /dev/null 2>&1 || \
		solana program dump -u $(PYTH_RPC) rec5EKMGg6MxZYaMdyBfgwp4d5rB9T1VQH5pJv5LtFJ \
		target/deploy/pyth_receiver.so > /dev/null
	@echo "$(WORMHOLE_SHA256)  target/deploy/wormhole_core_bridge.so" | shasum -a 256 -c - > /dev/null 2>&1 || \
		solana program dump -u $(PYTH_RPC) HDwcJBJXjL9FpJ7UBsYBtaDjsBUhuLCUYoz3zr8SWWaQ \
		target/deploy/wormhole_core_bridge.so > /dev/null
	@echo "$(PYTH_RECEIVER_SHA256)  target/deploy/pyth_receiver.so" | shasum -a 256 -c -
	@echo "$(WORMHOLE_SHA256)  target/deploy/wormhole_core_bridge.so" | shasum -a 256 -c -

# Failing controls for `program`: the log check must reject a frame overflow in both wordings,
# and a failing cargo-build-sbf (an unknown sBPF arch) must make `make program` fail.
.PHONY: program-control
program-control:
	@mkdir -p target
	@printf '%s\n' 'Error: Function entrypoint overflows the maximum allowed frame space by accessing an offset 64 bytes greater than the maximum of 4096.' \
		> target/sbf-control.log
	@if sh scripts/check-sbf-log.sh target/sbf-control.log > /dev/null; then \
		echo "program-control: the log check accepted a frame overflow"; exit 1; fi
	@printf '%s\n' 'Error: Function _ZN4main Stack offset of 4160 exceeded max offset of 4096 by 64 bytes' \
		> target/sbf-control.log
	@if sh scripts/check-sbf-log.sh target/sbf-control.log > /dev/null; then \
		echo "program-control: the log check accepted the older wording"; exit 1; fi
	@if $(MAKE) --no-print-directory program SBF_ARCH=no-such-arch > /dev/null 2>&1; then \
		echo "program-control: make program succeeded with a failing cargo-build-sbf"; exit 1; fi
	@echo "program-control: a frame overflow and a failing cargo-build-sbf both fail make program"

# Run the CU benchmark in LiteSVM and regenerate docs/benchmarks.md
.PHONY: bench
bench: program
	METALBOOK_BENCH_OUT=$(CURDIR)/docs/benchmarks.md \
	cargo test $(CARGO_FLAGS) -p metalbook-tests --release --test cu -- --nocapture

# Verifiable build (docs/verifiable-build.md, milestone H2): metalbook-program built by
# solana-verify in the verifiable-build image pinned by digest, so anyone can rebuild the deployed
# binary and compare hashes. Needs Docker and `cargo install --locked solana-verify --version
# 0.5.2`. The image is linux/amd64 only: on an ARM Mac it runs under emulation, where the build
# hung (2026-11-02), so CI (.github/workflows/verifiable.yml) is where it is built and checked.
# The binary is copied to target/verifiable, since cargo-build-sbf's target/deploy is shared
# with `make program`.
SOLANA_VERIFY_VERSION := 0.5.2
VERIFY_IMAGE := solanafoundation/solana-verifiable-build:4.3.0@sha256:12fd4c0a0790f0fc41ef74b0cdb6bccc167ba6137eb1adb749bac649481c86bd
VERIFIABLE_SO := target/verifiable/metalbook_program.so
.PHONY: verifiable
verifiable:
	@solana-verify --version | grep -qxF "solana-verify $(SOLANA_VERIFY_VERSION)" || \
		{ echo "verifiable: want solana-verify $(SOLANA_VERIFY_VERSION)"; exit 1; }
	@mkdir -p target/verifiable
	solana-verify build --library-name metalbook_program --arch $(SBF_ARCH) \
		--base-image "$(VERIFY_IMAGE)" $(CURDIR)
	@cp target/deploy/metalbook_program.so $(VERIFIABLE_SO)
	@echo "verifiable: $$(solana-verify get-executable-hash $(VERIFIABLE_SO))  $(VERIFIABLE_SO)"

# The hash `make verifiable` must reproduce, read from docs/verifiable-build.md.
VERIFIABLE_HASH = $(shell sed -n 's/^Program hash: `\([0-9a-f]\{64\}\)`.*/\1/p' docs/verifiable-build.md)
.PHONY: verifiable-check
verifiable-check:
	@want="$(VERIFIABLE_HASH)"; got=$$(solana-verify get-executable-hash $(VERIFIABLE_SO)); \
		[ -n "$$want" ] || { echo "verifiable-check: no program hash in docs/verifiable-build.md"; exit 1; }; \
		[ "$$got" = "$$want" ] || { echo "verifiable-check: built $$got, docs/verifiable-build.md has $$want"; exit 1; }; \
		echo "verifiable-check: $$got matches docs/verifiable-build.md"

# Measure the verifiable binary (run `make verifiable` first) into docs/benchmarks-verifiable.md.
.PHONY: bench-verifiable
bench-verifiable:
	METALBOOK_PROGRAM_SO=$(abspath $(VERIFIABLE_SO)) \
	METALBOOK_BENCH_OUT=$(CURDIR)/docs/benchmarks-verifiable.md \
	cargo test $(CARGO_FLAGS) -p metalbook-tests --release --test cu -- --nocapture

# Run every test, including the property tests and the LiteSVM tests (which need
# `make program`, `make quotebook-program` and `make pyth-programs` first). METALBOOK_PROPTEST_CASES=<n> overrides the case count.
.PHONY: test
test:
	cargo test $(CARGO_FLAGS) --workspace --all-features

# Format the code
.PHONY: fmt
fmt:
	cargo +$(RUST_TOOLCHAIN) fmt --all

# Check formatting
.PHONY: fmt-check
fmt-check:
	cargo +$(RUST_TOOLCHAIN) fmt --all --check

# Clippy with every warning denied
.PHONY: lint
lint:
	cargo clippy $(CARGO_FLAGS) --workspace --all-targets --all-features -- -D warnings

# Failing control: clippy must reject every banned macro in lint-control's library code, on
# each probe line with the exact definition path (see lint-control/check.sh).
.PHONY: lint-control
lint-control:
	@sh lint-control/check.sh

# Rustdoc with every warning denied
.PHONY: doc
doc:
	RUSTDOCFLAGS="-D warnings" cargo doc $(CARGO_FLAGS) --workspace --no-deps --all-features

# Fuzz every target for FUZZ_SECONDS each on its corpus (fuzz/, cargo-fuzz, nightly;
# docs/PLAN-improvements.md H5). sample_settlement and ledger_sequence load the program.
FUZZ_TOOLCHAIN ?= nightly
FUZZ_SECONDS ?= 120
FUZZ_TARGETS = settlement_model sample_settlement ledger_sequence
.PHONY: fuzz
fuzz: program
	@for t in $(FUZZ_TARGETS); do \
		echo "fuzz: $$t for $(FUZZ_SECONDS) s"; \
		cargo +$(FUZZ_TOOLCHAIN) fuzz run -O $$t -- -max_total_time=$(FUZZ_SECONDS) || exit 1; \
	done

# Formatting of the fuzz crate (its own workspace, so `fmt-check` does not reach it).
.PHONY: fuzz-fmt-check
fuzz-fmt-check:
	cargo +$(RUST_TOOLCHAIN) fmt --manifest-path fuzz/Cargo.toml -- --check

# Deterministic fault controls: each planted bug in fuzz/controls must crash its target on its
# committed input, and the input must pass without the bug (fuzz/controls/run.sh).
.PHONY: fuzz-control
fuzz-control:
	@FUZZ_TOOLCHAIN=$(FUZZ_TOOLCHAIN) sh fuzz/controls/run.sh

# Pre-push checks: must pass with zero warnings
.PHONY: pre-push
pre-push: fmt-check lint lint-control program-control program quotebook-program pyth-programs test doc

.PHONY: clean
clean:
	cargo clean
