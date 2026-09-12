#!/usr/bin/env bash
set -euo pipefail

# iota 主配置目录
I6_DIR="/home/iota/.i6"
CONFIG_FILE="$I6_DIR/nimia.yaml"
TEMPLATE_FILE="$I6_DIR/nimia.yaml.template"

# 1. 确保配置目录存在
mkdir -p "$I6_DIR"

# 2. 如果 nimia.yaml 不存在，尝试使用 template 进行初始化
if [ ! -f "$CONFIG_FILE" ]; then
    echo "=========================================================="
    echo " [Iota] nimia.yaml not found."
    if [ -f "$TEMPLATE_FILE" ]; then
        echo " [Iota] Initializing default nimia.yaml from template..."
        cp "$TEMPLATE_FILE" "$CONFIG_FILE"
        echo " [Iota] Successfully initialized $CONFIG_FILE"
    else
        echo " [Iota] WARNING: Template file not found. Creating a blank one."
        touch "$CONFIG_FILE"
    fi
    echo " [Iota] IMPORTANT: Please configure your API keys in your host's"
    echo "        mounted volume path or edit the config inside the container."
    echo "=========================================================="
fi

# 3. 运行模式派发
# 如果没有指定参数，或者指定参数是 "daemon"，启动 Iota 内部守护进程
if [ $# -eq 0 ] || [ "$1" = "daemon" ]; then
    echo " [Iota] Starting iota background agent daemon..."
    echo " [Iota] Listening address: ${IOTA_DAEMON_ADDR:-0.0.0.0:47661}"
    echo " [Iota] Telemetry receiver: ${OTEL_EXPORTER_OTLP_ENDPOINT:-http://otel-collector:4317}"
    exec iota __daemon
else
    # 否则，把参数透传给 iota 命令行（例如运行 check, run 等）
    exec iota "$@"
fi
