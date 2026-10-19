#!/usr/bin/env bash
#
# Build and bundle a macOS release of Stock & Currency Tracker.
#
# Produces (in ./dist):
#   Stock & Currency Tracker-macOS-<version>.app.zip   (portable .app)
#   Stock & Currency Tracker-macOS-<version>.dmg        (installer image)
#
# Optional environment:
#   RELEASE_VERSION   Version string for artifact names (default: Cargo version)
#   SIGN_IDENTITY     Keychain identity to code-sign with, e.g.
#                    "Developer ID Application: Name (TEAMID)"
#   NOTARIZE=1        Also notarize + staple the .app/.dmg (needs APPLE_ID,
#                    APPLE_PASSWORD, APPLE_TEAM_ID)
#
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")/.." && pwd)"
cd "$ROOT"

VERSION="${RELEASE_VERSION:-$(cargo pkgid | cut -d@ -f2)}"
PRODUCT="Stock & Currency Tracker"
BUNDLE_DIR="target/release/bundle"
DIST="dist"

log() { printf '\033[1;34m==>\033[0m %s\n' "$*"; }

log "Building release binary (macOS / v$VERSION)"
cargo build --release

log "Ensuring cargo-bundle is installed"
if ! command -v cargo-bundle >/dev/null 2>&1; then
  cargo install cargo-bundle
fi

log "Bundling .app and .dmg"
rm -rf "$BUNDLE_DIR"
cargo bundle --format osx
cargo bundle --format dmg

find_dmg() { ls "$BUNDLE_DIR/dmg"/*.dmg 2>/dev/null | head -n1 || true; }
find_app()  { ls -d "$BUNDLE_DIR/osx"/*.app 2>/dev/null | head -n1 || true; }

if [ -n "${SIGN_IDENTITY:-}" ]; then
  log "Code signing with identity: $SIGN_IDENTITY"
  APP="$(find_app)"
  [ -n "$APP" ] && codesign --force --deep --options runtime --sign "$SIGN_IDENTITY" "$APP"
  log "Rebuilding signed .dmg"
  rm -rf "$BUNDLE_DIR/dmg"
  cargo bundle --format dmg
fi

if [ "${NOTARIZE:-0}" = "1" ]; then
  if [ -z "${APPLE_ID:-}" ] || [ -z "${APPLE_PASSWORD:-}" ] || [ -z "${APPLE_TEAM_ID:-}" ]; then
    echo "NOTARIZE=1 but APPLE_ID / APPLE_PASSWORD / APPLE_TEAM_ID are not set." >&2
    exit 1
  fi
  log "Notarizing and stapling"
  APP="$(find_app)"
  notarytool submit "$APP" \
    --apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID" \
    --wait --output-format json || true
  DMG="$(find_dmg)"
  [ -n "$DMG" ] && xcrun stapler staple "$DMG"
fi

log "Packaging into $DIST"
mkdir -p "$DIST"

APP="$(find_app)"
if [ -n "$APP" ]; then
  ZIP="$DIST/${PRODUCT}-macOS-${VERSION}.zip"
  rm -f "$ZIP"
  ditto -c -k --keepParent "$APP" "$ZIP"
fi

DMG="$(find_dmg)"
if [ -n "$DMG" ]; then
  cp "$DMG" "$DIST/${PRODUCT}-macOS-${VERSION}.dmg"
fi

log "Artifacts:"
ls -lh "$DIST"
