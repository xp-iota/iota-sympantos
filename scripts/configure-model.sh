#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONFIG_DIR="${IOTA_CONFIG_DIR:-$HOME/.i6}"
CONFIG_PATH="$CONFIG_DIR/nimia.yaml"
INIT=0
OPEN=0

usage() {
  cat <<'USAGE'
Usage: scripts/configure-model.sh [--init] [--open]

Initialize or open the single iota model configuration file:
  ~/.i6/nimia.yaml

Options:
  --init   Create the parent directory and copy nimia.yaml.template if absent.
  --open   Open the file with $VISUAL, $EDITOR, or the platform default editor.
  -h       Show this help.
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --init) INIT=1 ;;
    --open) OPEN=1 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown option: $1" >&2; usage >&2; exit 2 ;;
  esac
  shift
done

if [[ ! -f "$CONFIG_PATH" ]]; then
  INIT=1
fi

if [[ "$INIT" -eq 1 ]]; then
  mkdir -p "$CONFIG_DIR"
  if [[ ! -f "$CONFIG_PATH" ]]; then
    install -m 600 "$ROOT_DIR/nimia.yaml.template" "$CONFIG_PATH"
    echo "created $CONFIG_PATH"
  else
    echo "kept existing $CONFIG_PATH"
  fi
fi

open_config() {
  if [[ -n "${VISUAL:-}" ]]; then
    "$VISUAL" "$CONFIG_PATH"
  elif [[ -n "${EDITOR:-}" ]]; then
    "$EDITOR" "$CONFIG_PATH"
  elif [[ "$(uname -s)" == "Darwin" ]] && command -v open >/dev/null 2>&1; then
    open -t "$CONFIG_PATH"
  elif command -v xdg-open >/dev/null 2>&1; then
    xdg-open "$CONFIG_PATH"
  elif command -v sensible-editor >/dev/null 2>&1; then
    sensible-editor "$CONFIG_PATH"
  else
    echo "no editor found; set VISUAL or EDITOR and rerun with --open" >&2
    return 1
  fi
}

if [[ "$OPEN" -eq 1 ]]; then
  open_config
else
  echo "edit $CONFIG_PATH and replace placeholder API keys before running iota"
fi
