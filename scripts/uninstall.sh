#!/usr/bin/env bash
set -euo pipefail

BIN_DIR="${IOTA_INSTALL_DIR:-$HOME/.local/bin}"
APP_DIR="${IOTA_APP_DIR:-$HOME/Applications}"

rm -f "$BIN_DIR/iota" "$BIN_DIR/iota-desktop.AppImage"
rm -rf "$APP_DIR/iota-desktop.app"
echo "removed iota CLI and user-local desktop artifacts"
echo "configuration and data under $HOME/.i6 were kept"
