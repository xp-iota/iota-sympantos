#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
STACK_DIR="$ROOT_DIR/docker/observability"
RUN_LOG="${TMPDIR:-/tmp}/iota-observability-run.log"

BACKEND="${IOTA_VERIFY_BACKEND:-codex}"
PROMPT="${IOTA_VERIFY_PROMPT:-Reply with exactly: iota-observability-ok-$(date +%s). Do not call tools or run commands.}"
RESTART_STACK=1
DOWN_STACK=0
OTEL_GRPC_PORT="${OTEL_GRPC_PORT:-4317}"
OTEL_HTTP_PORT="${OTEL_HTTP_PORT:-4318}"
JAEGER_PORT="${JAEGER_PORT:-16686}"
PROMETHEUS_PORT="${PROMETHEUS_PORT:-9090}"
LOKI_PORT="${LOKI_PORT:-3100}"
GRAFANA_PORT="${GRAFANA_PORT:-3000}"
OTEL_EXPORTER_OTLP_ENDPOINT="${OTEL_EXPORTER_OTLP_ENDPOINT:-http://localhost:${OTEL_GRPC_PORT}}"
IOTA_LOKI_URL="${IOTA_LOKI_URL:-http://localhost:${LOKI_PORT}}"
IOTA_JAEGER_URL="${IOTA_JAEGER_URL:-http://localhost:${JAEGER_PORT}}"
PROMETHEUS_METRIC_QUERIES="${PROMETHEUS_METRIC_QUERIES:-iota_execution_count_total iota_execution_count}"

export OTEL_GRPC_PORT OTEL_HTTP_PORT JAEGER_PORT PROMETHEUS_PORT LOKI_PORT GRAFANA_PORT

usage() {
  cat <<'USAGE'
Usage: scripts/verify-observability.sh [options]

Options:
  --no-restart          Do not stop port conflicts or run docker compose up.
  --backend <name>      Backend to run (default: codex or IOTA_VERIFY_BACKEND).
  --prompt <text>       Prompt to send to the backend.
  --keep-running        Leave the Docker stack running after verification.
  --down                Run docker compose down before exiting.
  -h, --help            Show this help.
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --no-restart)
      RESTART_STACK=0
      shift
      ;;
    --backend)
      BACKEND="${2:?missing value for --backend}"
      shift 2
      ;;
    --prompt)
      PROMPT="${2:?missing value for --prompt}"
      shift 2
      ;;
    --keep-running)
      DOWN_STACK=0
      shift
      ;;
    --down)
      DOWN_STACK=1
      shift
      ;;
    -h| :--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown option: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required command: $1" >&2
    exit 1
  fi
}

wait_http() {
  local url="$1"
  local name="$2"
  local attempts="${3:-60}"
  for _ in $(seq 1 "$attempts"); do
    if curl -fsS "$url" >/dev/null 2>&1; then
      echo "$name ready: $url"
      return 0
    fi
    sleep 1
  done
  echo "$name did not become ready: $url" >&2
  return 1
}

stop_containers_on_ports() {
  local ports=("$@")
  local containers=()
  local line id published private_port type
  while IFS= read -r line; do
    id="${line%% *}"
    line="${line#* }"
    while read -r published private_port type; do
      for port in "${ports[@]}"; do
        if [[ "$published" == "$port" ]]; then
          containers+=("$id")
        fi
      done
    done < <(docker port "$id" 2>/dev/null | sed -n 's#^\([0-9]*/tcp\) -> .*:\([0-9][0-9]*\)$#\2 \1 tcp#p')
  done < <(docker ps --format '{{.ID}} {{.Names}}')

  if ((${#containers[@]} > 0)); then
    local unique
    unique="$(printf '%s\n' "${containers[@]}" | sort -u | tr '\n' ' ')"
    echo "stopping containers using observability ports: $unique"
    # shellcheck disable=SC2086
    docker stop $unique >/dev/null
  fi
}

extract_execution_id() {
  grep '\[iota run timing\]' "$RUN_LOG" \
    | tail -1 \
    | sed 's/^.*\[iota run timing\] //' \
    | jq -r '.execution_id // empty'
}

query_prometheus_metric() {
  local query="$1"
  curl -fsS --get \
    --data-urlencode "query=$query" \
    "http://localhost:${PROMETHEUS_PORT}/api/v1/query"
}

wait_prometheus_metric() {
  local metric body
  for _ in $(seq 1 12); do
    for metric in $PROMETHEUS_METRIC_QUERIES; do
      if body="$(query_prometheus_metric "$metric" 2>/dev/null)" \
        && jq -e '.data.result | length > 0' >/dev/null <<<"$body"; then
        echo "prometheus metric ready: $metric"
        return 0
      fi
    done
    echo "prometheus metric not indexed yet; retrying in 5s"
    sleep 5
  done
  echo "failed to find Prometheus metrics. Tried: $PROMETHEUS_METRIC_QUERIES" >&2
  return 1
}

need cargo
need curl
need docker
need jq

cd "$STACK_DIR"
if [[ "$DOWN_STACK" -eq 1 ]]; then
  trap 'cd "$STACK_DIR" && docker compose down' EXIT
fi
if [[ "$RESTART_STACK" -eq 1 ]]; then
  stop_containers_on_ports \
    "$OTEL_GRPC_PORT" "$OTEL_HTTP_PORT" "$JAEGER_PORT" \
    "$PROMETHEUS_PORT" "$LOKI_PORT" "$GRAFANA_PORT"
  docker compose up -d
fi

wait_http "http://localhost:${JAEGER_PORT}/api/services" "jaeger"
wait_http "http://localhost:${PROMETHEUS_PORT}/-/ready" "prometheus"
wait_http "http://localhost:${LOKI_PORT}/ready" "loki"

cd "$ROOT_DIR"
echo "running iota prompt through backend: $BACKEND"
set +e
OTEL_EXPORTER_OTLP_ENDPOINT="$OTEL_EXPORTER_OTLP_ENDPOINT" \
  cargo run -- run "$BACKEND" --timing "$PROMPT" 2>&1 | tee "$RUN_LOG"
RUN_STATUS=${PIPESTATUS[0]}
set -e

EXECUTION_ID="$(extract_execution_id)"
if [[ -z "$EXECUTION_ID" || "$EXECUTION_ID" == "null" ]]; then
  echo "failed to extract execution_id from $RUN_LOG" >&2
  exit 1
fi

echo "execution_id=$EXECUTION_ID"
if [[ "$RUN_STATUS" -ne 0 ]]; then
  echo "iota run exited with status $RUN_STATUS; continuing because execution_id was emitted"
fi
echo "querying Loki logs"
IOTA_LOKI_URL="$IOTA_LOKI_URL" cargo run -- logs "$EXECUTION_ID"

echo "querying Jaeger trace via execution id"
TRACE_OK=0
for _ in $(seq 1 12); do
  if IOTA_LOKI_URL="$IOTA_LOKI_URL" IOTA_JAEGER_URL="$IOTA_JAEGER_URL" \
    cargo run -- trace --execution "$EXECUTION_ID"; then
    TRACE_OK=1
    break
  fi
  echo "trace not indexed yet; retrying in 5s"
  sleep 5
done

if [[ "$TRACE_OK" -ne 1 ]]; then
  echo "failed to query Jaeger trace for execution_id=$EXECUTION_ID" >&2
  exit 1
fi

echo "querying Prometheus metrics"
wait_prometheus_metric

echo "observability verification completed"
