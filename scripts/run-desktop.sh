#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DESKTOP_DIR="$ROOT_DIR/crates/iota-desktop"
GRACE_SECONDS="${IOTA_DEV_DAEMON_STOP_GRACE_SECONDS:-3}"
DESKTOP_DEV_PORTS="${IOTA_DESKTOP_DEV_PORTS:-1420 1421}"

usage() {
  cat <<'USAGE'
Usage: scripts/dev-desktop.sh [--stop-only] [--] [extra npm tauri args...]

Stops existing iota daemon and desktop dev-server processes before starting the
Tauri desktop dev app.
The script builds the workspace iota CLI, installs frontend dependencies if
they are missing, and exports IOTA_CLI_PATH so Tauri autostarts the matching
daemon version.

Options:
  --stop-only   Stop daemon and desktop dev-server processes and exit.
  -h, --help    Show this help.

Environment:
  IOTA_DEV_DAEMON_STOP_GRACE_SECONDS  Seconds to wait before SIGKILL fallback. Default: 3.
  IOTA_DESKTOP_DEV_PORTS              Desktop dev ports to clear. Default: "1420 1421".
  CARGO_TARGET_DIR                    Optional Cargo target directory.
USAGE
}

find_daemon_pids() {
  ps -axo pid=,command= \
    | awk '
      {
        command = $0
        sub(/^[[:space:]]*[0-9]+[[:space:]]+/, "", command)
        has_daemon_arg = command ~ /(^|[[:space:]])__daemon([[:space:]]|$)/
        is_iota_binary = command ~ /(^|[[:space:]\/])iota(\.exe)?([[:space:]]|$)/
        is_cargo_iota = command ~ /cargo[[:space:]]+run/ && command ~ /--[[:space:]]+__daemon/
        if (has_daemon_arg && (is_iota_binary || is_cargo_iota)) {
          print $1
        }
      }
    ' \
    | sort -u
}

stop_daemons() {
  local pids
  pids="$(find_daemon_pids | tr '\n' ' ')"
  if [[ -z "${pids// }" ]]; then
    echo "no iota daemon processes found"
    return 0
  fi

  echo "stopping iota daemon process(es): $pids"
  # shellcheck disable=SC2086
  kill $pids 2>/dev/null || true

  local deadline=$((SECONDS + GRACE_SECONDS))
  while [[ "$SECONDS" -lt "$deadline" ]]; do
    pids="$(find_daemon_pids | tr '\n' ' ')"
    if [[ -z "${pids// }" ]]; then
      echo "iota daemon stopped"
      return 0
    fi
    sleep 0.2
  done

  pids="$(find_daemon_pids | tr '\n' ' ')"
  if [[ -n "${pids// }" ]]; then
    echo "daemon still running after ${GRACE_SECONDS}s; sending SIGKILL: $pids"
    # shellcheck disable=SC2086
    kill -9 $pids 2>/dev/null || true
  fi
}

find_port_pids() {
  local port
  for port in "$@"; do
    if command -v lsof >/dev/null 2>&1; then
      lsof -tiTCP:"$port" -sTCP:LISTEN 2>/dev/null || true
    fi
  done | sort -u
}

stop_desktop_dev_servers() {
  local ports=($DESKTOP_DEV_PORTS)
  if ((${#ports[@]} == 0)); then
    return 0
  fi

  local pids
  pids="$(find_port_pids "${ports[@]}" | tr '\n' ' ')"
  if [[ -z "${pids// }" ]]; then
    echo "no desktop dev server processes found on port(s): ${ports[*]}"
    return 0
  fi

  echo "stopping desktop dev server process(es) on port(s) ${ports[*]}: $pids"
  # shellcheck disable=SC2086
  kill $pids 2>/dev/null || true

  local deadline=$((SECONDS + GRACE_SECONDS))
  while [[ "$SECONDS" -lt "$deadline" ]]; do
    pids="$(find_port_pids "${ports[@]}" | tr '\n' ' ')"
    if [[ -z "${pids// }" ]]; then
      echo "desktop dev server stopped"
      return 0
    fi
    sleep 0.2
  done

  pids="$(find_port_pids "${ports[@]}" | tr '\n' ' ')"
  if [[ -n "${pids// }" ]]; then
    echo "desktop dev server still running after ${GRACE_SECONDS}s; sending SIGKILL: $pids"
    # shellcheck disable=SC2086
    kill -9 $pids 2>/dev/null || true
  fi
}

iota_cli_path() {
  local target_dir="${CARGO_TARGET_DIR:-$ROOT_DIR/target}"
  local exe_name="iota"
  if [[ "${OS:-}" == "Windows_NT" ]]; then
    exe_name="iota.exe"
  fi
  printf '%s\n' "$target_dir/debug/$exe_name"
}

ensure_frontend_deps() {
  # A one-click dev entry point cannot assume `npm install` was already run.
  # `node_modules/.bin/vite` is the concrete artifact `tauri dev`'s
  # beforeDevCommand needs; its absence is what produces the confusing
  # "vite: command not found" failure, so treat it as the install signal
  # rather than only checking for the `node_modules` directory itself.
  if [[ -x "$DESKTOP_DIR/node_modules/.bin/vite" ]]; then
    return 0
  fi

  if ! command -v npm >/dev/null 2>&1; then
    echo "npm is required to install frontend dependencies but was not found in PATH" >&2
    exit 1
  fi

  echo "frontend dependencies not found, running npm install in $DESKTOP_DIR..."
  (cd "$DESKTOP_DIR" && npm install)

  if [[ ! -x "$DESKTOP_DIR/node_modules/.bin/vite" ]]; then
    echo "npm install completed but vite is still missing from $DESKTOP_DIR/node_modules/.bin" >&2
    exit 1
  fi
}

build_iota_cli() {
  echo "building current workspace iota CLI..."
  (cd "$ROOT_DIR" && cargo build -p iota-cli --bin iota)

  local cli_path
  cli_path="$(iota_cli_path)"
  if [[ ! -x "$cli_path" ]]; then
    echo "built iota CLI was not found or is not executable: $cli_path" >&2
    exit 1
  fi

  export IOTA_CLI_PATH="$cli_path"
  echo "using IOTA_CLI_PATH=$IOTA_CLI_PATH"
}

STOP_ONLY=0
EXTRA_ARGS=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --stop-only)
      STOP_ONLY=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    --)
      shift
      EXTRA_ARGS+=("$@")
      break
      ;;
    *)
      EXTRA_ARGS+=("$1")
      shift
      ;;
  esac
done

stop_daemons
stop_desktop_dev_servers

if [[ "$STOP_ONLY" -eq 1 ]]; then
  exit 0
fi

build_iota_cli

ensure_frontend_deps

cd "$DESKTOP_DIR"
exec npm run tauri -- dev ${EXTRA_ARGS[@]+"${EXTRA_ARGS[@]}"}
