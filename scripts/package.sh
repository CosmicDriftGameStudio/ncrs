#!/usr/bin/env bash
# Lays out a release: the archives of the build job, the unsigned macOS app and
# the Linux .deb and .rpm.
# One place, so the PR build, the release build and the signing step cannot
# drift apart. Runs on macOS, Linux and Windows (git-bash).
#
#   package.sh archive <triple> <binary> <pdfium-dir> <out-dir> [--licenses <file>]
#   package.sh app <dir-with-binary-and-lib> <version> <min-macos> <out-dir> [--licenses <file>]
#   package.sh sandbox-app <dir-with-binary-and-lib> <sandbox-binary> <version> <min-macos> <out-dir> [--licenses <file>]
#   package.sh check-sandbox <app> sandboxed|plain
#   package.sh store-app <dir-with-binary-and-lib> <store-binary> <version> <profile> <out-dir> [--licenses <file>]
#   package.sh check-store <app> <profile> <arch>...
#   package.sh linux-packages <triple> <stage-dir> <version> <out-dir> <nfpm-binary>
#
# sandbox-app is app with the binary swapped for <sandbox-binary> (the build
# with the sandbox feature) and the sandbox bundle ID. <out-dir> must differ
# from the one of the normal app, both write <out-dir>/ncrs.app.
# check-sandbox reads the entitlements of a signed app and fails unless the App
# Sandbox is on (sandboxed) or not on (plain).
# store-app is app for the Mac App Store: <store-binary> (the sandbox build,
# universal), the store bundle ID, <profile> embedded as
# Contents/embedded.provisionprofile and <out-dir>/store.entitlements (the
# sandbox ones plus application and team identifier) to sign the bundle with.
# check-store verifies the signed result: bundle ID, entitlements, the embedded
# profile equal to <profile> and every <arch> in the executable and the library.
#
# <pdfium-dir> is what scripts/fetch-pdfium.sh unpacked. archive and app end
# with a layout check that lists the result and fails when the binary or the
# library is missing.
#
# <stage-dir> is the folder `archive --licenses` leaves behind
# (<out-dir>/ncrs-<triple>), <nfpm-binary> what scripts/fetch-nfpm.sh unpacked.
# linux-packages writes <out-dir>/ncrs-<triple>.deb and .rpm.
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

ICON_SIZES="32 48 64 128 256 512"

# Own bundle ID, so the sandboxed app gets its own container, does not take over
# the Launch Services entry of the Developer ID app and leaves the ID of the
# later App Store app untouched.
SANDBOX_BUNDLE_ID=dev.cosmicdrift.ncrs.sandbox

STORE_BUNDLE_ID=dev.cosmicdrift.ncrs
STORE_TEAM_ID=2ZJ4C4D9SP
# The store app is universal: 10.15 is the floor of the x86_64 slice, the
# arm64 slice carries 11.0 in its own load command (the Xcode convention).
STORE_MIN_MACOS=10.15

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
  case "$triple" in
    *-linux-*)
      # Laid out like /usr/share, so the .deb and .rpm take the tree as it is
      # and a package built from the tarball (the AUR one) installs it with a
      # plain copy.
      mkdir -p "$stage/share/applications" "$stage/share/icons"
      cp "$repo_root/packaging/linux/ncrs.desktop" "$stage/share/applications/"
      cp -R "$repo_root/packaging/linux/icons/hicolor" "$stage/share/icons/"
      expected+=(share/applications/ncrs.desktop)
      local icon_size
      for icon_size in $ICON_SIZES; do
        expected+=("share/icons/hicolor/${icon_size}x${icon_size}/apps/ncrs.png")
      done
      ;;
  esac
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

# package_app with the binary swapped for <binary> and the given bundle ID.
package_app_with_binary() {
  [ "$#" -ge 6 ] || usage
  local bundle_id="$1" source_dir="$2" swapped_binary="$3"
  shift 3
  [ -f "$swapped_binary" ] || { echo "error: no binary at $swapped_binary" >&2; exit 1; }
  local out_dir="$3"

  local stage_root
  stage_root="$(mktemp -d)"
  mkdir -p "$stage_root/ncrs-swapped"
  cp -R "$source_dir/." "$stage_root/ncrs-swapped"
  cp "$swapped_binary" "$stage_root/ncrs-swapped/ncrs"
  # upload-artifact drops the executable bit.
  chmod +x "$stage_root/ncrs-swapped/ncrs"

  package_app "$stage_root/ncrs-swapped" "$@"
  plutil -replace CFBundleIdentifier -string "$bundle_id" "$(absolute "$out_dir")/ncrs.app/Contents/Info.plist"
  rm -rf "$stage_root"
}

package_sandbox_app() {
  [ "$#" -ge 5 ] || usage
  package_app_with_binary "$SANDBOX_BUNDLE_ID" "$@"
}

package_store_app() {
  [ "$#" -ge 5 ] || usage
  local source_dir="$1" store_binary="$2" version="$3" profile="$4" out_dir="$5"
  shift 5
  [ -f "$profile" ] || { echo "error: no provisioning profile at $profile" >&2; exit 1; }
  package_app_with_binary "$STORE_BUNDLE_ID" "$source_dir" "$store_binary" "$version" "$STORE_MIN_MACOS" "$out_dir" "$@"
  out_dir="$(absolute "$out_dir")"
  # Before any signing, so the seal covers it.
  cp "$profile" "$out_dir/ncrs.app/Contents/embedded.provisionprofile"
  # The secret it came from is decoded owner-only, and cp keeps that mode.
  chmod 0644 "$out_dir/ncrs.app/Contents/embedded.provisionprofile"
  cp "$repo_root/packaging/macos/sandbox.entitlements" "$out_dir/store.entitlements"
  # PlistBuddy, not plutil: plutil splits keys on dots.
  /usr/libexec/PlistBuddy \
    -c "Add :com.apple.application-identifier string $STORE_TEAM_ID.$STORE_BUNDLE_ID" \
    -c "Add :com.apple.developer.team-identifier string $STORE_TEAM_ID" \
    "$out_dir/store.entitlements"
  echo "wrote $out_dir/store.entitlements and the embedded profile"
}

# Fails unless the signed app has (sandboxed) or lacks (plain) the App Sandbox
# entitlement.
check_sandbox() {
  [ "$#" -eq 2 ] || usage
  local app="$1" expected="$2" entitlements
  case "$expected" in
    sandboxed | plain) ;;
    *) usage ;;
  esac
  # A bundle without a seal still lists its linker-signed executable, so the
  # entitlements query alone does not tell signed from unsigned.
  codesign --verify "$app" 2>/dev/null \
    || { echo "error: $app is not signed" >&2; exit 1; }
  entitlements="$(codesign -d --entitlements - --xml "$app" 2>/dev/null)" \
    || { echo "error: cannot read the entitlements of $app" >&2; exit 1; }
  entitlements="$(printf '%s' "$entitlements" | tr -d '[:space:]')"
  local sandbox_entitlement='<key>com.apple.security.app-sandbox</key><true/>'
  local actual=plain
  case "$entitlements" in
    *"$sandbox_entitlement"*) actual=sandboxed ;;
  esac
  if [ "$actual" != "$expected" ]; then
    echo "error: $app is $actual, expected $expected" >&2
    exit 1
  fi
  echo "$app is $actual, as expected"
}

# Fails unless the signed app is the App Store app: store bundle ID, sandbox,
# application and team identifier in the entitlements, the given profile
# embedded and both slices in the executable and the library.
check_store() {
  [ "$#" -ge 3 ] || usage
  local app="$1" profile="$2" bundle_id entitlements file arch
  shift 2
  codesign --verify --deep --strict "$app" \
    || { echo "error: $app does not pass codesign --verify --deep --strict" >&2; exit 1; }
  bundle_id="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app/Contents/Info.plist")"
  [ "$bundle_id" = "$STORE_BUNDLE_ID" ] \
    || { echo "error: bundle ID is $bundle_id, expected $STORE_BUNDLE_ID" >&2; exit 1; }
  check_sandbox "$app" sandboxed
  entitlements="$(codesign -d --entitlements - --xml "$app" 2>/dev/null | tr -d '[:space:]')"
  local expected_entitlement
  for expected_entitlement in \
    "<key>com.apple.application-identifier</key><string>$STORE_TEAM_ID.$STORE_BUNDLE_ID</string>" \
    "<key>com.apple.developer.team-identifier</key><string>$STORE_TEAM_ID</string>"; do
    case "$entitlements" in
      *"$expected_entitlement"*) ;;
      *) echo "error: entitlements of $app lack $expected_entitlement" >&2; exit 1 ;;
    esac
  done
  cmp "$app/Contents/embedded.provisionprofile" "$profile" \
    || { echo "error: embedded.provisionprofile differs from $profile" >&2; exit 1; }
  # The installer keeps these modes; an owner-only file is unreadable for the user.
  local unreadable
  unreadable="$(find "$app" ! -perm -o+r | head -n 1)"
  [ -z "$unreadable" ] || { echo "error: $unreadable is not world-readable" >&2; exit 1; }
  for file in "$app/Contents/MacOS/ncrs" "$app/Contents/Frameworks/libpdfium.dylib"; do
    for arch in "$@"; do
      lipo "$file" -verify_arch "$arch" \
        || { echo "error: $file lacks the $arch slice" >&2; exit 1; }
    done
  done
  echo "$app is the App Store app ($*), as expected"
}

# NEEDED sonames of an ELF file, one per line.
needed_sonames() {
  readelf -d "$1" | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p'
}

# Highest version of one symbol-version family (GLIBC_2.39 -> 2.39) that the
# given ELF files require, or nothing when they use none. `readelf -V` reads
# the file itself, so this works on a cross-built aarch64 binary too.
highest_symbol_version() {
  local family="$1"
  shift
  local file
  for file in "$@"; do
    readelf -V "$file"
  done | grep -o "Name: ${family}_[0-9][0-9.]*" | sed "s/^Name: ${family}_//" | sort -uV | tail -n 1 || true
}

package_linux() {
  [ "$#" -eq 5 ] || usage
  local triple="$1" stage version out_dir nfpm
  stage="$(absolute "$2")"
  version="$3"
  mkdir -p "$4"
  out_dir="$(absolute "$4")"
  nfpm="$(absolute "$5")"

  local arch
  case "$triple" in
    x86_64-unknown-linux-gnu) arch=amd64 ;;
    aarch64-unknown-linux-gnu) arch=arm64 ;;
    *) echo "error: no Linux packages for target: $triple" >&2; exit 2 ;;
  esac
  printf '%s' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+([.+~-][0-9A-Za-z.+~-]+)?$' \
    || { echo "error: not a usable package version: $version" >&2; exit 2; }

  local binary="$stage/ncrs" library="$stage/libpdfium.so" path
  for path in ncrs libpdfium.so LICENSE README.md THIRD_PARTY_LICENSES.html \
    licenses/pdfium/LICENSE share/applications/ncrs.desktop share/icons/hicolor; do
    [ -e "$stage/$path" ] || { echo "error: no $path in $stage" >&2; exit 1; }
  done

  # What the dynamic loader needs before main() runs. Anything outside this
  # list has to be declared as a dependency below on purpose, so a new
  # link-time library cannot slip into the package unnoticed.
  local allowed=" libc.so.6 libm.so.6 libgcc_s.so.1 libstdc++.so.6 libpthread.so.0 libdl.so.2 ld-linux-x86-64.so.2 ld-linux-aarch64.so.1 "
  local file soname unexpected=0
  for file in "$binary" "$library"; do
    for soname in $(needed_sonames "$file"); do
      case "$allowed" in
        *" $soname "*) ;;
        *)
          echo "error: $(basename "$file") links $soname, which linux-packages does not declare." >&2
          echo "       Add it to the allowlist in scripts/package.sh and to the dependencies in packaging/linux/nfpm.yaml." >&2
          unexpected=1
          ;;
      esac
    done
  done
  [ "$unexpected" -eq 0 ] || exit 1

  # The lowest glibc / libstdc++ / libgcc the binaries run on is whatever they
  # were linked against, so it is read from them, not written down.
  local glibc glibcxx cxxabi gcc
  glibc="$(highest_symbol_version GLIBC "$binary" "$library")"
  glibcxx="$(highest_symbol_version GLIBCXX "$binary" "$library")"
  cxxabi="$(highest_symbol_version CXXABI "$binary" "$library")"
  gcc="$(highest_symbol_version GCC "$binary" "$library")"
  [ -n "$glibc" ] || { echo "error: no GLIBC_ symbol versions found in $binary" >&2; exit 1; }
  echo "minimum versions: glibc ${glibc}, GLIBCXX ${glibcxx:-none}, CXXABI ${cxxabi:-none}, GCC ${gcc:-none}"

  # Capability names of the rpm. Soname capabilities instead of package names
  # because Fedora, RHEL and openSUSE name the packages differently.
  rpm_capability() {
    if [ -n "$2" ]; then echo "$1($2)(64bit)"; else echo "$1()(64bit)"; fi
  }
  # Debian's libstdc++6 and libgcc-s1 carry the gcc version (14.2.0), not the
  # GLIBCXX_ or GCC_ symbol version, so only libc6 can be versioned. A
  # libstdc++ too old for the binary comes with a glibc too old for it as well.
  export NCRS_STAGE="$stage" NCRS_VERSION="$version" NCRS_ARCH="$arch"
  export NCRS_DEB_LIBC="libc6 (>= ${glibc})"
  NCRS_RPM_LIBC="$(rpm_capability libc.so.6 "GLIBC_${glibc}")"
  NCRS_RPM_LIBSTDCXX="$(rpm_capability libstdc++.so.6 "${glibcxx:+GLIBCXX_$glibcxx}")"
  NCRS_RPM_CXXABI="$(rpm_capability libstdc++.so.6 "${cxxabi:+CXXABI_$cxxabi}")"
  NCRS_RPM_LIBGCC="$(rpm_capability libgcc_s.so.1 "${gcc:+GCC_$gcc}")"
  export NCRS_RPM_LIBC NCRS_RPM_LIBSTDCXX NCRS_RPM_CXXABI NCRS_RPM_LIBGCC

  local config="$repo_root/packaging/linux/nfpm.yaml" format built
  for format in deb rpm; do
    built="$out_dir/ncrs-$triple.$format"
    rm -f "$built"
    "$nfpm" package --config "$config" --packager "$format" --target "$built"
    [ -f "$built" ] || { echo "error: nfpm wrote no $built" >&2; exit 1; }
  done

  local expected=(
    usr/lib/ncrs/ncrs usr/lib/ncrs/libpdfium.so usr/bin/ncrs
    usr/share/applications/ncrs.desktop
    usr/share/doc/ncrs/LICENSE usr/share/doc/ncrs/README.md
    usr/share/doc/ncrs/THIRD_PARTY_LICENSES.html usr/share/doc/ncrs/licenses/pdfium/LICENSE
  )
  local icon_size
  for icon_size in $ICON_SIZES; do
    expected+=("usr/share/icons/hicolor/${icon_size}x${icon_size}/apps/ncrs.png")
  done

  # Package contents, one path per line without a leading "./" or "/".
  local listing
  if command -v dpkg-deb >/dev/null 2>&1; then
    listing="$(dpkg-deb -c "$out_dir/ncrs-$triple.deb" | awk '{print $6}' | sed 's|^\./||; s|^/||')"
    check_listing "ncrs-$triple.deb" "$listing" "${expected[@]}"
  else
    echo "(no dpkg-deb here; ncrs-$triple.deb is not listed)"
  fi
  if command -v rpm >/dev/null 2>&1; then
    listing="$(rpm -qlp "$out_dir/ncrs-$triple.rpm" | sed 's|^/||')"
    check_listing "ncrs-$triple.rpm" "$listing" "${expected[@]}"
  else
    echo "(no rpm here; ncrs-$triple.rpm is not listed)"
  fi
}

# Lists a package's files and fails when one of the expected paths is not in it.
check_listing() {
  local name="$1" listing="$2" missing=0 path
  shift 2
  echo "contents of $name:"
  echo "$listing"
  for path in "$@"; do
    if ! echo "$listing" | grep -qx "$path"; then
      echo "error: $name does not contain $path" >&2
      missing=1
    fi
  done
  return "$missing"
}

[ "$#" -ge 1 ] || usage
command="$1"
shift
case "$command" in
  archive) package_archive "$@" ;;
  app) package_app "$@" ;;
  sandbox-app) package_sandbox_app "$@" ;;
  check-sandbox) check_sandbox "$@" ;;
  store-app) package_store_app "$@" ;;
  check-store) check_store "$@" ;;
  linux-packages) package_linux "$@" ;;
  *) usage ;;
esac
