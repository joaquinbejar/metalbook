#!/bin/sh
# Fails if a cargo-build-sbf log reports a stack frame above 4 KiB. cargo-build-sbf only prints
# this and still exits 0, while the overflow corrupts memory at run time
# (rules/global_rules.md). Used by `make program`; `make program-control` proves it fires.
set -u
log=$1
if grep -q "overflows the maximum allowed frame space" "$log"; then
    grep "overflows the maximum allowed frame space" "$log"
    echo "program: a stack frame exceeds 4 KiB (see above)"
    exit 1
fi
