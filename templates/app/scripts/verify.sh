#!/usr/bin/env bash
# Every check a push to main must pass. The first failure stops with its
# output; the last line is `ALL OK: ...` when everything passed. The same
# steps as ito's, kura's and tsumugi's, plus this app's own.
#
# Clippy covers every OS the app ships for -- Linux, Windows and macOS --
# whatever the host: the host first, then the others. Those are type checks
# only -- they never run that code, so read Windows path strings by eye.
# objc2 compiles a little Objective-C for macOS, so off a Mac that clippy
# goes through cargo-zigbuild, whose zig is a C compiler for it (installed
# from PyPI below when it is missing).
# `cargo fmt --check` only looks; run `cargo fmt --all` yourself when it fails.

set -o pipefail
cd "$(dirname "$0")/.." || exit 1

host=$(rustc -vV | sed -n 's/^host: //p')
others=
for t in x86_64-unknown-linux-gnu x86_64-pc-windows-msvc aarch64-apple-darwin; do
    [ "$t" = "$host" ] && continue
    others="$others $t"
    rustup target list --installed 2>/dev/null | grep -qx "$t" || rustup target add "$t" >/dev/null 2>&1
done

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

# cargo-zigbuild comes from PyPI with zig. Where pip may not touch the system
# Python (PEP 668) it goes to ~/.local/bin. zig's cache goes with Cargo's
# output (on the owner's PC the RAM disk, not the SSD).
zigbuild() {
    if ! command -v cargo-zigbuild >/dev/null 2>&1; then
        python3 -m pip install -q cargo-zigbuild >/dev/null 2>&1 \
            || python3 -m pip install -q --user --break-system-packages cargo-zigbuild >/dev/null 2>&1
        PATH="$PATH:$HOME/.local/bin"
        command -v cargo-zigbuild >/dev/null 2>&1 || { echo "FAILED: no cargo-zigbuild (pip install cargo-zigbuild)"; exit 1; }
    fi
    local dir
    dir=$(cargo metadata -q --no-deps --format-version 1 | sed 's/.*"target_directory":"\([^"]*\)".*/\1/; s/\\/\//g')
    run env RUSTUP_TOOLCHAIN=stable ZIG_GLOBAL_CACHE_DIR="$dir/zig" cargo-zigbuild "$@"
}

run cargo fmt --all --check
run cargo build -q --locked --workspace --all-features
run cargo test -q --workspace --all-features
tests=$(echo "$last" | grep 'test result' | awk '{p+=$4; f+=$6} END {print p " passed; " f " failed"}')
run cargo +stable clippy -q --workspace --all-targets --all-features -- -D warnings
for t in $others; do
    case "$t:$host" in
        *-apple-darwin:*-apple-darwin | *-linux-gnu:* | *-windows-msvc:*)
            run cargo +stable clippy -q --workspace --all-targets --all-features --target "$t" -- -D warnings ;;
        *) zigbuild clippy -q --workspace --all-targets --all-features --target "$t" -- -D warnings ;;
    esac
done
run cargo +stable clippy -q --workspace --all-targets -- -D warnings
# Known vulnerabilities (RustSec) in the tree, as the Audit workflow runs
# weekly. Warnings (an unmaintained crate deep in the tree) do not fail.
command -v cargo-audit >/dev/null 2>&1 || run cargo install cargo-audit --locked
run cargo audit
# The checklist follows TESTING.md; the bug form is ito's shared one.
run cargo run -q -p {{app}} --example make-testcheck -- --check
run cargo run -q -p {{app}} --example make-bugform -- --check
# The lanes' shared scripts are ito's (scripts/lanes.conf's files=).
run scripts/lanes.sh check
# ito first: no crate ito already stands in for, and every crate an app needs.
run python3 scripts/ito-first.py
echo "ALL OK: $tests"
