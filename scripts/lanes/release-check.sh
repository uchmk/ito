#!/usr/bin/env bash
# Is a release finished? Run after release.yml succeeds: `bash scripts/release-check.sh vX.Y.Z`.
# "The workflow succeeded" is not the end: the six assets, the SHA-256 table
# (release-sums.yml) and the notes are each checked here, one line per check.
# Exit 0 only when all pass; a failing line says what is left.
# Needs `gh` signed in. `REPO=owner/name` overrides the repository of the checkout.
set -uo pipefail

tag=${1:?usage: release-check.sh vX.Y.Z}
repo=${REPO:-$(gh repo view --json nameWithOwner --jq .nameWithOwner)}
fail=0
say() { printf '%-6s %s\n' "$1" "$2"; [ "$1" = NG ] && fail=1; }

out=$(gh release view "$tag" --repo "$repo" --json tagName 2>&1) || { echo "NG     no release $tag: $out"; exit 1; }
assets=$(gh release view "$tag" --repo "$repo" --json assets --jq '.assets[].name' | sort)
# What CI builds (counted below), and what the SHA-256 table lists: those plus
# packages uploaded by hand. Keep `listed` in step with release-sums.sh.
names=$(printf '%s\n' "$assets" | grep -E '\.(zip|tar\.gz)$')
listed=$(printf '%s\n' "$assets" | grep -E '\.(zip|tar\.gz|deb|rpm|msi|dmg)$')
body=$(gh release view "$tag" --repo "$repo" --json body --jq .body)

# 1. Six assets: Windows x64 / ARM64 (.zip), macOS and Linux x64 / ARM64 (.tar.gz);
# two more (Linux .tar.gz) when lanes.conf has tui_bin.
want=6; tar=4
if [ -n "$(sed -n 's/^tui_bin=//p' scripts/lanes.conf 2>/dev/null | head -n 1)" ]; then want=8; tar=6; fi
n=$(printf '%s\n' "$names" | grep -c . || true)
zips=$(printf '%s\n' "$names" | grep -c '\.zip$' || true)
if [ "$n" -eq "$want" ] && [ "$zips" -eq 2 ]; then say OK "assets: $want (2 zip + $tar tar.gz)"; else say NG "assets: $n ($zips zip); want $want = 2 zip + $tar tar.gz"; fi

# 2. The SHA-256 table (release-sums.yml, called from release.yml or run by hand)
# is in the notes, with a row for every package (a .deb uploaded after the
# workflow ran needs it run again). Without it, run the workflow.
if printf '%s' "$body" | grep -q '^<!-- checksums -->'; then
  missing=$(printf '%s\n' "$listed" | while read -r f; do [ -n "$f" ] && ! printf '%s' "$body" | grep -qF "\`$f\`" && echo "$f"; done)
  if [ -z "$missing" ]; then say OK "SHA-256 table: a row for every package"; else say NG "SHA-256 table: no row for: $(echo $missing)"; fi
else
  say NG "SHA-256 table: not in the notes (gh workflow run release-sums.yml -f tag=$tag; gh run watch; gh run view --log-failed)"
fi

# 3. The notes only hold what changed since the previous release.
prev=$(gh release list --repo "$repo" --limit 5 --json tagName --jq '.[].tagName' | grep -vxF "$tag" | head -1)
# Only the release's own notes count: the lines before the fixed tail an app
# appends (from `### Platforms`: Platforms, Signing, ...) or the SHA-256 table,
# whichever comes first. Up to 15 lines pass. Longer notes pass when they open
# with `## Highlights` (the 3-5 main changes, up to 10 lines under the heading)
# and keep the per-version notes under `## All changes since vPREV`.
own=$(printf '%s\n' "$body" | tr -d '\r' | sed -e '/^### Platforms/,$d' -e '/^<!-- checksums -->$/,$d')
lines=$(printf '%s\n' "$own" | grep -c . || true)
high=
case "$(printf '%s\n' "$own" | grep -m 1 .)" in
  "## Highlights"*) high=$(printf '%s\n' "$own" | awk 'seen && /^## /{exit} !seen && /^## Highlights/{seen=1; next} seen && /./{n++} END{print n+0}') ;;
esac
since=${prev:-the start}
if [ "$lines" -le 15 ]; then
  say OK "notes: $lines lines since $since"
elif [ -n "$high" ] && [ "$high" -le 10 ]; then
  say OK "notes: $lines lines since $since, led by $high lines of Highlights"
elif [ -n "$high" ]; then
  say WARN "Highlights: $high lines; keep it to the 3-5 main changes (gh release edit $tag --notes-file ...)"
else
  say WARN "notes: $lines lines since $since; put \`## Highlights\` (the 3-5 main changes) first and the per-version notes under \`## All changes since $since\` (gh release edit $tag --notes-file ...)"
fi

[ "$fail" -eq 0 ] && echo "RELEASE OK: $tag" || echo "RELEASE INCOMPLETE: $tag"
exit "$fail"
