#!/usr/bin/env bash
# Downloads the model of the built-in AI provider into a folder, checked against
# its pinned size and checksum. The app asks before it downloads the model
# itself; this script is for CI and for the tests.
#
# The same pin is in crates/ncrs-ai/src/model_file.rs (LOCAL_MODEL). Change
# both together.
#
# Usage: scripts/fetch-ai-model.sh <dest-dir>
set -euo pipefail

NAME="Qwen3-0.6B-Q8_0.gguf"
# Pinned to a revision of the Hugging Face repository, so the file cannot change.
URL="https://huggingface.co/Qwen/Qwen3-0.6B-GGUF/resolve/23749fefcc72300e3a2ad315e1317431b06b590a/$NAME"
SIZE=639446688
SHA256="9465e63a22add5354d9bb4b99e90117043c7124007664907259bd16d043bb031"

if [ "$#" -ne 1 ]; then
  echo "usage: $0 <dest-dir>" >&2
  exit 2
fi
dest="$1"

if command -v sha256sum >/dev/null 2>&1; then
  sha256() { sha256sum "$1" | cut -d' ' -f1; }
else
  sha256() { shasum -a 256 "$1" | cut -d' ' -f1; }
fi
size_of() { wc -c < "$1" | tr -d ' '; }

# A cached copy (CI cache hit) is kept when it is the right file.
if [ -f "$dest/$NAME" ] && [ "$(size_of "$dest/$NAME")" = "$SIZE" ] \
  && [ "$(sha256 "$dest/$NAME")" = "$SHA256" ]; then
  echo "$NAME is already in $dest"
  exit 0
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

mkdir -p "$dest"
curl --fail --silent --show-error --location \
  --max-filesize "$SIZE" \
  --output "$work/$NAME" \
  "$URL"
if [ "$(size_of "$work/$NAME")" != "$SIZE" ]; then
  echo "size mismatch for $NAME: expected $SIZE, got $(size_of "$work/$NAME")" >&2
  exit 1
fi
actual="$(sha256 "$work/$NAME")"
if [ "$actual" != "$SHA256" ]; then
  echo "checksum mismatch for $NAME: expected $SHA256, got $actual" >&2
  exit 1
fi
mv "$work/$NAME" "$dest/$NAME"
