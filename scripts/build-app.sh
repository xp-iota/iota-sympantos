#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DESKTOP_DIR="$ROOT_DIR/crates/iota-desktop"

if ! command -v npm >/dev/null 2>&1; then
  echo "npm is required to build the desktop app" >&2
  exit 1
fi

if [[ ! -d "$DESKTOP_DIR/node_modules" ]]; then
  (cd "$DESKTOP_DIR" && npm ci)
fi

(cd "$DESKTOP_DIR" && npm run tauri -- build "$@")
echo "desktop bundles: $ROOT_DIR/target/release/bundle"
