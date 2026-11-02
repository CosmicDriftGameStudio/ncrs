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

NAME="Qwen3-4B-Q4_K_M.gguf"
# Pinned to a revision of the Hugging Face repository, so the file cannot change.
URL="https://huggingface.co/Qwen/Qwen3-4B-GGUF/resolve/bc640142c66e1fdd12af0bd68f40445458f3869b/$NAME"
SIZE=2497280256
SHA256="7485fe6f11af29433bc51cab58009521f205840f5b4ae3a32fa7f92e8534fdf5"

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
