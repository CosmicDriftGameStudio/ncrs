#!/usr/bin/env bash
# Prints the folder with the llama.cpp runtime of a release build: the shared
# libraries (lib/ on Linux, bin/ on Windows) and the backends/ folder that
# llama-cpp-sys-2 leaves in its OUT_DIR
# (target/<triple>/release/build/llama-cpp-sys-2-<hash>/out). `package.sh
# archive --llama` takes it for Windows and x86_64 Linux.
#
# Usage: scripts/llama-runtime-dir.sh <target-triple>
#
# Fails when no build has a non-empty backends/, and when several have and they
# differ (an old build next to a new one would ship the wrong modules).
set -euo pipefail

if [ "$#" -ne 1 ]; then
  echo "usage: $0 <target-triple>" >&2
  exit 2
fi
triple="$1"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
build_root="${CARGO_TARGET_DIR:-$repo_root/target}/$triple/release/build"

# A checksum over the shared libraries (lib/*.so*, bin/*.dll) and everything in
# backends/; the rest of OUT_DIR is the CMake build tree.
digest_of() {
  local sum=sha256sum
  command -v sha256sum >/dev/null 2>&1 || sum="shasum -a 256"
  (cd "$1" && { find ./lib -name '*.so*' 2>/dev/null; find ./bin -name '*.dll' 2>/dev/null; find ./backends -type f; } | LC_ALL=C sort | while read -r file; do
    $sum "$file"
  done | $sum | cut -d' ' -f1)
}

candidates=()
if [ -d "$build_root" ]; then
  # Newest first.
  while IFS= read -r folder; do
    out="$folder/out"
    if [ -d "$out/backends" ] && [ -n "$(find "$out/backends" -type f | head -n 1)" ]; then
      candidates+=("$out")
    fi
  done < <(ls -dt "$build_root"/llama-cpp-sys-2-* 2>/dev/null || true)
fi

if [ "${#candidates[@]}" -eq 0 ]; then
  echo "error: no llama-cpp-sys-2 build with a backends/ folder below $build_root" >&2
  echo "       (build ncrs with the local-ai feature for $triple first)" >&2
  exit 1
fi

first_digest="$(digest_of "${candidates[0]}")"
for candidate in "${candidates[@]:1}"; do
  if [ "$(digest_of "$candidate")" != "$first_digest" ]; then
    echo "error: several llama-cpp-sys-2 builds with different contents:" >&2
    printf '  %s\n' "${candidates[@]}" >&2
    exit 1
  fi
done
echo "${candidates[0]}"
