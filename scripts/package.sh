#!/usr/bin/env bash
# Lays out a release: the archives of the build job and the unsigned macOS app.
# One place, so the PR build, the release build and the signing step cannot
# drift apart. Runs on macOS, Linux and Windows (git-bash).
#
#   package.sh archive <triple> <binary> <pdfium-dir> <out-dir> [--licenses <file>]
#   package.sh app <dir-with-binary-and-lib> <version> <min-macos> <out-dir> [--licenses <file>]
#
# <pdfium-dir> is what scripts/fetch-pdfium.sh unpacked. Both commands end
# with a layout check that lists the result and fails when the binary or the
# library is missing.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

usage() {
  sed -n '2,/^set -euo/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//' >&2
  exit 2
}

# Absolute path of an existing file or directory.
absolute() {
  if [ -d "$1" ]; then
    (cd "$1" && pwd)
  else
    echo "$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
  fi
}

# Prints "<library file name> <subfolder of the fetch-pdfium output>".
library_for() {
  case "$1" in
    *-apple-darwin) echo "libpdfium.dylib lib" ;;
    *-windows-*) echo "pdfium.dll bin" ;;
    *-linux-*) echo "libpdfium.so lib" ;;
    *) echo "no pdfium library known for target: $1" >&2; return 1 ;;
  esac
}

# Fails unless every given path exists below <root>; lists them with sizes.
check_layout() {
  local root="$1"
  shift
  echo "layout of $root:"
  (cd "$root" && find . -type f -o -type l | sort | while read -r f; do ls -l "$f"; done)
  local missing=0 path
  for path in "$@"; do
    if [ ! -f "$root/$path" ]; then
      echo "error: missing in $root: $path" >&2
      missing=1
    fi
  done
  return "$missing"
}

# Zips <dir-name> inside <cwd> into <zip-name>, with the folder as top level.
make_zip() {
  local cwd="$1" dir_name="$2" zip_name="$3"
  if command -v pwsh >/dev/null 2>&1; then
    (cd "$cwd" && pwsh -NoProfile -Command "Compress-Archive -Path '$dir_name' -DestinationPath '$zip_name'")
  elif command -v powershell >/dev/null 2>&1; then
    (cd "$cwd" && powershell -NoProfile -Command "Compress-Archive -Path '$dir_name' -DestinationPath '$zip_name'")
  else
    (cd "$cwd" && 7z a -tzip "$zip_name" "$dir_name" >/dev/null)
  fi
}

list_zip() {
  if command -v unzip >/dev/null 2>&1; then
    unzip -l "$1"
  elif command -v 7z >/dev/null 2>&1; then
    7z l "$1"
  else
    echo "(no unzip or 7z here; the staged folder above is what was zipped)"
  fi
}

parse_licenses_option() {
  licenses=""
  if [ "$#" -eq 0 ]; then
    return 0
  fi
  if [ "$#" -ne 2 ] || [ "$1" != "--licenses" ]; then
    usage
  fi
  licenses="$(absolute "$2")"
}

package_archive() {
  [ "$#" -ge 4 ] || usage
  local triple="$1" binary pdfium_dir out_dir
  binary="$(absolute "$2")"
  pdfium_dir="$(absolute "$3")"
  mkdir -p "$4"
  out_dir="$(absolute "$4")"
  shift 4
  parse_licenses_option "$@"

  local library_name library_subdir
  read -r library_name library_subdir <<<"$(library_for "$triple")"
  local library="$pdfium_dir/$library_subdir/$library_name"
  [ -f "$library" ] || { echo "error: no $library_name in $pdfium_dir/$library_subdir" >&2; exit 1; }

  local name="ncrs-$triple" binary_name=ncrs
  case "$triple" in *-windows-*) binary_name=ncrs.exe ;; esac

  local stage="$out_dir/$name"
  rm -rf "$stage"
  mkdir -p "$stage"
  cp "$binary" "$stage/$binary_name"
  cp "$library" "$stage/$library_name"
  cp "$repo_root/LICENSE" "$repo_root/README.md" "$repo_root/ROADMAP.md" "$stage/"
  # The notices of pdfium and of the components built into it (FreeType, ICU,
  # zlib, ...) must ship with the library.
  [ -f "$pdfium_dir/LICENSE" ] && [ -d "$pdfium_dir/licenses" ] \
    || { echo "error: no LICENSE and licenses/ in $pdfium_dir" >&2; exit 1; }
  mkdir -p "$stage/licenses/pdfium"
  cp "$pdfium_dir/LICENSE" "$stage/licenses/pdfium/"
  cp -R "$pdfium_dir/licenses/." "$stage/licenses/pdfium/"
  local expected=("$binary_name" "$library_name" LICENSE README.md ROADMAP.md licenses/pdfium/LICENSE)
  if [ -n "$licenses" ]; then
    cp "$licenses" "$stage/THIRD_PARTY_LICENSES.html"
    expected+=(THIRD_PARTY_LICENSES.html)
  fi

  case "$triple" in
    *-windows-*)
      make_zip "$out_dir" "$name" "$name.zip"
      # Portable: a data folder next to the executable switches ncrs to
      # keeping its config and index there.
      local portable="$out_dir/$name-portable"
      rm -rf "$portable"
      cp -R "$stage" "$portable"
      mkdir -p "$portable/data"
      cp "$repo_root/packaging/windows/data/README.txt" "$portable/data/"
      make_zip "$out_dir" "$name-portable" "$name-portable.zip"
      rm -rf "$portable"
      check_layout "$stage" "${expected[@]}"
      echo "archive $name.zip:"
      list_zip "$out_dir/$name.zip"
      echo "archive $name-portable.zip:"
      list_zip "$out_dir/$name-portable.zip"
      ;;
    *)
      tar -czf "$out_dir/$name.tar.gz" -C "$out_dir" "$name"
      check_layout "$stage" "${expected[@]}"
      echo "archive $name.tar.gz:"
      local listing
      listing="$(tar -tzf "$out_dir/$name.tar.gz")"
      echo "$listing"
      local path
      for path in "${expected[@]}"; do
        echo "$listing" | grep -qx "$name/$path" \
          || { echo "error: $name.tar.gz does not contain $path" >&2; exit 1; }
      done
      ;;
  esac
}

package_app() {
  [ "$#" -ge 4 ] || usage
  local source_dir version min_macos out_dir
  source_dir="$(absolute "$1")"
  version="$2"
  min_macos="$3"
  mkdir -p "$4"
  out_dir="$(absolute "$4")"
  shift 4
  parse_licenses_option "$@"

  [ -f "$source_dir/ncrs" ] || { echo "error: no ncrs binary in $source_dir" >&2; exit 1; }
  [ -f "$source_dir/libpdfium.dylib" ] || { echo "error: no libpdfium.dylib in $source_dir" >&2; exit 1; }

  [ -f "$source_dir/licenses/pdfium/LICENSE" ] \
    || { echo "error: no licenses/pdfium/LICENSE in $source_dir" >&2; exit 1; }

  local app="$out_dir/ncrs.app"
  rm -rf "$app"
  mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" "$app/Contents/Frameworks"
  cp "$source_dir/ncrs" "$app/Contents/MacOS/ncrs"
  # Frameworks, where the app looks for the library when it is not next to the
  # executable; signed on its own before the bundle.
  cp "$source_dir/libpdfium.dylib" "$app/Contents/Frameworks/libpdfium.dylib"
  cp "$repo_root/assets/ncrs.icns" "$app/Contents/Resources/ncrs.icns"
  # Inside Resources so the notices are covered by the bundle signature.
  mkdir -p "$app/Contents/Resources/licenses"
  cp -R "$source_dir/licenses/pdfium" "$app/Contents/Resources/licenses/pdfium"
  local expected=(Contents/MacOS/ncrs Contents/Frameworks/libpdfium.dylib Contents/Resources/ncrs.icns Contents/Info.plist Contents/Resources/licenses/pdfium/LICENSE)
  if [ -n "$licenses" ]; then
    # Inside Resources so it is covered by the bundle signature.
    cp "$licenses" "$app/Contents/Resources/THIRD_PARTY_LICENSES.html"
    expected+=(Contents/Resources/THIRD_PARTY_LICENSES.html)
  fi
  cp "$repo_root/packaging/macos/Info.plist" "$app/Contents/Info.plist"
  plutil -replace CFBundleShortVersionString -string "$version" "$app/Contents/Info.plist"
  plutil -replace CFBundleVersion -string "$version" "$app/Contents/Info.plist"
  plutil -replace LSMinimumSystemVersion -string "$min_macos" "$app/Contents/Info.plist"

  check_layout "$app" "${expected[@]}"
}

[ "$#" -ge 1 ] || usage
command="$1"
shift
case "$command" in
  archive) package_archive "$@" ;;
  app) package_app "$@" ;;
  *) usage ;;
esac
