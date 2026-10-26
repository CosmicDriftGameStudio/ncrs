#!/usr/bin/env bash
# Downloads the pinned komac release for the machine it runs on and unpacks it
# into a folder; the binary is <dest-dir>/komac. komac submits the winget
# manifest update for a release to microsoft/winget-pkgs (release.yml, job
# `winget`). 2.16.0 rather than the newest: a release is only pinned once it
# is a few weeks old.
#
# Usage: scripts/fetch-komac.sh <dest-dir>
set -euo pipefail

VERSION="2.16.0"

asset_for() {
  case "$1" in
    Linux-x86_64) echo "komac-${VERSION}-x86_64-unknown-linux-gnu.tar.gz 7d2707fa6210f2789a3702de49fbd150b736dbf426ee0b9bc8e098736f9fd82d" ;;
    Darwin-arm64) echo "komac-${VERSION}-aarch64-apple-darwin.tar.gz d75880e72d12c25c41f8e5db278ce7583e3efa5794c1c3347090e7e2c7247788" ;;
    *) return 1 ;;
  esac
}

if [ "$#" -ne 1 ]; then
  echo "usage: $0 <dest-dir>" >&2
  exit 2
fi
dest="$1"

host="$(uname -s)-$(uname -m)"
if ! entry="$(asset_for "$host")"; then
  echo "no komac pinned for host: $host" >&2
  exit 2
fi
asset="${entry%% *}"
expected="${entry##* }"

if command -v sha256sum >/dev/null 2>&1; then
  sha256() { sha256sum "$1" | cut -d' ' -f1; }
else
  sha256() { shasum -a 256 "$1" | cut -d' ' -f1; }
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

curl --fail --silent --show-error --location \
  --output "$work/$asset" \
  "https://github.com/russellbanks/Komac/releases/download/v${VERSION}/${asset}"

actual="$(sha256 "$work/$asset")"
if [ "$actual" != "$expected" ]; then
  echo "checksum mismatch for $asset: expected $expected, got $actual" >&2
  exit 1
fi

rm -rf "$dest"
mkdir -p "$dest"
tar -xzf "$work/$asset" -C "$dest"
test -x "$dest/komac" || { echo "error: no komac binary in $asset" >&2; exit 1; }
