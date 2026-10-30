# Makefile for the metalbook workspace. Targets mirror matchbook's; `pre-push` follows
# rules/global_rules.md (fmt check, clippy -D warnings, lint control, tests, docs -D warnings).

.PHONY: all
all: fmt-check lint test

# sBPF target version for the program (cargo-build-sbf's default is v3).
SBF_ARCH ?= v3

# Build the workspace
.PHONY: build
build:
	cargo build --workspace

# Build metalbook-program for SBF into target/deploy. cargo-build-sbf's exit status is kept (no
# pipe), and scripts/check-sbf-log.sh fails the build on a stack frame above 4 KiB, which
# cargo-build-sbf only warns about.
.PHONY: program
program:
	@mkdir -p target
	@cargo build-sbf --manifest-path metalbook-program/Cargo.toml --arch $(SBF_ARCH) \
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
		--arch $(SBF_ARCH) --sbf-out-dir $(CURDIR)/target/deploy > target/build-quotebook.log 2>&1; status=$$?; \
		if [ $$status -ne 0 ]; then cat target/build-quotebook.log; echo "quotebook-program: build failed ($$status)"; exit $$status; fi
	@sh scripts/check-sbf-log.sh target/build-quotebook.log

# The Pyth receiver and the Wormhole core bridge as deployed on devnet (the milestone 7b-2 LiteSVM
# test posts a recorded Hermes update through them), dumped into target/deploy and checked
# against the hashes below; a redeployment changes the hash and fails here, not in the test.
PYTH_RPC ?= https://api.devnet.solana.com
PYTH_RECEIVER_SHA256 := 0d6bf9142c2eb1fdb8ead48054491369bed3a8d6846c5887e362844155599ab2
WORMHOLE_SHA256 := ca237355a04cc5ceeb788853833ae46c0cb8b0a728625e8f9bb9bc05d1e9c619
.PHONY: pyth-programs
pyth-programs:
	@mkdir -p target/deploy
	@solana program dump -u $(PYTH_RPC) rec5EKMGg6MxZYaMdyBfgwp4d5rB9T1VQH5pJv5LtFJ \
		target/deploy/pyth_receiver.so > /dev/null
	@solana program dump -u $(PYTH_RPC) HDwcJBJXjL9FpJ7UBsYBtaDjsBUhuLCUYoz3zr8SWWaQ \
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
	cargo test -p metalbook-tests --release --test cu -- --nocapture

# Run every test, including the property tests and the LiteSVM tests (which need
# `make program`, `make quotebook-program` and `make pyth-programs` first). METALBOOK_PROPTEST_CASES=<n> overrides the case count.
.PHONY: test
test:
	cargo test --workspace --all-features

# Format the code
.PHONY: fmt
fmt:
	cargo +stable fmt --all

# Check formatting
.PHONY: fmt-check
fmt-check:
	cargo +stable fmt --all --check

# Clippy with every warning denied
.PHONY: lint
lint:
	cargo clippy --workspace --all-targets --all-features -- -D warnings

# Failing control: clippy must reject every banned macro in lint-control's library code, on
# each probe line with the exact definition path (see lint-control/check.sh).
.PHONY: lint-control
lint-control:
	@sh lint-control/check.sh

# Rustdoc with every warning denied
.PHONY: doc
doc:
	RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features

# Pre-push checks: must pass with zero warnings
.PHONY: pre-push
pre-push: fmt-check lint lint-control program-control program quotebook-program pyth-programs test doc

.PHONY: clean
clean:
	cargo clean
