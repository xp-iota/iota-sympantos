# Docker 容器化方案

Docker 方案提供一个常驻 `iota __daemon` 和一组可观测性服务。它适合把 ACP 后端、SQLite 状态和 OpenTelemetry/Loki/Jaeger/Grafana 放进固定运行环境中。

## 架构

| 服务 | 端口 | 作用 |
| :--- | :--- | :--- |
| `iota-daemon` | `47661` | 运行 `iota __daemon`，处理 CLI/desktop daemon 请求 |
| `otel-collector` | `4317`, `4318` | 接收 OTLP trace/metric/log |
| `jaeger` | `16686` | 查询 trace |
| `prometheus` | `9090` | 抓取 metrics |
| `loki` | `3100` | 日志查询 |
| `grafana` | `3000` | 可观测性 UI，默认 `admin/admin` |

## 启动

从仓库根目录运行：

```bash
docker compose -f docker/docker-compose.yml up -d --build
```

查看日志：

```bash
docker logs -f iota-daemon
```

停止：

```bash
docker compose -f docker/docker-compose.yml down
```

## 镜像内容

`docker/Dockerfile` 使用两阶段构建：

- builder：`rust:1.95-slim-bookworm`，构建 `iota-cli` release binary；该版本与 workspace 的 `rust-version = "1.95.0"` 对齐。
- runtime：`debian:bookworm-slim`，安装 `git`、`curl`、`sqlite3`、`nodejs`、`npm`，以非 root 用户 `iota` 运行。

运行时默认：

```bash
IOTA_DAEMON_ADDR=0.0.0.0:47661
OTEL_EXPORTER_OTLP_ENDPOINT=http://otel-collector:4317
```

容器入口 `docker/entrypoint.sh` 会在 `/home/iota/.i6/nimia.yaml` 不存在时，从 `/home/iota/.i6/nimia.yaml.template` 初始化一份模板配置。

## 数据卷

| 挂载 | 容器路径 | 作用 |
| :--- | :--- | :--- |
| `iota-config` | `/home/iota/.i6` | `nimia.yaml`、SQLite store、logs、skills、kanban |
| `../` | `/workspace` | 宿主机仓库工作区 |

不要把真实 API key 写入镜像。应在命名卷或容器内的 `/home/iota/.i6/nimia.yaml` 中配置。

## 宿主机复用容器 daemon

宿主机 CLI 可以连接容器内 daemon：

```bash
export IOTA_DAEMON_ADDR=127.0.0.1:47661
iota run --daemon hermes "ping"
```

Windows PowerShell：

```powershell
$env:IOTA_DAEMON_ADDR="127.0.0.1:47661"
iota run --daemon hermes "ping"
```

## 容器内运行命令

```bash
docker exec -it iota-daemon iota check
docker exec -it iota-daemon iota run hermes "ping"
docker exec -it iota-daemon iota
```

TUI 必须使用 `-it`，否则 crossterm 无法正确接管终端。

## 配置维护

复制配置到宿主机编辑：

```bash
docker cp iota-daemon:/home/iota/.i6/nimia.yaml ./nimia.yaml
```

写回：

```bash
docker cp ./nimia.yaml iota-daemon:/home/iota/.i6/nimia.yaml
```

Linux 权限问题通常来自宿主机目录 UID/GID 与容器用户 `1000:1000` 不一致。可调整 compose 的 `user` 字段，或确保项目目录对 UID 1000 可读写。
