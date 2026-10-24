#!/usr/bin/env bash
# Downloads the pinned pdfium-binaries release for a Rust target triple and
# unpacks it into a folder. The library ends up in <dest-dir>/lib on macOS and
# Linux and in <dest-dir>/bin on Windows.
#
# Usage: scripts/fetch-pdfium.sh <rust-target-triple> <dest-dir>
set -euo pipefail

RELEASE="chromium%2F8086"

asset_for() {
  case "$1" in
    aarch64-unknown-linux-gnu) echo "pdfium-linux-arm64.tgz e7e2fe4686925618330103cb167950aca5a84bb00fd977a41b86be59dd1480a2" ;;
    x86_64-unknown-linux-gnu) echo "pdfium-linux-x64.tgz 588577cf52dabc1a444988bac841920df54cc2f141801424de97ab04f4fbb935" ;;
    aarch64-apple-darwin) echo "pdfium-mac-arm64.tgz e98679e052c07edbb5a627980902abb823d4b3f35744d877bd21668bd9fc13ab" ;;
    x86_64-apple-darwin) echo "pdfium-mac-x64.tgz 933a85a138f6027243c56bff8676375c33ceeb767401389415ffc44d689ca85d" ;;
    x86_64-pc-windows-msvc) echo "pdfium-win-x64.tgz 1fd8af952832dbb0eb16d9249f68fe09e5f5ebf7c3dd9f6066ea2720cc28487d" ;;
    *) return 1 ;;
  esac
}

if [ "$#" -ne 2 ]; then
  echo "usage: $0 <rust-target-triple> <dest-dir>" >&2
  exit 2
fi
triple="$1"
dest="$2"

if ! entry="$(asset_for "$triple")"; then
  echo "no pdfium binaries pinned for target: $triple" >&2
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
  "https://github.com/bblanchon/pdfium-binaries/releases/download/${RELEASE}/${asset}"

actual="$(sha256 "$work/$asset")"
if [ "$actual" != "$expected" ]; then
  echo "checksum mismatch for $asset: expected $expected, got $actual" >&2
  exit 1
fi

rm -rf "$dest"
mkdir -p "$dest"
tar -xzf "$work/$asset" -C "$dest"
