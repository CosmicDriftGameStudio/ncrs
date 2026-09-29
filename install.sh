#!/bin/sh
# NC-rs installer.
#
#   curl -fsSL https://raw.githubusercontent.com/<owner>/ncrs/main/install.sh | sh
#
# Writes only inside $HOME. Never needs sudo, never deletes anything, safe to
# re-run. Install with --uninstall to remove it again.

set -eu

REPO="${NCRS_REPO:-CosmicDriftGameStudio/ncrs}"
BIN_DIR="${NCRS_BIN_DIR:-$HOME/.local/bin}"
BIN_NAME="ncrs"

info() { printf '  %s\n' "$*"; }
fail() { printf 'error: %s\n' "$*" >&2; exit 1; }

uninstall() {
    if [ -e "$BIN_DIR/$BIN_NAME" ]; then
        rm -f "$BIN_DIR/$BIN_NAME"
        info "removed $BIN_DIR/$BIN_NAME"
    else
        info "nothing to remove at $BIN_DIR/$BIN_NAME"
    fi
    info "config, if any, is in ${XDG_CONFIG_HOME:-$HOME/.config}/ncrs"
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

tmp=$(mktemp -d 2>/dev/null || mktemp -d -t ncrs)
trap 'rm -rf "$tmp"' EXIT INT TERM

printf 'ncrs installer\n'
info "platform   $target"
info "install to $BIN_DIR"
info ""

# --- checksum ---------------------------------------------------------------

# If a SHA256SUMS is published alongside the release, verify. A missing file is
# not fatal: releases may be produced without one.
sums_available=no
if fetch "https://github.com/$REPO/releases/latest/download/SHA256SUMS" "$tmp/SHA256SUMS" 2>/dev/null; then
    sums_available=yes
fi

# --- download ---------------------------------------------------------------

archive="ncrs-$target.tar.gz"
url="https://github.com/$REPO/releases/latest/download/$archive"

if ! fetch "$url" "$tmp/$archive" 2>/dev/null; then
    fail "no release found for $target at $url
       (this is expected until a v* tag exists)"
fi

if [ "$sums_available" = yes ]; then
    expected=$(grep " $archive\$" "$tmp/SHA256SUMS" | awk '{print $1}' | head -n 1)
    if [ -n "$expected" ]; then
        if command -v sha256sum >/dev/null 2>&1; then
            actual=$(sha256sum "$tmp/$archive" | awk '{print $1}')
        elif command -v shasum >/dev/null 2>&1; then
            actual=$(shasum -a 256 "$tmp/$archive" | awk '{print $1}')
        else
            actual=""
            info "no sha256 tool found, skipping checksum"
        fi
        if [ -n "$actual" ]; then
            [ "$actual" = "$expected" ] || fail "checksum mismatch for $archive
  expected $expected
  actual   $actual"
            info "checksum ok"
        fi
    else
        info "no checksum for $archive, skipping"
    fi
else
    info "no SHA256SUMS published, skipping checksum"
fi

# --- install ----------------------------------------------------------------

tar -xzf "$tmp/$archive" -C "$tmp" || fail "could not unpack $archive"

# Find the binary wherever the archive put it, rather than assuming a layout.
src=$(find "$tmp" -type f -name "$BIN_NAME" | head -n 1)
[ -n "$src" ] || fail "archive did not contain a file called $BIN_NAME"

mkdir -p "$BIN_DIR"
# Install via a temp name so a running instance is not clobbered mid-execution.
cp "$src" "$BIN_DIR/.$BIN_NAME.new"
chmod +x "$BIN_DIR/.$BIN_NAME.new"
mv "$BIN_DIR/.$BIN_NAME.new" "$BIN_DIR/$BIN_NAME"

info "installed $BIN_DIR/$BIN_NAME"

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
