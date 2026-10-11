#!/usr/bin/env bash
# Source is uchmk/ito scripts/lanes/; an app's copy is written by its
# scripts/lanes.sh sync, so edit ito's.
# Claims for the development routine's overlapping runs: a run writes which
# TODO.md item it is on into the `claims` branch on GitHub, and the other runs
# leave that item alone (.claude/dev-routine.md, 4).
#
#   scripts/claim.sh take <TODO.md line>   claim the item that starts there;
#                                          exit 1 when another run holds it
#   scripts/claim.sh take '#N'             the same for issue N (a finding or
#                                          an answered question, v0.102.0)
#   scripts/claim.sh drop                  release this run's claim
#   scripts/claim.sh list                  the claims held now, with their age
#
# A run starts every hour at :30, but a long one, or an interactive session,
# may still be at work when the next starts: a run may start while the one before is still pushing, from a `main` without that
# work. Picking items by position alone collides when an item is added above
# (2026-10-05).
#
# The branch holds one commit with one file, `claims`: a line per claim,
# "<key> <unix time> <item>". Every change replaces that commit with
# `--force-with-lease` naming the commit it read, so of two runs writing at
# once, one is refused, reads again and retries. One branch rather than a
# branch per claim: a cloud session can create branches but not delete them
# (403), and the owner reads the graph in SourceTree. The branch runs no
# workflow (they all build `main` or pull requests only).
#
# The key is a hash of the item's first line, so it survives line numbers
# moving. A claim older than an hour belongs to a run that died, and lapses.

set -uo pipefail
cd "$(dirname "$0")/.." || exit 2

# The app's name, from scripts/lanes.conf beside this file.
app=$(sed -n 's/^app=//p' scripts/lanes.conf)
state="$(git rev-parse --git-dir)/${app:?scripts/lanes.conf has no app=}-claim"
stale=3600
ref=refs/heads/claims

key() { sed -n "${1}p" TODO.md | sed 's/^[[:space:]]*- \[.\] //' | sha1sum | cut -c1-10; }

# Read the branch into $old (its commit, or empty) and $held (the live lines).
read_claims() {
    old=$(git ls-remote origin "$ref" | cut -f1)
    held=
    [ -n "$old" ] || return 0
    git fetch -q origin "+$ref:refs/claims/head" || return 1
    local now; now=$(date +%s)
    held=$(git show refs/claims/head:claims 2>/dev/null | awk -v now="$now" -v s="$stale" 'now - $2 < s')
}

# Replace the branch with one holding $1 as the file; fails if it moved.
write_claims() {
    local blob tree commit
    blob=$(printf '%s\n' "$1" | sed '/^$/d' | git hash-object -w --stdin)
    tree=$(printf '100644 blob %s\tclaims\n' "$blob" | git mktree)
    commit=$(git commit-tree "$tree" -m "claims")
    git push -q --force-with-lease="$ref:$old" origin "$commit:$ref" 2>/dev/null
}

case "${1:-}" in
take)
    line=${2:?usage: claim.sh take <TODO.md line> | '#N'}
    case "$line" in
    \#*[0-9])
        k="issue-${line#\#}"
        text="- [ ] issue $line" ;;
    *)
        text=$(sed -n "${line}p" TODO.md)
        case "$text" in *"- [ ] "*) ;; *) echo "claim: TODO.md:$line is not an open item: $text"; exit 2 ;; esac
        k=$(key "$line") ;;
    esac
    for _ in 1 2 3 4 5; do
        read_claims || { echo "claim: could not read the claims branch"; exit 2; }
        if printf '%s\n' "$held" | grep -q "^$k "; then
            echo "claim: another run is on this item; take the next one"
            exit 1
        fi
        if write_claims "$held"$'\n'"$k $(date +%s) ${text#*- \[ \] }"; then
            echo "$k" > "$state"
            echo "claim: took $k"
            exit 0
        fi
        sleep $((RANDOM % 3 + 1))
    done
    echo "claim: the claims branch kept changing; take the next item"
    exit 1
    ;;
drop)
    [ -f "$state" ] || exit 0
    k=$(cat "$state")
    for _ in 1 2 3 4 5; do
        read_claims || break
        write_claims "$(printf '%s\n' "$held" | grep -v "^$k ")" && break
        sleep $((RANDOM % 3 + 1))
    done
    rm -f "$state"
    ;;
list)
    read_claims && now=$(date +%s) && printf '%s\n' "$held" | sed '/^$/d' |
        while read -r k t rest; do echo "$k  $(( (now - t) / 60 )) min  ${rest:0:80}"; done
    ;;
*)
    sed -n '2,11p' "$0"
    exit 2
    ;;
esac
