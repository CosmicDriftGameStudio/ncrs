#!/usr/bin/env bash
# Downloads the two OCR models of the ocrs project into a folder, each checked
# against its pinned size and checksum. The app asks before it downloads them
# itself; this script is for CI and for the tests.
#
# The same pins are in crates/ncrs-intake/src/ocr.rs (OCR_MODEL_FILES). Change
# both together.
#
# Usage: scripts/fetch-ocr-models.sh <dest-dir>
set -euo pipefail

BASE_URL="https://ocrs-models.s3-accelerate.amazonaws.com"

# name size sha256
MODELS="
text-detection.onnx 2499479 a917b23dbd9524b465df7e922641b3ff2981623df4ded5a0234004ef2fee7cfe
text-recognition.onnx 9713177 86c145c2edb96c8caed5b1ebb8f44d706408922211451c309c625157dd6061c5
"

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

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

mkdir -p "$dest"
echo "$MODELS" | while read -r name size expected; do
  [ -n "$name" ] || continue
  curl --fail --silent --show-error --location \
    --max-filesize "$size" \
    --output "$work/$name" \
    "$BASE_URL/$name"
  if [ "$(size_of "$work/$name")" != "$size" ]; then
    echo "size mismatch for $name: expected $size, got $(size_of "$work/$name")" >&2
    exit 1
  fi
  actual="$(sha256 "$work/$name")"
  if [ "$actual" != "$expected" ]; then
    echo "checksum mismatch for $name: expected $expected, got $actual" >&2
    exit 1
  fi
  mv "$work/$name" "$dest/$name"
done
