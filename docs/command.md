# iota-sympantos 命令参考

本文覆盖 CLI 命令和 TUI slash command。所有后端配置都从 `~/.i6/nimia.yaml` 读取，不读取项目级配置。

## CLI 命令

| 命令 | 作用 |
| :--- | :--- |
| `iota` | 进入交互式 TUI |
| `iota check [--daemon|-d]` | 输出合并后的后端 JSON 信息；带 daemon 时先 warm 当前目录 |
| `iota run [backend] [options] <prompt>` | 单次 prompt 执行；默认 backend 为 Codex |
| `iota run --daemon [backend] <prompt>` | 通过本机 daemon 执行，必要时静默启动 `iota __daemon` |
| `iota bench <cold|warm> [rounds] [--daemon|-d]` | 冷/热启动 benchmark |
| `iota bench-cold [rounds] [--daemon|-d]` | 兼容命令，等价于 cold benchmark |
| `iota bench-warm [rounds] [--daemon|-d]` | 兼容命令，等价于 warm benchmark |
| `iota mcp <context|fun>` | 启动 MCP stdio server |
| `iota context-mcp` | 兼容命令，启动 iota-context MCP |
| `iota fun-mcp` | 兼容命令，启动 iota-fun MCP |
| `iota skill pull <source> [name] [--sha256 <hex-digest>]` | 从本地路径或 HTTPS URL 拉取 skill；网络源拒绝 HTTP，可选校验 SHA-256 后原子发布到 `~/.i6/skills/registry-cache` |
| `iota observability <logging|tokens|metrics|logs|trace> ...` | 查询本地 token usage、Prometheus 文本指标、Loki 日志或 Jaeger trace |
| `iota logs <execution_id>` | `iota observability logs` 的顶层别名 |
| `iota trace <trace_id>` | `iota observability trace` 的顶层别名 |
| `iota kanban <subcommand>` | Kanban board/task、dispatch、event sync |
| `iota __daemon` | 内部 daemon 入口，不作为普通用户入口 |

`iota run` 常用选项：

| 选项 | 作用 |
| :--- | :--- |
| `--backend <name>` | 指定后端，也可直接把 backend 放在 prompt 前 |
| `--cwd <path>` | 指定执行工作目录 |
| `--daemon` / `-d` | 经 daemon 路由（默认开启） |
| `--no-daemon` | 绕过 daemon，直接在进程内执行 |
| `--show-native` | 打印原生 ACP wire 内容；不能与 `--daemon` 同用 |
| `--log-events` | 输出 normalized `RuntimeEvent` |
| `--timing` | 输出 route、spawn、init、prompt、total timing JSON |
| `--timeout-ms <ms>` | 覆盖 ACP prompt timeout |

## daemon 与 desktop 协议

`iota run --daemon` 与桌面端共用一个本机 daemon（`iota __daemon`），监听 loopback，鉴权 token 位于 `~/.i6/daemon.token`（可用 `IOTA_DAEMON_TOKEN_PATH` 覆盖）。

| 项目 | 值 |
| :--- | :--- |
| 协议版本 | `3`（`DESKTOP_PROTOCOL_VERSION`），可接受范围 `PROTOCOL_VERSION_MIN`..`PROTOCOL_VERSION_MAX` |
| 握手 | 客户端先发 `hello`（含 client 名与 token），服务端回 `hello_accepted`（含 protocol_version / min_version / max_version）或 `protocol_error` |
| 关联 ID | 客户端消息可带 `request_id`，响应回带同一个值 |
| 幂等 | 同一连接上重放已处理过的 `request_id` 会得到 `protocol_error`（`invalid_request`，说明 duplicate），不会重复执行副作用 |
| 错误码 | `DaemonErrorCode` 区分可重试与不可重试，客户端据此决定是否自动重连重发 |
| 未知字段 | 反序列化忽略未知字段，便于新增字段时旧客户端继续工作 |

客户端消息：`hello`、`start_turn`、`cancel_turn`、`respond_approval`、`get_config`、`save_backend_model`、`check_backend`、`get_observability_summary`、`get_memory_context_snapshot`、`ping`。

服务端消息：`hello_accepted`、`protocol_error`、`turn_started`、`text_chunk`、`turn_event`、`approval_requested`、`approval_responded`、`turn_completed`、`turn_failed`、`turn_cancelled`、`config_snapshot`、`backend_check_result`、`observability_summary`、`memory_context_snapshot`、`pong`。

密钥边界：`config_snapshot` 只回 `api_key_configured`（布尔），`api_key_update` 是入站专用字段，服务端永不回传原始 key。

## 后端别名

| 后端 | 默认命令 | 别名 |
| :--- | :--- | :--- |
| Claude Code | `npx -y @agentclientprotocol/claude-agent-acp@0.32.0` | `claude`, `claude-code`, `claudecode` |
| Codex | `npx -y @zed-industries/codex-acp@0.12.0` | `codex` |
| Gemini CLI | `npx -y @google/gemini-cli@0.41.2 --acp` | `gemini`, `gemini-cli` |
| Hermes | `hermes acp` | `hermes`, `hermes-agent` |
| OpenCode | `npx -y opencode-ai@1.14.40 acp` | `opencode`, `open-code` |

## Kanban 命令

Kanban 数据默认写入 `~/.i6/kanban/iota.db`，shadow workspace 位于 `~/.i6/kanban/shadows`。

| 命令 | 作用 |
| :--- | :--- |
| `iota kanban create-board <slug> <name>` | 创建 board |
| `iota kanban create-task <board-id> <title>` | 创建 task，初始状态为 `triage` |
| `iota kanban move <id> <status>` | 通过状态机迁移 task |
| `iota kanban dispatch <id> [--timeout <secs>]` | 用 Hermes worker 执行 task |
| `iota kanban specify <id>` | 通过 AdvancedBridge 细化 task |
| `iota kanban decompose <id>` | 将 task 拆分为子任务 |
| `iota kanban export <path> [cursor]` | 导出 event bundle |
| `iota kanban import <path>` | 导入 event bundle |
| `iota kanban serve-sync [addr]` | 启动 event sync server，默认 `127.0.0.1:47662` |
| `iota kanban pull <addr> [cursor]` | 从远端拉取并导入 events |
| `iota kanban push <addr> [cursor]` | 推送 events 到远端 |

合法状态：`triage`、`todo`、`ready`、`running`、`done`、`archived`、`blocked`。

## TUI Slash Commands

在 TUI 输入框行首输入 `/` 使用 slash command。iota 会优先处理本地命令；未识别的命令会作为普通 prompt 透传给当前后端。

| Command | Aliases | 作用 |
| :--- | :--- | :--- |
| `/?` | `/help` | 显示 TUI 帮助浮层；Hermes/Gemini 的 native `/help` 由后端处理时除外 |
| `/backend` | `/backends` | 列出可用后端 |
| `/backend <name>` |  | 切换后端 |
| `/claude` |  | 切换到 Claude Code |
| `/codex` |  | 切换到 Codex |
| `/gemini` |  | 切换到 Gemini |
| `/hermes` |  | 切换到 Hermes |
| `/opencode` |  | 切换到 OpenCode |
| `/clear` | `/new`, `/reset` | 清空可见 transcript |
| `/model` | `/models` | 显示当前后端模型 |
| `/status` | `/stats`, `/usage`, `/about`, `/profile` | 显示当前状态 |
| `/export` | `/save` | 导出当前 transcript |
| `/quit` | `/exit` | 打开退出确认 |
| `/q` |  | OpenCode session 下的退出确认 |
| `/kanban ...` |  | 在 TUI 中执行 Kanban slash command |
| `/memory` | `/mem` | 在 TUI 中执行本地 memory recall / hybrid search |

已验证可 native 处理的 provider command：

| Command | Aliases | 后端 |
| :--- | :--- | :--- |
| `/help` |  | Hermes, Gemini |
| `/init` |  | Gemini |
| `/compact` | `/compress` | Hermes, OpenCode |
| `/memory` |  | Gemini |
| `/queue` |  | Hermes |
| `/steer` |  | Hermes |
| `/tools` |  | Hermes |
| `/rollback` | `/restore` | Gemini |
| `/extensions` |  | Gemini |

Claude Code 和 Codex 的 ACP text prompt 通常不会把 slash command 当作 native command 执行；自定义命令仍会以普通文本交给后端处理。
