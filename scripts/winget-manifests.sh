#!/usr/bin/env bash
# Renders the winget manifests from packaging/winget into a folder. CI installs
# from that folder to test the manifests, and the very first submission to
# microsoft/winget-pkgs (komac can only update a package that already exists
# there) is made from it too.
#
# Usage: scripts/winget-manifests.sh <version> <installer-url> <sha256> <out-dir>
set -euo pipefail

if [ "$#" -ne 4 ]; then
  echo "usage: $0 <version> <installer-url> <sha256> <out-dir>" >&2
  exit 2
fi
version="$1"
url="$2"
sha256="$3"
out="$4"

if ! printf '%s' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
  echo "not a plain version (no leading v): $version" >&2
  exit 2
fi
if ! printf '%s' "$sha256" | grep -Eq '^[0-9A-Fa-f]{64}$'; then
  echo "not a sha256: $sha256" >&2
  exit 2
fi
case "$url" in
  https://*) ;;
  *) echo "installer url must be https: $url" >&2; exit 2 ;;
esac
case "$url" in
  *[\|\&\\]*) echo "installer url holds a character sed would misread: $url" >&2; exit 2 ;;
esac

templates="$(cd "$(dirname "$0")/../packaging/winget" && pwd)"
mkdir -p "$out"
for template in "$templates"/*.yaml; do
  sed -e '/^# Template:/d' \
    -e "s|@VERSION@|$version|g" \
    -e "s|@INSTALLER_URL@|$url|g" \
    -e "s|@INSTALLER_SHA256@|$(printf '%s' "$sha256" | tr 'a-f' 'A-F')|g" \
    "$template" > "$out/$(basename "$template")"
done
