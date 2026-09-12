#!/usr/bin/env sh
set -eu

workspace=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$workspace"

for package in iota-sympantos-kanban iota-sympantos-core; do
  if [ "${1:-}" = "--dry-run" ]; then
    echo "Packaging $package"
    cargo package -p "$package" --allow-dirty
  else
    echo "Publishing $package"
    cargo publish -p "$package"
  fi
done
