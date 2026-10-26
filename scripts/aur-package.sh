#!/usr/bin/env bash
# Renders the AUR files (PKGBUILD and .SRCINFO) from packaging/aur into a
# folder. CI builds the package from that folder, and the release job pushes
# it to the AUR.
#
# Usage: scripts/aur-package.sh <version> <sha256-x86_64> <sha256-aarch64> <out-dir>
set -euo pipefail

if [ "$#" -ne 4 ]; then
  echo "usage: $0 <version> <sha256-x86_64> <sha256-aarch64> <out-dir>" >&2
  exit 2
fi
version="$1"
sha256_x86_64="$2"
sha256_aarch64="$3"
out="$4"

if ! printf '%s' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
  echo "not a plain version (no leading v): $version" >&2
  exit 2
fi
for sha256 in "$sha256_x86_64" "$sha256_aarch64"; do
  if ! printf '%s' "$sha256" | grep -Eq '^[0-9A-Fa-f]{64}$'; then
    echo "not a sha256: $sha256" >&2
    exit 2
  fi
done

templates="$(cd "$(dirname "$0")/../packaging/aur" && pwd)"
mkdir -p "$out"
render() {
  sed -e '/^# Template:/d' \
    -e "s|@VERSION@|$version|g" \
    -e "s|@SHA256_X86_64@|$(printf '%s' "$sha256_x86_64" | tr 'A-F' 'a-f')|g" \
    -e "s|@SHA256_AARCH64@|$(printf '%s' "$sha256_aarch64" | tr 'A-F' 'a-f')|g" \
    "$1" > "$2"
}
render "$templates/PKGBUILD" "$out/PKGBUILD"
render "$templates/SRCINFO" "$out/.SRCINFO"
