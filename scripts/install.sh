#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_DIR="${IOTA_INSTALL_DIR:-$HOME/.local/bin}"
APP_DIR="${IOTA_APP_DIR:-$HOME/Applications}"
CLI_SOURCE="${IOTA_CLI_SOURCE:-$ROOT_DIR/target/release/iota}"
APP_SOURCE="${IOTA_APP_SOURCE:-}"

usage() {
  cat <<'USAGE'
Usage: scripts/install.sh [--cli] [--app PATH] [--all]

Install the release CLI into ~/.local/bin and, on macOS, copy the .app bundle
into ~/Applications. On Linux, --app accepts an AppImage and installs it into
~/.local/bin. A .deb path is installed with dpkg.

Options:
  --cli          Install only the CLI.
  --app PATH     Install the specified desktop bundle.
  --all          Install the CLI and auto-detect a desktop bundle.
USAGE
}

INSTALL_CLI=0
INSTALL_APP=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --cli) INSTALL_CLI=1 ;;
    --app) INSTALL_APP=1; APP_SOURCE="${2:?--app requires a path}"; shift ;;
    --all) INSTALL_CLI=1; INSTALL_APP=1 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown option: $1" >&2; usage >&2; exit 2 ;;
  esac
  shift
done

if [[ "$INSTALL_CLI" -eq 0 && "$INSTALL_APP" -eq 0 ]]; then
  INSTALL_CLI=1
fi

if [[ "$INSTALL_CLI" -eq 1 ]]; then
  [[ -f "$CLI_SOURCE" ]] || { echo "CLI binary not found: $CLI_SOURCE; run scripts/build-cli.sh" >&2; exit 1; }
  mkdir -p "$BIN_DIR"
  install -m 755 "$CLI_SOURCE" "$BIN_DIR/iota"
  echo "installed CLI: $BIN_DIR/iota"
fi

if [[ "$INSTALL_APP" -eq 1 ]]; then
  if [[ -z "$APP_SOURCE" ]]; then
    if [[ "$(uname -s)" == "Darwin" ]]; then
      APP_SOURCE="$ROOT_DIR/target/release/bundle/macos/iota-desktop.app"
    else
      APP_SOURCE="$(find "$ROOT_DIR/target/release/bundle/appimage" -maxdepth 1 -type f -name '*.AppImage' -print -quit 2>/dev/null || true)"
    fi
  fi
  [[ -n "$APP_SOURCE" && -e "$APP_SOURCE" ]] || { echo "desktop bundle not found; pass --app PATH or run scripts/build-app.sh" >&2; exit 1; }
  if [[ "$APP_SOURCE" == *.deb ]]; then
    sudo dpkg -i "$APP_SOURCE"
  elif [[ "$(uname -s)" == "Darwin" && "$APP_SOURCE" == *.app ]]; then
    mkdir -p "$APP_DIR"
    rm -rf "$APP_DIR/iota-desktop.app"
    ditto "$APP_SOURCE" "$APP_DIR/iota-desktop.app"
    echo "installed desktop app: $APP_DIR/iota-desktop.app"
  else
    mkdir -p "$BIN_DIR"
    install -m 755 "$APP_SOURCE" "$BIN_DIR/iota-desktop.AppImage"
    echo "installed desktop AppImage: $BIN_DIR/iota-desktop.AppImage"
  fi
fi
