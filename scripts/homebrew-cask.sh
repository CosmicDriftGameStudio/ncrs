#!/usr/bin/env bash
# Renders the Homebrew cask from packaging/homebrew into a folder. CI checks
# the result, and the release job opens the pull request in the tap with it.
#
# Usage: scripts/homebrew-cask.sh <version> <sha256-arm> <sha256-intel> <out-dir>
set -euo pipefail

if [ "$#" -ne 4 ]; then
  echo "usage: $0 <version> <sha256-arm> <sha256-intel> <out-dir>" >&2
  exit 2
fi
version="$1"
sha256_arm="$2"
sha256_intel="$3"
out="$4"

if ! printf '%s' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
  echo "not a plain version (no leading v): $version" >&2
  exit 2
fi
for sha256 in "$sha256_arm" "$sha256_intel"; do
  if ! printf '%s' "$sha256" | grep -Eq '^[0-9A-Fa-f]{64}$'; then
    echo "not a sha256: $sha256" >&2
    exit 2
  fi
done

template="$(cd "$(dirname "$0")/../packaging/homebrew" && pwd)/ncrs.rb"
mkdir -p "$out"
sed -e '/^# Template:/,/^cask /{/^#/d;}' \
  -e "s|@VERSION@|$version|g" \
  -e "s|@SHA256_ARM@|$(printf '%s' "$sha256_arm" | tr 'A-F' 'a-f')|g" \
  -e "s|@SHA256_INTEL@|$(printf '%s' "$sha256_intel" | tr 'A-F' 'a-f')|g" \
  "$template" > "$out/ncrs.rb"
