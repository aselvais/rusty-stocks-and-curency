#!/usr/bin/env bash
#
# release.sh — cross-platform release orchestrator.
#
# Dispatches to the platform-specific build/bundle script that matches the host
# operating system, then produces distributables in ./dist.
#
# Usage:
#   ./scripts/release.sh
#   RELEASE_VERSION=1.2.3 ./scripts/release.sh
#
# To build all three platforms from one machine, use the GitHub Actions workflow
# (.github/workflows/release.yml), which runs each build on a native runner —
# cross-compiling a GUI desktop binary to another OS is rarely practical.
#
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)"
os="$(uname -s)"

case "$os" in
  Darwin)
    exec "$here/release-macos.sh" "$@"
    ;;
  Linux)
    exec "$here/release-linux.sh" "$@"
    ;;
  MINGW*|MSYS*|CYGWIN*)
    # Native Windows shell (Git Bash / MSYS2). Prefer PowerShell when present.
    if command -v powershell.exe >/dev/null 2>&1; then
      exec powershell.exe -ExecutionPolicy Bypass -File "$here/release-windows.ps1" "$@"
    fi
    exec "$here/release-windows.sh" "$@"
    ;;
  *)
    echo "Unsupported platform: $os" >&2
    exit 1
    ;;
esac
