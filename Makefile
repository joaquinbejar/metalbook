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

# Build the quotebook program at the pinned quotebook-sdk rev (for the M5 LiteSVM demo) into
# target/deploy/quotebook_program.so, from cargo's checkout of the git dependency.
.PHONY: quotebook-program
quotebook-program:
	@mkdir -p target
	@QB=$$(cargo metadata --format-version 1 | python3 -c 'import json,sys,os; print(next(os.path.dirname(os.path.dirname(p["manifest_path"])) for p in json.load(sys.stdin)["packages"] if p["name"]=="quotebook-sdk"))'); \
		CARGO_TARGET_DIR=$(CURDIR)/target/quotebook cargo build-sbf --manifest-path $$QB/program/Cargo.toml \
		--arch $(SBF_ARCH) --sbf-out-dir $(CURDIR)/target/deploy > target/build-quotebook.log 2>&1; status=$$?; \
		if [ $$status -ne 0 ]; then cat target/build-quotebook.log; echo "quotebook-program: build failed ($$status)"; exit $$status; fi
	@sh scripts/check-sbf-log.sh target/build-quotebook.log

# Failing controls for `program`: the log check must reject a frame-overflow line, and a
# failing cargo-build-sbf (an unknown sBPF arch) must make `make program` fail.
.PHONY: program-control
program-control:
	@mkdir -p target
	@printf '%s\n' 'Error: Function entrypoint overflows the maximum allowed frame space by accessing an offset 64 bytes greater than the maximum of 4096.' \
		> target/sbf-control.log
	@if sh scripts/check-sbf-log.sh target/sbf-control.log > /dev/null; then \
		echo "program-control: the log check accepted a frame overflow"; exit 1; fi
	@if $(MAKE) --no-print-directory program SBF_ARCH=no-such-arch > /dev/null 2>&1; then \
		echo "program-control: make program succeeded with a failing cargo-build-sbf"; exit 1; fi
	@echo "program-control: a frame overflow and a failing cargo-build-sbf both fail make program"

# Run the CU benchmark in LiteSVM and regenerate docs/benchmarks.md
.PHONY: bench
bench: program
	METALBOOK_BENCH_OUT=$(CURDIR)/docs/benchmarks.md \
	cargo test -p metalbook-tests --release --test cu -- --nocapture

# Run every test, including the property tests and the LiteSVM tests (which need
# `make program` and `make quotebook-program` first). METALBOOK_PROPTEST_CASES=<n> overrides the case count.
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
pre-push: fmt-check lint lint-control program-control program quotebook-program test doc

.PHONY: clean
clean:
	cargo clean
