#!/usr/bin/env bash
# Write the SHA-256 section of a release page: one row per package (the zips
# and tarballs CI builds, and any .deb, .rpm, .msi or .dmg uploaded by hand),
# then what is inside each Windows zip, file by file. Run from a directory
# holding the downloaded assets; prints Markdown on stdout.
#
# Kept out of release.yml so that it can be run by hand on a folder of files,
# which is the only way to try it before a release is cut.
set -euo pipefail

echo '<!-- checksums -->'
echo
echo '### SHA-256'
echo
echo 'Every package on this release. Compare with `Get-FileHash <file> -Algorithm SHA256` on Windows,'
echo '`shasum -a 256 <file>` on macOS or `sha256sum <file>` on Linux.'
echo
echo '| File | SHA-256 |'
echo '| --- | --- |'
for f in $(ls -1 | grep -E '\.(zip|tar\.gz|deb|rpm|msi|dmg)$' | sort); do
  echo "| \`$f\` | \`$(sha256sum "$f" | cut -d' ' -f1)\` |"
done

shopt -s nullglob
for z in *.zip; do
  d=$(mktemp -d)
  # Exit status 1 is a warning (Windows PowerShell 5 writes `\` separators,
  # which unzip mentions and then handles); 2 and up is a failure.
  unzip -q "$z" -d "$d" || [ $? -eq 1 ]
  echo
  echo "#### Inside \`$z\`"
  echo
  echo 'What a download should hold once extracted. A file missing here, or one with another hash,'
  echo 'is not what CI packed.'
  echo
  echo '| File | Bytes | SHA-256 |'
  echo '| --- | ---: | --- |'
  (cd "$d" && find . -type f | sed 's|^\./||' | sort | while read -r p; do
    echo "| \`$p\` | $(stat -c %s "$p") | \`$(sha256sum "$p" | cut -d' ' -f1)\` |"
  done)
  rm -rf "$d"
done
