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

# Build metalbook-program for SBF into target/deploy. cargo-build-sbf only warns when a
# function's stack frame exceeds 4 KiB, which corrupts memory at run time; fail instead.
.PHONY: program
program:
	@cargo build-sbf --manifest-path metalbook-program/Cargo.toml --arch $(SBF_ARCH) 2>&1 | tee target/build-sbf.log
	@if grep -q "overflows the maximum allowed frame space" target/build-sbf.log; then \
		echo "program: a stack frame exceeds 4 KiB (see above)"; exit 1; fi

# Run the CU benchmark in LiteSVM and regenerate docs/benchmarks.md
.PHONY: bench
bench: program
	METALBOOK_BENCH_OUT=$(CURDIR)/docs/benchmarks.md \
	cargo test -p metalbook-tests --release --test cu -- --nocapture

# Run every test, including the property tests and the LiteSVM tests (which need
# `make program` first). METALBOOK_PROPTEST_CASES=<n> overrides the case count.
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
pre-push: fmt-check lint lint-control program test doc

.PHONY: clean
clean:
	cargo clean
