#!/usr/bin/env bash
# Every check a push to main must pass. The first failure stops with its
# output; the last line is `ALL OK: ...` when everything passed. The same as
# tsumugi's scripts/verify.sh, less its checklists and test lanes.
#
# Clippy covers every OS the crates ship for -- Linux, Windows and macOS --
# whatever the host: the host first, then the others. Those are type checks
# only -- they never run that code, so read Windows path strings by eye.
# objc2 compiles a little Objective-C for macOS and ito-store compiles SQLite
# (bundled), so any target that is not the host's own goes through
# cargo-zigbuild, whose zig is a C compiler for it (installed from PyPI below
# when it is missing). Before 2026-10-15 the second clippy
# was always Windows, so a run on the Windows PC never compiled Linux and
# still said ALL OK (yagura's v0.62.0 went red on Linux that way); before
# 2026-10-16 macOS was left to CI, which has no Mac here.
# `cargo fmt --check` only looks; run `cargo fmt` yourself when it fails.

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
    dir=$(cargo metadata -q --no-deps --format-version 1 | sed 's/.*"target_directory":"\([^"]*\)".*/\1/; s/\\\\/\//g')
    run env RUSTUP_TOOLCHAIN=stable ZIG_GLOBAL_CACHE_DIR="$dir/zig" cargo-zigbuild "$@"
}

run cargo fmt --all --check
run cargo build -q --locked --workspace --all-features
run cargo test -q --workspace --all-features
tests=$(echo "$last" | grep 'test result' | awk '{p+=$4; f+=$6} END {print p " passed; " f " failed"}')
run cargo +stable clippy -q --workspace --all-targets --all-features -- -D warnings
for t in $others; do
    case "$t:$host" in
        *-apple-darwin:*-apple-darwin | *-linux-gnu:*-linux-gnu | *-windows-msvc:*-windows-msvc)
            run cargo +stable clippy -q --workspace --all-targets --all-features --target "$t" -- -D warnings ;;
        *) zigbuild clippy -q --workspace --all-targets --all-features --target "$t" -- -D warnings ;;
    esac
done
run cargo +stable clippy -q --workspace --all-targets -- -D warnings
# The lanes' scripts (scripts/lanes/FILES): they run in the apps, so here they
# are only parsed, and the self-tests of merge-lanes.py, ito-first.py, visual-check.py, make-canary.py and evidence.py run;
# check-schemas.py checks docs/schemas/ against its examples and against what those self-tests emit.
for f in scripts/lanes/*.sh; do run bash -n "$f"; done
run python3 scripts/lanes/merge-lanes.py --self-test
run python3 scripts/lanes/ito-first.py --self-test
run python3 scripts/lanes/visual-check.py --self-test
run python3 scripts/lanes/make-canary.py --self-test
run python3 scripts/lanes/evidence.py --self-test
run python3 scripts/check-schemas.py
run bash -n scripts/lanes/lanes.sh
if command -v pwsh >/dev/null; then run pwsh -NoProfile -File scripts/lanes/check-ps1.ps1; fi
# The workflow templates: every {{token}} is one lanes.sh fills, every lanes: block is balanced,
# and each renders to YAML for every combination (tui on/off x plain Linux on/off) through the real lanes.sh sync.
run python3 -c '
import glob, re, sys
for f in glob.glob("scripts/lanes/workflows/*.yml"):
    t = open(f, encoding="utf-8").read()
    for tok in set(re.findall(r"\{\{(\w+)\}\}", t)):
        if tok not in ("app", "crate", "bin", "tui_crate", "tui_bin"):
            sys.exit("%s: unknown token {{%s}}" % (f, tok))
    stack = []
    for n, line in enumerate(t.splitlines(), 1):
        m = re.match(r"^ *# lanes:(begin|end) (\S+)$", line)
        if m is None:
            if "# lanes:" in line:
                sys.exit("%s:%d: malformed lanes: marker" % (f, n))
            continue
        if m.group(1) == "begin":
            stack.append(m.group(2))
        elif not stack or stack.pop() != m.group(2):
            sys.exit("%s:%d: lanes:end %s without its begin" % (f, n, m.group(2)))
    if stack:
        sys.exit("%s: lanes:begin %s is never ended" % (f, stack[-1]))
'
render_combos() {
    local root tmp d tui linux rc
    root=$(pwd)
    tmp=$(mktemp -d)
    for tui in yes no; do
        for linux in yes no; do
            d="$tmp/$tui-$linux"
            mkdir -p "$d/scripts"
            cp scripts/lanes/lanes.sh "$d/scripts/lanes.sh"
            { echo "app=x"; echo "crate=x"; echo "bin=x"; echo "workflows=$(cd scripts/lanes/workflows && ls *.yml | tr '\n' ' ')"
              [ $tui = no ] || { echo "tui_crate=x-tui"; echo "tui_bin=x-tui"; }
              [ $linux = yes ] || echo "release_linux=no"; } > "$d/scripts/lanes.conf"
            (cd "$d" && git init -q . && ITO_DIR="$root" bash scripts/lanes.sh sync >/dev/null && ITO_DIR="$root" bash scripts/lanes.sh check >/dev/null) || { echo "lanes: render tui=$tui linux=$linux failed"; rm -rf "$tmp"; return 1; }
        done
    done
    python3 - "$tmp" <<'PY'
import glob, sys
files = [f.replace("\\", "/") for f in glob.glob(sys.argv[1] + "/*/.github/workflows/*.yml")]  # Windows Python joins with backslashes
for f in files:
    if "# lanes:" in open(f, encoding="utf-8").read():
        sys.exit("%s: a lanes: marker is left" % f)
releases = [f for f in files if f.endswith("/release.yml")]
if len(releases) != 4:
    sys.exit("lanes: release.yml was written in %d of the 4 combinations" % len(releases))
try:
    import yaml
except ImportError:
    sys.exit(0)  # Without PyYAML only the YAML part is skipped.
for f in files:
    d = yaml.safe_load(open(f, encoding="utf-8").read())
    if f.endswith("/release.yml"):
        tui, linux = f.split("/")[-4].split("-")
        jobs = d["jobs"]
        if ("tui" in jobs) != (tui == "yes") or ("tui" in jobs["sums"]["needs"]) != (tui == "yes"):
            sys.exit("%s: tui job and sums.needs disagree" % f)
        names = [e["name"] for e in jobs["unix"]["strategy"]["matrix"]["include"]]
        if (("linux-x64" in names) != (linux == "yes")) or ("macos-arm64" not in names):
            sys.exit("%s: unix matrix is %s" % (f, names))
PY
    rc=$?
    rm -rf "$tmp"
    return $rc
}
run render_combos
echo "ALL OK: $tests"
