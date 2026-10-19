#!/usr/bin/env bash
#
# Build a Windows release of Stock & Currency Tracker (MSVC target) and package a
# distributable .zip of the release binary. Run from Git Bash / MSYS2 / WSL on a
# machine that has Visual Studio 2022 ("Desktop development with C++") installed.
#
# For native PowerShell users, prefer release-windows.ps1.
#
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")/.." && pwd)"
cd "$ROOT"

VERSION="${RELEASE_VERSION:-$(cargo pkgid | cut -d@ -f2)}"
PRODUCT="Stock & Currency Tracker"
BIN="stock_currency_tracker"
TARGET="x86_64-pc-windows-msvc"
DIST="dist"

log() { printf '\033[1;34m==>\033[0m %s\n' "$*"; }

log "Ensuring target $TARGET is installed"
rustup target add "$TARGET"

log "Building release binary (Windows / v$VERSION)"
cargo build --release --target "$TARGET"

log "Packaging portable binary"
mkdir -p "$DIST"
STAGE="target/$TARGET/release/dist"
rm -rf "$STAGE"
mkdir -p "$STAGE"
cp "target/$TARGET/release/$BIN.exe" "$STAGE/"

ZIP="$DIST/${PRODUCT}-windows-x86_64-${VERSION}.zip"
if command -v zip >/dev/null 2>&1; then
  rm -f "$ZIP"
  ( cd "$STAGE" && zip -r "$ZIP" . )
elif command -v powershell.exe >/dev/null 2>&1; then
  ZIP_W="$(cygpath -w "$ZIP" 2>/dev/null || echo "$ZIP")"
  powershell.exe -NoProfile -Command "Compress-Archive -Path '$STAGE/*' -DestinationPath '$ZIP_W'"
else
  echo "Install 'zip' (apt/pacman) to produce a Windows archive." >&2
fi

log "Optional: build an MSI with cargo-bundle"
if command -v cargo-bundle >/dev/null 2>&1; then
  cargo bundle --format msi --target "$TARGET" || true
fi

log "Artifacts:"
ls -lh "$DIST"
