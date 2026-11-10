#!/usr/bin/env bash
# Renders the app pictures in the iced Simulator, then the stage (camera, titles, key caps,
# cross-fades) into target/trailer/out/: ncrs-trailer.mp4, web/en.mp4, web/en.jpg, social.mp4.
set -euo pipefail
cd "$(dirname "$0")/../.."

export PATH="${NCRS_TOOLCHAIN_BIN:-$HOME/.rustup/toolchains/1.98.1-aarch64-apple-darwin/bin}:$PATH"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cargo-target/ncrs}"
export NCRS_PDFIUM_DIR="${NCRS_PDFIUM_DIR:-$PWD/target/pdfium/lib}"

cargo test -p ncrs trailer_frames -- --ignored

cd scripts/trailer
[ -d node_modules ] || bun install
bun stage.ts

for video in ncrs-trailer.mp4 web/en.mp4 social.mp4; do
  printf '%s %ss\n' "$video" "$(ffprobe -v error -show_entries format=duration -of csv=p=0 "../../target/trailer/out/$video")"
done
