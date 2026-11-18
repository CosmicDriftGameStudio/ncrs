#!/bin/sh
# NC-rs installer.
#
#   curl -fsSL https://raw.githubusercontent.com/<owner>/ncrs/main/install.sh | sh
#
# Writes only inside $HOME and never needs sudo. Re-running replaces the files
# of the previous install. --uninstall removes the program files again and
# leaves settings and data alone.

set -eu

REPO="${NCRS_REPO:-CosmicDriftGameStudio/ncrs}"
BIN_DIR="${NCRS_BIN_DIR:-$HOME/.local/bin}"
BIN_NAME="ncrs"
# The PDF library sits next to the binary; ncrs looks for it there.
case "$(uname -s)" in
    Darwin) LIB_NAME="libpdfium.dylib" ;;
    *)      LIB_NAME="libpdfium.so" ;;
esac

info() { printf '  %s\n' "$*"; }
fail() { printf 'error: %s\n' "$*" >&2; exit 1; }

# Everything an install puts next to the binary is listed here, one path
# relative to BIN_DIR per line, so an uninstall removes those files and nothing
# else in a folder other programs share.
MANIFEST_NAME=".ncrs-install-manifest"

remove_listed() {
    # remove_listed <relative path>: only plain files, never outside BIN_DIR.
    case "$1" in
        ""|/*|*..*) return 0 ;;
    esac
    if [ -f "$BIN_DIR/$1" ]; then
        rm -f "$BIN_DIR/$1"
        info "removed $BIN_DIR/$1"
    fi
}

uninstall() {
    if [ -e "$BIN_DIR/$BIN_NAME" ]; then
        rm -f "$BIN_DIR/$BIN_NAME"
        info "removed $BIN_DIR/$BIN_NAME"
    else
        info "nothing to remove at $BIN_DIR/$BIN_NAME"
    fi
    if [ -e "$BIN_DIR/$LIB_NAME" ]; then
        rm -f "$BIN_DIR/$LIB_NAME"
        info "removed $BIN_DIR/$LIB_NAME"
    fi
    if [ -f "$BIN_DIR/$MANIFEST_NAME" ]; then
        while IFS= read -r listed; do
            remove_listed "$listed"
        done < "$BIN_DIR/$MANIFEST_NAME"
        # Folders only when they are empty now: anything else in them is not ours.
        if [ -d "$BIN_DIR/backends" ]; then
            find "$BIN_DIR/backends" -depth -type d -exec rmdir {} \; 2>/dev/null || true
        fi
        rm -f "$BIN_DIR/$MANIFEST_NAME"
    elif [ -d "$BIN_DIR/backends" ] || [ -e "$BIN_DIR/libllama.so.0" ]; then
        info "left $BIN_DIR/backends and the libllama and libggml files in place:"
        info "this install did not list them, and other programs may use the same names"
    fi
    info "settings, if any, are in ${XDG_CONFIG_HOME:-$HOME/.config}/ncrs"
    case "$(uname -s)" in
        Darwin) info "data (index, notes, models) is in $HOME/Library/Application Support/ncrs" ;;
        *)      info "data (index, notes, models) is in ${XDG_DATA_HOME:-$HOME/.local/share}/ncrs" ;;
    esac
    info "these are kept; delete them by hand to remove everything"
}

case "${1:-}" in
    --uninstall) uninstall; exit 0 ;;
    --help|-h)
        printf 'usage: install.sh [--uninstall] [--help]\n'
        printf '  installs ncrs into %s\n' "$BIN_DIR"
        exit 0
        ;;
    "") ;;
    *) fail "unknown option: $1 (try --help)" ;;
esac

# --- platform ---------------------------------------------------------------

os=$(uname -s)
arch=$(uname -m)

case "$os" in
    Linux)  platform=unknown-linux-gnu ;;
    Darwin) platform=apple-darwin ;;
    *) fail "unsupported OS: $os (Linux and macOS are supported; see install.ps1 for Windows)" ;;
esac

case "$arch" in
    x86_64|amd64)  cpu=x86_64 ;;
    arm64|aarch64) cpu=aarch64 ;;
    *) fail "unsupported architecture: $arch" ;;
esac

target="$cpu-$platform"

# --- tools ------------------------------------------------------------------

need_curl() {
    command -v curl >/dev/null 2>&1 || command -v wget >/dev/null 2>&1 \
        || fail "neither curl nor wget found"
}

fetch() {
    # fetch <url> <destination>
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$1" -o "$2"
    else
        wget -qO "$2" "$1"
    fi
}

need_curl

# The tag the latest release has right now. The checksum list and the archive
# are both fetched from that tag, so a release published in between cannot
# leave them from two different releases.
latest_tag() {
    latest_url="https://github.com/$REPO/releases/latest"
    if command -v curl >/dev/null 2>&1; then
        location=$(curl -fsS -o /dev/null -w '%{redirect_url}' "$latest_url") || return 1
    else
        location=$(wget -S --max-redirect=0 -O /dev/null "$latest_url" 2>&1 \
            | sed -n 's/^ *[Ll]ocation: *//p' | head -n 1 | tr -d '\r')
    fi
    printf '%s\n' "${location##*/tag/}"
}

tmp=$(mktemp -d 2>/dev/null || mktemp -d -t ncrs)
trap 'rm -rf "$tmp"' EXIT INT TERM

printf 'ncrs installer\n'
info "platform   $target"
info "install to $BIN_DIR"
info ""

# --- checksum ---------------------------------------------------------------

# Verify against the published SHA256SUMS. Required, not optional: a release
# workflow that fails halfway can leave the file behind while the binary is
# stale, or vice versa, and a checksum that is skipped when it is inconvenient
# is a checksum that verifies nothing. The two failure modes stay separate so
# the message says which one it was — a missing SHA256SUMS means the release
# was built wrong, a missing entry for this archive means the release is
# incomplete.
tag=$(latest_tag) || tag=""
case "$tag" in
    v[0-9]*) ;;
    *) fail "could not find the latest release of $REPO" ;;
esac
info "release    $tag"
download_base="https://github.com/$REPO/releases/download/$tag"

if ! fetch "$download_base/SHA256SUMS" "$tmp/SHA256SUMS" 2>/dev/null; then
    fail "no SHA256SUMS published for $tag
       (expected at $download_base/SHA256SUMS)
       Refusing to install an unverified binary."
fi

# --- download ---------------------------------------------------------------

archive="ncrs-$target.tar.gz"
url="$download_base/$archive"

if ! fetch "$url" "$tmp/$archive" 2>/dev/null; then
    fail "no release found for $target at $url
       (a release exists but not for this platform, or none is published yet)"
fi

expected=$(grep " $archive\$" "$tmp/SHA256SUMS" | awk '{print $1}' | head -n 1)
[ -n "$expected" ] || fail "SHA256SUMS has no entry for $archive
       The release is incomplete. Refusing to install an unverified binary."
if command -v sha256sum >/dev/null 2>&1; then
    actual=$(sha256sum "$tmp/$archive" | awk '{print $1}')
elif command -v shasum >/dev/null 2>&1; then
    actual=$(shasum -a 256 "$tmp/$archive" | awk '{print $1}')
else
    fail "no sha256 tool found (neither sha256sum nor shasum is installed)
       Cannot verify $archive, so refusing to install it.
       Install coreutils, or fetch and verify the archive by hand:
         $url"
fi
[ -n "$actual" ] || fail "could not compute the checksum of $archive
       Refusing to install an unverified binary."
[ "$actual" = "$expected" ] || fail "checksum mismatch for $archive
  expected $expected
  actual   $actual"
info "checksum ok"

# --- install ----------------------------------------------------------------

tar -xzf "$tmp/$archive" -C "$tmp" || fail "could not unpack $archive"

# Find the binary wherever the archive put it, rather than assuming a layout.
src=$(find "$tmp" -type f -name "$BIN_NAME" | head -n 1)
[ -n "$src" ] || fail "archive did not contain a file called $BIN_NAME"

lib_src=$(find "$tmp" -type f -name "$LIB_NAME" | head -n 1)
[ -n "$lib_src" ] || fail "archive did not contain a file called $LIB_NAME"

mkdir -p "$BIN_DIR"
# Install via a temp name so a running instance is not clobbered mid-execution.
cp "$src" "$BIN_DIR/.$BIN_NAME.new"
chmod +x "$BIN_DIR/.$BIN_NAME.new"
mv "$BIN_DIR/.$BIN_NAME.new" "$BIN_DIR/$BIN_NAME"

cp "$lib_src" "$BIN_DIR/.$LIB_NAME.new"
mv "$BIN_DIR/.$LIB_NAME.new" "$BIN_DIR/$LIB_NAME"

installed_what="$BIN_DIR/$BIN_NAME and $BIN_DIR/$LIB_NAME"

# The local AI model: llama.cpp libraries next to the binary and its backend
# modules in backends/. Archives without backends/ (macOS, aarch64) link it in.
backends_src="$(dirname "$src")/backends"
if [ -d "$backends_src" ]; then
    for llama_lib in libllama.so.0 libggml.so.0 libggml-base.so.0; do
        [ -f "$(dirname "$src")/$llama_lib" ] \
            || fail "archive has a backends folder but no $llama_lib"
        cp "$(dirname "$src")/$llama_lib" "$BIN_DIR/.$llama_lib.new"
        mv "$BIN_DIR/.$llama_lib.new" "$BIN_DIR/$llama_lib"
    done
    rm -rf "$BIN_DIR/.backends.new"
    cp -R "$backends_src" "$BIN_DIR/.backends.new"
    rm -rf "$BIN_DIR/backends"
    mv "$BIN_DIR/.backends.new" "$BIN_DIR/backends"
    installed_what="$installed_what, the llama.cpp libraries and $BIN_DIR/backends"
fi

# What this install put next to the binary, for --uninstall.
{
    printf '%s\n' "$LIB_NAME"
    if [ -d "$backends_src" ]; then
        printf '%s\n' libllama.so.0 libggml.so.0 libggml-base.so.0
        (cd "$BIN_DIR" && find backends -type f)
    fi
} > "$BIN_DIR/$MANIFEST_NAME"

info "installed $installed_what"

# --- PATH hint --------------------------------------------------------------

case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *)
        info ""
        info "$BIN_DIR is not in your PATH. Add this to your shell profile:"
        info "    export PATH=\"\$HOME/.local/bin:\$PATH\""
        ;;
esac

info ""
info "run it with:  $BIN_NAME"
