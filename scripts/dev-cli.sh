#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [[ ! -f "$HOME/.i6/nimia.yaml" ]]; then
  echo "model config is missing: $HOME/.i6/nimia.yaml" >&2
  echo "run scripts/configure-model.sh --init first" >&2
  exit 1
fi

cd "$ROOT_DIR"
exec cargo run -p iota-cli --quiet -- "$@"
