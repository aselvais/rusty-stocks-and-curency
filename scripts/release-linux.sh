#!/usr/bin/env bash
#
# Build and bundle a Linux release of Stock & Currency Tracker.
#
# Produces (in ./dist):
#   Stock & Currency Tracker-linux-<arch>-<version>.zip      (portable binary)
#   Stock & Currency Tracker-linux-x86_64-<version>.AppImage (if linuxdeploy present)
#
# Optional environment:
#   RELEASE_VERSION  Version string for artifact names (default: Cargo version)
#   SKIP_APPIMAGE=1  Skip AppImage creation even if linuxdeploy is available
#
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")/.." && pwd)"
cd "$ROOT"

VERSION="${RELEASE_VERSION:-$(cargo pkgid | cut -d@ -f2)}"
PRODUCT="Stock & Currency Tracker"
BIN="stock_currency_tracker"
BUNDLE_DIR="target/release/bundle"
DIST="dist"
ARCH="$(uname -m)"

log() { printf '\033[1;34m==>\033[0m %s\n' "$*"; }

log "Building release binary (Linux / $ARCH / v$VERSION)"
cargo build --release

# ----------------------------------------------------------------- portable zip
log "Packaging portable binary"
STAGE="target/release/portable"
rm -rf "$STAGE"
mkdir -p "$STAGE/bin"
cp "target/release/$BIN" "$STAGE/bin/"
mkdir -p "$DIST"

ZIP="$DIST/${PRODUCT}-linux-${ARCH}-${VERSION}.zip"
ZIP="$(cd "$(dirname "$ZIP")" && pwd)/$(basename "$ZIP")"
if command -v zip >/dev/null 2>&1; then
  rm -f "$ZIP"
  ( cd "$STAGE" && zip -r "$ZIP" . )
else
  TGZ="$DIST/${PRODUCT}-linux-${ARCH}-${VERSION}.tar.gz"
  log "zip not found, producing tarball instead"
  tar -czf "$TGZ" -C "$STAGE" .
fi

# --------------------------------------------------------------------- AppImage
if [ "${SKIP_APPIMAGE:-0}" != "1" ]; then
  if command -v linuxdeploy >/dev/null 2>&1 && [ "$ARCH" = "x86_64" ]; then
    log "Creating AppImage"
    if ! command -v cargo-bundle >/dev/null 2>&1; then
      cargo install cargo-bundle
    fi
    rm -rf "$BUNDLE_DIR"
    cargo bundle --format appimage || log "AppImage bundling failed; portable zip still produced."
    APPIMG="$(ls "$BUNDLE_DIR/appimage/"*.AppImage 2>/dev/null | head -n1 || true)"
    [ -n "$APPIMG" ] && cp "$APPIMG" "$DIST/${PRODUCT}-linux-x86_64-${VERSION}.AppImage"
  else
    log "Skipping AppImage (linuxdeploy missing or arch=$ARCH). Set SKIP_APPIMAGE=1 to silence."
  fi
fi

log "Artifacts:"
ls -lh "$DIST"
