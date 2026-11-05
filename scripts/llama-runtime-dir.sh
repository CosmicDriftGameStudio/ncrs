#!/usr/bin/env bash
# Prints the folder with the llama.cpp runtime of a release build: the shared
# libraries (lib/ on Linux, bin/ on Windows) and the backends/ folder that
# llama-cpp-sys-2 leaves in its OUT_DIR. `package.sh archive --llama` takes it
# for Windows and x86_64 Linux.
#
# Usage: scripts/llama-runtime-dir.sh <target-triple> [cargo build options]
#
# The options must be those of the build, so cargo finds it up to date and only
# reports it. Cargo names the folder: the build folder can also hold older
# llama-cpp-sys-2 builds restored from the cache, whose modules must not ship.
# Fails when that folder has no non-empty backends/.
set -euo pipefail

if [ "$#" -lt 1 ]; then
  echo "usage: $0 <target-triple> [cargo build options]" >&2
  exit 2
fi
triple="$1"
shift

# On Windows cargo reports backslashes and jq may end its lines with CRLF.
out="$(cargo build --release --target "$triple" "$@" --message-format=json |
  jq -r 'select(.reason == "build-script-executed" and (.package_id | test("#llama-cpp-sys-2@"))) | .out_dir' |
  tr -d '\r' | tr '\\' '/')"

if [ -z "$out" ] || [ ! -d "$out/backends" ] || [ -z "$(find "$out/backends" -type f | head -n 1)" ]; then
  echo "error: no llama-cpp-sys-2 build with a backends/ folder for $triple: '$out'" >&2
  echo "       (build ncrs with the local-ai feature for $triple first)" >&2
  exit 1
fi

echo "$out"
