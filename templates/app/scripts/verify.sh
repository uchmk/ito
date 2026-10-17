#!/usr/bin/env bash
# Every check a push to main must pass. The first failure stops with its
# output; the last line is `ALL OK: ...` when everything passed.

set -o pipefail
cd "$(dirname "$0")/.." || exit 1

run() {
    local out rc
    out=$("$@" 2>&1)
    rc=$?
    if [ $rc -ne 0 ]; then
        echo "FAILED: $*"
        echo "$out" | tail -30
        exit 1
    fi
    last=$out
}

run cargo fmt --all --check
run cargo build -q --locked --workspace --all-features
run cargo test -q --workspace --all-features
tests=$(echo "$last" | grep 'test result' | awk '{p+=$4; f+=$6} END {print p " passed; " f " failed"}')
run cargo +stable clippy -q --workspace --all-targets --all-features -- -D warnings
# The checklist follows TESTING.md; the bug form is ito's shared one.
run cargo run -q -p {{app}} --example make-testcheck -- --check
run cargo run -q -p {{app}} --example make-bugform -- --check
# The lanes' shared scripts are ito's (scripts/lanes.conf's files=).
run scripts/lanes.sh check
# ito first: no crate ito already stands in for, and every crate an app needs.
run python3 scripts/ito-first.py
echo "ALL OK: $tests"
