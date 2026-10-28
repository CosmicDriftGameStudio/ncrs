#!/usr/bin/env bash
# Builds every icon file from assets/icon.svg: assets/icon-1024.png, the window
# icon assets/icon-128.rgba, the macOS assets/ncrs.icns and the Linux icons in
# packaging/linux/icons/hicolor. The results are committed, so only a change
# to the SVG needs this.
#
#   scripts/icon.sh
#
# macOS only: AppKit renders the SVG (assets/render.swift) and iconutil builds
# the .icns.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

swiftc -O "$repo_root/assets/render.swift" -o "$work/render"
# render.swift reads icon.svg from the current directory.
cd "$repo_root/assets"

"$work/render" png 1024 icon-1024.png
"$work/render" rgba 128 icon-128.rgba

iconset="$work/ncrs.iconset"
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
  "$work/render" png "$size" "$iconset/icon_${size}x${size}.png"
  "$work/render" png "$((size * 2))" "$iconset/icon_${size}x${size}@2x.png"
done
iconutil -c icns "$iconset" -o ncrs.icns

# The sizes scripts/package.sh checks for in the Linux archive and packages.
for size in 32 48 64 128 256 512; do
  linux_dir="$repo_root/packaging/linux/icons/hicolor/${size}x${size}/apps"
  mkdir -p "$linux_dir"
  "$work/render" png "$size" "$linux_dir/ncrs.png"
done
