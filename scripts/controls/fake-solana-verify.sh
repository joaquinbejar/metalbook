#!/bin/sh
# A stand-in for solana-verify, used only by `make verifiable-control` to drive the real
# `make verifiable` recipe without Docker. FAKE_MODE picks the build's behaviour:
# ok (a binary and a clean log), overflow (a binary, exit 0, and cargo-build-sbf's stack-frame
# diagnostic in the log) or fail (exit 3, with a binary written anyway, so only the exit status
# can catch it). The binary goes to FAKE_OUT.
set -eu
case "$1" in
--version) echo "solana-verify ${FAKE_VERSION}" ;;
build)
    case "${FAKE_MODE}" in
    ok) printf 'fake\n' > "${FAKE_OUT}" ;;
    overflow)
        echo 'Error: Function entrypoint overflows the maximum allowed frame space by accessing an offset 64 bytes greater than the maximum of 4096.'
        printf 'fake\n' > "${FAKE_OUT}"
        ;;
    *)
        printf 'fake\n' > "${FAKE_OUT}"
        echo "fake build failure"
        exit 3
        ;;
    esac
    ;;
get-executable-hash) echo "fakehash" ;;
*) exit 2 ;;
esac
