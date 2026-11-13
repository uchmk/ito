#!/usr/bin/env bash
# Source is uchmk/ito scripts/lanes/; an app's copy is written by its scripts/lanes.sh sync, so edit ito's.
#
# The lanes' shared scripts (merge-lanes.py, claim.sh, push-main.sh, xrun.sh,
# keys.ps1 and the rest that ito's scripts/lanes/FILES names, this one too)
# are kept in uchmk/ito, the same bytes for every app; the app's own values are
# in scripts/lanes.conf. The app's scripts/ holds copies, because the lanes and
# the workflows run them from a checkout of the app alone. Edit them in ito,
# push, move the ito `rev` in the app's Cargo.toml files (cargo build), then
# sync.
#
#   scripts/lanes.sh sync    copy them from ito at the app's rev into scripts/
#   scripts/lanes.sh check   exit 1 when a copy differs (scripts/verify.sh runs it)
#
# ito is cargo's checkout of the rev every ito crate the app uses is pinned to
# (they must all be the same one). ITO_DIR=../ito takes a local checkout
# instead, to try a change before ito is pushed. lanes.conf's `files=` names
# the ones the app takes when it does not take them all; this file always.
#
# Workflows: ito's scripts/lanes/workflows/*.yml are templates with {{app}},
# {{crate}} and {{bin}} (lanes.conf's values; {{tui_crate}} and {{tui_bin}} too,
# empty unless the app has a terminal version). A template can mark blocks that
# are left out: `# lanes:begin <name>` ... `# lanes:end <name>` on lines of their
# own (the markers never reach the written file). The blocks: `tui` (kept when
# lanes.conf has a non-empty tui_crate), `no-tui` (kept when it has not), and
# `linux` (release.yml's two plain Linux tarballs; left out by `release_linux=no`,
# anything else or absent keeps them). lanes.conf's `workflows=` names
# the ones the app takes; sync writes them to .github/workflows/ with a header
# line, and check fails when a written copy differs. An app that names none
# keeps its own workflows.

set -euo pipefail
cd "$(dirname "$0")/.."

if [ -z "${ITO_DIR:-}" ]; then
    # cargo's checkout: <...>/checkouts/ito-<hash>/<short rev>/crates/<crate>/Cargo.toml
    ITO_DIR=$(cargo metadata -q --locked --format-version 1 | python3 -c '
import json, os, sys
ito = {p["source"]: p["manifest_path"] for p in json.load(sys.stdin)["packages"]
       if (p.get("source") or "").startswith("git+https://github.com/uchmk/ito")}
if len(ito) != 1:
    sys.exit("lanes: the ito crates are at %d revs, not one: %s" % (len(ito), " ".join(sorted(ito)) or "none"))
print(os.path.dirname(os.path.dirname(os.path.dirname(next(iter(ito.values()))))))') || exit 2
fi
src="$ITO_DIR/scripts/lanes"
[ -f "$src/FILES" ] || { echo "lanes: $src/FILES is missing (is ito older than v0.7.0?)" >&2; exit 2; }
all=$(grep -v '^#' "$src/FILES" | grep .)
files=$(sed -n 's/^files=//p' scripts/lanes.conf 2>/dev/null | tr ' ' '\n' | grep . || true)
if [ -z "$files" ]; then
    files=$all
else
    files=$(printf 'lanes.sh\n%s\n' "$files" | sort -u)
    for f in $files; do
        echo "$all" | grep -qx "$f" || { echo "lanes: scripts/lanes.conf names $f, which ito's FILES does not" >&2; exit 2; }
    done
fi

conf() { sed -n "s/^$1=//p" scripts/lanes.conf 2>/dev/null | head -n 1; }
workflows=$(conf workflows | tr ' ' '
' | grep . || true)
for w in $workflows; do
    [ -f "$src/workflows/$w" ] || { echo "lanes: scripts/lanes.conf names workflow $w, which ito's workflows/ does not have" >&2; exit 2; }
done
# One workflow, as it is written for this app. A block in a template is
# `# lanes:begin <name>` ... `# lanes:end <name>` on lines of their own: the
# block is deleted, markers included, when its switch is off, and only the two
# marker lines are deleted when it is on, so a written workflow has none.
# A template whose begins and ends do not pair up is refused before any of it
# is cut (a lone begin would otherwise cut the file to its end).
balanced() {
    awk '/^ *# lanes:(begin|end) / { if ($2 == "lanes:begin") s[++d] = $3; else if (d > 0 && s[d] == $3) d--; else { bad = 1; exit } }
        END { exit (bad || d > 0) }' "$1"
}
leave_out() { sed -e "/^ *# lanes:begin $1\$/,/^ *# lanes:end $1\$/d"; }
keep_in() { sed -e "/^ *# lanes:begin $1\$/d" -e "/^ *# lanes:end $1\$/d"; }
render() {
    local tui=keep_in no_tui=leave_out linux=keep_in
    [ -n "$(conf tui_crate)" ] || { tui=leave_out; no_tui=keep_in; }
    [ "$(conf release_linux)" != no ] || linux=leave_out
    balanced "$src/workflows/$1" || { echo "lanes: $1 has a lanes:begin and lanes:end that do not pair up" >&2; return 1; }
    local out
    out=$(sed -e "s/{{app}}/$(conf app)/g" -e "s/{{crate}}/$(conf crate)/g" -e "s/{{bin}}/$(conf bin)/g" -e "s/{{tui_crate}}/$(conf tui_crate)/g" -e "s/{{tui_bin}}/$(conf tui_bin)/g" "$src/workflows/$1" \
        | $tui tui | $no_tui no-tui | $linux linux)
    case "$out" in *'# lanes:'*) echo "lanes: $1 has a lanes: marker this lanes.sh does not know" >&2; return 1 ;; esac
    echo "# Source is uchmk/ito scripts/lanes/workflows/$1; written by scripts/lanes.sh sync, so edit ito's."
    printf '%s\n' "$out"
}

case "${1:-}" in
    sync)
        for f in $files; do
            # A new file moved over the old one: bash is still reading this one.
            cp "$src/$f" "scripts/$f.new"
            case $f in *.sh|*.py) chmod +x "scripts/$f.new" ;; esac
            mv -f "scripts/$f.new" "scripts/$f"
        done
        for w in $workflows; do
            mkdir -p .github/workflows
            render "$w" > ".github/workflows/$w.new"
            mv -f ".github/workflows/$w.new" ".github/workflows/$w"
        done
        echo "lanes: $(echo "$files" | wc -l) files and $(echo "$workflows" | grep -c . || true) workflows from $src" ;;
    check)
        bad=0
        for f in $files; do
            cmp -s "$src/$f" "scripts/$f" || { echo "lanes: scripts/$f differs from ito's ($src/$f)"; bad=1; }
        done
        for w in $workflows; do
            render "$w" | cmp -s - ".github/workflows/$w" || { echo "lanes: .github/workflows/$w differs from ito's ($src/workflows/$w)"; bad=1; }
        done
        [ $bad = 0 ] || { echo "lanes: edit them in ito, or run scripts/lanes.sh sync"; exit 1; } ;;
    *)
        echo "usage: scripts/lanes.sh sync|check" >&2; exit 2 ;;
esac
