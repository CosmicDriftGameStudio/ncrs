#!/usr/bin/env bash
# Downloads the pinned nfpm release for the machine it runs on and unpacks it
# into a folder; the binary is <dest-dir>/nfpm. nfpm builds the .deb and .rpm
# in scripts/package.sh without dpkg or rpmbuild, so one runner image does both.
#
# Usage: scripts/fetch-nfpm.sh <dest-dir>
set -euo pipefail

VERSION="2.47.0"

asset_for() {
  case "$1" in
    Linux-x86_64) echo "nfpm_${VERSION}_Linux_x86_64.tar.gz 0660ca602b2d2d2ae4781a06c692b3eeb9d437ffea05b831d76e41f4a3188783" ;;
    Linux-aarch64 | Linux-arm64) echo "nfpm_${VERSION}_Linux_arm64.tar.gz 1c0f5f2999b9a974bfb04fdb0cc3306096de530ac5dbb25d739cc5f5219c919c" ;;
    Darwin-arm64) echo "nfpm_${VERSION}_Darwin_arm64.tar.gz e8c9d1d9ac218eeed479375143dc46b8d51a2b8dbba8e2f9f15ecc8faa2e404b" ;;
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
  echo "no nfpm pinned for host: $host" >&2
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
  "https://github.com/goreleaser/nfpm/releases/download/v${VERSION}/${asset}"

actual="$(sha256 "$work/$asset")"
if [ "$actual" != "$expected" ]; then
  echo "checksum mismatch for $asset: expected $expected, got $actual" >&2
  exit 1
fi

rm -rf "$dest"
mkdir -p "$dest"
tar -xzf "$work/$asset" -C "$dest"
test -x "$dest/nfpm" || { echo "error: no nfpm binary in $asset" >&2; exit 1; }
