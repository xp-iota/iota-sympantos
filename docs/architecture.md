# iota-sympantos 架构总览

iota-sympantos 是一个 Rust workspace，用 ACP 编排多个 AI 编程助手后端，并在同一套配置、Context Fabric、memory、skill、observability 和 Kanban 体系下提供 CLI、TUI、daemon 和 Tauri desktop 入口。

相关文档：

- [code-call-chains.md](code-call-chains.md)
- [command.md](command.md)
- [observability.md](observability.md)
- [debugging.md](debugging.md)
- [desktop-mvp-acceptance.md](desktop-mvp-acceptance.md)

## Workspace 结构

```text
crates/
├── iota-cli/
│   └── src/
│       ├── main.rs
│       ├── cli/                  # run/check/bench/kanban/skill/observability/mcp/__daemon
│       └── tui/                  # interactive terminal UI
├── iota-core/
│   └── src/
│       ├── acp/                  # ACP backend process + JSON-RPC wire
│       ├── config/               # ~/.i6/nimia.yaml
│       ├── context/              # ContextEngine + capsule
│       ├── daemon/               # TCP daemon + desktop protocol
│       ├── engine/               # IotaEngine orchestration
│       ├── mcp/                  # iota-context MCP + router + tool dispatch
│       ├── memory/               # memory taxonomy + embedding + recall
│       ├── runtime_event/        # normalized event stream
│       ├── skill/                # skill registry + engine-run MCP skill + iota-fun
│       ├── store/                # cache/approvals/ledger/observability SQLite stores
│       └── telemetry/            # tracing + OpenTelemetry
├── iota-kanban/
│   └── src/                      # board/task state machine, SQLite event sourcing, worker, sync
└── iota-desktop/
    ├── src/                      # React UI
    └── src-tauri/                # Tauri commands + daemon client + Kanban commands
```

## 分层架构

```text
Entry
  iota-cli main.rs
  iota-desktop Tauri main.rs

Presentation
  CLI commands
  TUI
  React desktop workbench

Service orchestration
  IotaEngine
  daemon EnginePool
  desktop daemon protocol
  Kanban dispatcher/bridge

Context and tools
  ContextEngine
  MemoryStore
  SkillRegistry
  MCP server/router/tool_dispatch
  iota-fun

Protocol and external boundaries
  ACP child processes
  MCP stdio sidecars
  TCP daemon JSON-line protocol
  git/compiler/interpreter child processes

Stores and observability
  SQLite stores under ~/.i6
  RuntimeEvent
  OpenTelemetry, Loki, Jaeger, Prometheus
```

## Crate 职责

| Crate | 职责 |
| :--- | :--- |
| `iota-cli` | 用户命令入口、TUI、daemon autostart、observability 查询、Kanban CLI |
| `iota-core` | Workspace 目录名；registry 包名为 `iota-sympantos-core`，library target 为 `iota_core`。承载 ACP/MCP/daemon/engine/config/context/memory/skill/store/telemetry 核心运行时 |
| `iota-sympantos-kanban` | 已发布的独立 library crate；承载 Kanban 领域模型、状态机、SQLite event sourcing、Hermes worker、shadow workspace、event sync |
| `iota-desktop` | Tauri + React desktop，复用 daemon streaming protocol；提供 chat/config/inspector/memory/context UI，并在 Rust commands 中接入 Kanban store |

## 核心模块

| 模块 | 职责 |
| :--- | :--- |
| `engine/` | 按 `(backend, cwd)` 复用 ACP client；处理 session ledger、handoff、memory recall/write、skill short-circuit、context capsule、ACP 调用、events 和 store 写回 |
| `acp/` | 后端枚举、命令解析、子进程生命周期、`initialize/session/new/session/prompt`、stream reader、permission、wire parse |
| `daemon/` | 默认 `127.0.0.1:47661` TCP daemon；敏感 legacy/desktop 请求使用 CSPRNG owner-only token 认证，Unix 还支持 owner-only UDS 与同 UID peer credential 校验；config、backend check、observability、memory/context snapshot |
| `config/` | 唯一读取 `~/.i6/nimia.yaml`；生成 effective config、backend command/env、context options、MCP server 注入；包含 `effective.rs`（resolved config with defaults）、`helpers.rs`（path expansion, command normalization）、`paths.rs`（store path resolution） |
| `context/` | 组装 `<iota-context>` capsule：session、memory tools、memory buckets、working memory、workspace、skills、handoff；预算在 token 空间执行、注入内容统一 XML 转义（见下） |
| `memory/` | 六桶 memory taxonomy、FTS/LIKE、vector/hybrid search、embedding API 或 local trigram fallback |
| `skill/` | workspace/config/home skill 加载；trigger 匹配；engine-run MCP skill；skill pull/cache；iota-fun 7 语言 MCP server |
| `mcp/` | iota-context MCP stdio server、MCP client、ACP tool-call router、共享 tool dispatch |
| `store/` | execution lifecycle、approval、session ledger、observability SQLite stores |
| `runtime_event/` | 把 ACP update、complete、permission、usage、tool、error 归一为 `RuntimeEvent` |
| `telemetry/` | tracing、OpenTelemetry meter/exporter；stderr 出站前经 `redact` 脱敏 |

## 运行路径

### CLI 直接执行

```text
iota run [backend] <prompt>
  -> cli::run()
  -> acp::parse_acp_args()
  -> config::read_config()
  -> IotaEngine::create_session()
  -> IotaEngine::run_with_timing()
       -> execution lifecycle
       -> session ledger + handoff
       -> memory extraction / recall
       -> skill match and optional engine-run MCP skill
       -> context capsule
       -> ACP session/prompt
       -> RuntimeEvent + token usage + memory/session writeback
  -> stdout
```

### CLI 经 daemon 执行

```text
iota run --daemon ...
  -> daemon client connects IOTA_DAEMON_ADDR or 127.0.0.1:47661
  -> if failed, spawn current_exe __daemon
  -> daemon EnginePool::engine_for(cwd)
  -> same IotaEngine prompt path
  -> DaemonPromptResponse JSON line
```

### TUI 执行

```text
iota
  -> tui::run(config)
  -> TuiApp + IotaEngine
  -> install TUI approval channel
  -> crossterm raw mode + mouse capture + TerminalGuard
  -> event loop
       -> input/history/slash commands
       -> tokio engine task
       -> streaming chunks over mpsc
       -> approval overlay
       -> markdown/status/render
```

### Desktop 执行

```text
React ChatWorkbench
  -> Tauri command submit_prompt
  -> src-tauri daemon_client::start_turn()
  -> TCP Hello + StartTurn
  -> daemon desktop handler
  -> IotaEngine::run_with_timing()
  -> TextChunk / TurnEvent / ApprovalRequested / TurnCompleted
  -> Tauri emits daemon-message
  -> turnsReducer updates transcript and inspector
```

## ACP 后端

| Backend | 默认命令 | 别名 | 备注 |
| :--- | :--- | :--- | :--- |
| Claude Code | `npx -y @agentclientprotocol/claude-agent-acp@0.32.0` | `claude`, `claude-code`, `claudecode` | Claude Code ACP adapter |
| Codex | `npx -y @zed-industries/codex-acp@0.12.0` | `codex` | Codex ACP adapter |
| Gemini CLI | `npx -y @google/gemini-cli@0.41.2 --acp` | `gemini`, `gemini-cli` | Gemini ACP mode |
| Hermes | `hermes acp` | `hermes`, `hermes-agent` | 不覆盖 `HERMES_HOME` |
| OpenCode | `npx -y opencode-ai@1.14.40 acp` | `opencode`, `open-code` | OpenCode ACP mode |

Windows 上 `normalize_command()` 会把 `npx` 改为 `npx.cmd`。

### ACP 会话恢复能力协商

`AcpClient::start()` 会从 `initialize` 响应中分别解析两类恢复能力：

- `/agentCapabilities/sessionCapabilities/resume`：允许使用 `session/resume` 恢复后端原生会话；
- `/agentCapabilities/loadSession`：允许回退到 ACP v1 的 `session/load` 流程。

恢复时优先使用 `session/resume`，后端未声明该能力时才尝试 `session/load`。两项能力独立解析；字段缺失或响应形状不匹配时均视为不支持。此路径采用 fail-closed 语义：当后端没有明确声明任一能力时，`restore_session()` 返回错误，不会静默创建替代会话并宣称上下文连续。

恢复请求还要求 session ID 为 1..=1024 字节、恢复 cwd 与 ACP client 进程 cwd 一致，且 client 当前未持有另一个活动 session。

## 配置模型

配置只从 `~/.i6/nimia.yaml` 读取。顶层包含五个 backend section、`model`、`context_engine`、`context_engine_backend` 和 store/observability 相关配置。

Model env 映射：

| Backend | 映射 |
| :--- | :--- |
| Claude Code | `ANTHROPIC_API_KEY`、`ANTHROPIC_AUTH_TOKEN`、`ANTHROPIC_BASE_URL`、`ANTHROPIC_MODEL` |
| Codex | `OPENAI_API_KEY`、`ROUTER_API_KEY`、`OPENAI_BASE_URL`、`OPENAI_MODEL`，并按需追加 Codex `-c` 配置 |
| Gemini | `GEMINI_API_KEY`、`GEMINI_MODEL` |
| Hermes | `HERMES_INFERENCE_PROVIDER`、`HERMES_MODEL` 和 provider 原生 key/base URL |
| OpenCode | `OPENCODE_MODEL` |

Hermes 使用自己的默认 home，配置和 desktop 都不应覆盖 `HERMES_HOME`。

## 数据和存储

| Store | 默认路径 | 作用 |
| :--- | :--- | :--- |
| `MemoryStore` | `~/.i6/context/memory.sqlite` | memory taxonomy、recall、search、embedding |
| `CacheStore` | `~/.i6/context/events.sqlite` | execution lifecycle、status、fencing |
| `ObservabilityStore` | `~/.i6/context/events.sqlite` | token usage events、summary、metrics |
| `SessionLedger` | `~/.i6/context/store.sqlite` | sessions、backend sessions、turns、handoff |
| `ApprovalStore` | `~/.i6/context/store.sqlite` | approval request/decision |
| `SqliteKanbanStore` | `~/.i6/kanban/iota.db` | board/task/comment/link/run/event sourcing |

### 上下文预算与注入边界

- 预算按 `~/.i6/nimia.yaml` 的 `memory_chars` / `skills_chars` / `working_memory_chars` / `workspace_chars` / `handoff_chars` 配置，但按 token 执行：配置值视为 ASCII 等效字符数，除以 4 得到 token 预算，内容用 `context::tokens::estimate_tokens` 估算（CJK 按 1 token/字）。同样的配置对中文与英文收敛到相同的 token 成本。
- 超预算时不切在内容中间：记忆按 identity → preference → strategic → domain → procedural → episodic 的优先级整条保留或丢弃，workspace / handoff / skills / working-memory 只在行边界截断。
- 注入内容（记忆、handoff、skill 描述、workspace 输出、working memory）统一转义 `&` `<` `>`，capsule 里的标签只可能是 iota 自己写的；`trivial` 提示词走的最小 capsule 用同一套规则。
- 每次组装的区段 token 数与是否裁剪通过 `ContextComposition` 返回，并上报 `iota.context.*` 指标。

### SQLite 连接与描述符

`store/db.rs` 的 `DbPool` 每个库文件一个 writer 连接，读连接按需打开（复用空闲、全忙才新增、开不出时降级到 writer 并记 `iota.db.reader_fallback`），上限可用 `IOTA_SQLITE_READ_CONNECTIONS` 覆盖。连接池按库文件在进程内共享（`ledger` 与 `approvals` 同为 `store.sqlite`，`cache` 与 `observability` 同为 `events.sqlite`），注册表持弱引用，最后一个 store 释放即关闭连接。WAL 下每连接约占 3 个描述符，而 daemon 为每个缓存 workspace 各持一套 store，所以这两点直接决定长跑进程的描述符占用。

## Kanban

`iota-sympantos-kanban` 提供：

- `Task`、`Board`、`Run`、`Comment`、`Link` 领域类型。
- 状态机：`triage -> todo -> ready -> running -> done -> archived`，支持 `blocked`。
- SQLite event-sourced store。
- `Dispatcher` 调度 ready task 给 Hermes worker。
- `ShadowMaterializer` 和 `ShadowWatcher` 管理 shadow workspace。
- `AdvancedBridge` 支持 `specify` 和 `decompose`。
- event sync：export/import/serve/pull/push。

## Desktop

Desktop 由 React + Tauri 组成，当前界面是一个 daemon-first 的本地工作台：

- Frontend：`ChatWorkbench` 是主 shell，包含 Chat/Config 视图、后端选择器、daemon 状态、prompt form 和可调宽右侧 inspector。
- Inspector：`RightInspector` 承载 `Observability`、`Memory`、`Context` 三个 tab；`MemoryContextWorkspace` 是只读 memory bucket 和 runtime context capsule 浏览器。
- State：`turnReducer` 只处理 turn 状态，折叠 `TextChunk`、`TurnEvent`、approval、cancel、failure 和 late daemon error。
- Tauri commands：config、prompt、approval、cancel、backend check、observability summary、memory/context snapshot、current workspace、Kanban CRUD。
- Daemon protocol：`DESKTOP_PROTOCOL_VERSION = 3`（`PROTOCOL_VERSION_MIN = MAX = 3`，旧版本客户端被明确拒绝并返回 `unsupported_version`），`DESKTOP_SCHEMA_VERSION = 1`。使用 `DaemonClientMessage` 和 `DaemonServerMessage` tagged enum。
- Protocol 错误模型：`ProtocolError` 携带 `code`（`DaemonErrorCode`）、`retryable` 和 `request_id`。确定性失败（版本不符、请求畸形、未认证、未找到）`retryable = false`，客户端不应重试；`unavailable` / `backend_error` / `internal` 可重试。
- 请求关联：每个请求可带 `request_id`；同一连接内重复的 `request_id` 会被拒绝而非重跑，避免客户端在丢失响应后重试时重复执行副作用。`Hello` 另带 `capabilities`，未知能力标签被忽略。
- Daemon autostart：优先连接默认 daemon；失败时尝试 desktop fallback address；再通过 `IOTA_CLI_PATH`、sibling binary 或 `PATH` 启动 `iota __daemon`。
- Kanban：`RightInspector` 的 Kanban tab 已挂载 `KanbanWorkspace`；它通过 Rust commands 读写并刷新 `~/.i6/kanban/iota.db` 中的 board、task、comment、link 和 run。

## 依赖规则

- 外部消费者通过 crates.io 使用 `iota-sympantos-core` 和 `iota-sympantos-kanban`；workspace 内部依赖同时声明 `version` 与 `path`，本地开发走 path，发布产物按 version 从 registry 解析。
- `iota-sympantos-core` 的 `LocalResources` 只接收宿主显式提供的本地 skill roots；标准 workspace 布局为 `<workspace>/skills` 与 `<workspace>/.iota/skills`。core 不下载项目资源，也不持有宿主的发布或凭证配置。
- `iota-sympantos-core` 默认不依赖 Kanban；启用 `kanban` feature 后接入 `iota-sympantos-kanban` 和 `iota_kanban_*` MCP tools。
- `iota-core/src/acp/` 不依赖 CLI、TUI、desktop 或 daemon UI 层。
- Store 模块只暴露 typed operations，不调用 UI、daemon、ACP client 或 MCP client。
- Presentation 层不直接拥有 ACP session；后端执行统一经过 `IotaEngine` 或 daemon API。
- 外部进程、TCP、network、SQLite 边界必须显式。
- 路径处理使用 `Path`/`PathBuf`，home 目录通过 `dirs::home_dir()`。
- 测试必须放在与源模块同目录的独立 `<module>_tests.rs` 文件中；源文件使用 `#[cfg(test)]` 和 `#[path = "<module>_tests.rs"] mod tests;` 引入。
- `*_tests.rs` 中的测试函数必须位于文件顶层，并通过 `crate::...` 绝对路径导入被测项；禁止内联 `mod tests` 和 `use super::*`。例如 ACP capability 测试位于 `acp/client_tests.rs`，本地资源测试位于 `resources_tests.rs`。

## 安全边界

- **Daemon 本机认证**：敏感 legacy/desktop 请求必须提供 `~/.i6/daemon.token` 中的 CSPRNG token；文件以 owner-only、原子方式创建并以恒定时间比较。Unix UDS 还会把 socket 收紧为 `0600` 并校验 peer UID。`hello`/`ping` 只用于协商和存活检查，不返回敏感数据。
- **网络边界**：TCP 仍是明文 JSON-line，因此认证并不等于传输加密。默认应保持 loopback；跨主机部署必须由受控容器网络或加密代理提供额外传输保护，且不得复制 token 到日志或镜像。
- **Kanban sync 认证**：`iota kanban serve-sync` 只监听 loopback，且**每个**请求（含读取）都必须携带 `~/.i6/daemon.token`。sync 对端可向本地库导入事件，未认证的同机进程若能调用即可改写 Kanban 状态，因此读取同样受保护。无 token 时服务端 fail-closed 拒绝启动。bundle 内的 `bundle_hash` 只是完整性校验，不是签名；真正的认证在传输层 token。
- **日志脱敏**：stderr layer 的 writer 是 `telemetry::redact::RedactingStderr`，日志出站前先替换已登记的密钥字面值（后端 API key 写入子进程环境时、daemon token 加载时登记），再按键名形状改写 `*_API_KEY=` / `"api_key":` / `authorization: Bearer` / `cookie:` 等。因为按文本匹配，stderr 不再输出 ANSI 颜色。桌面协议侧只回 `api_key_configured` 布尔值，`api_key_update` 是入站专用字段。
- **iota-fun 执行隔离**：这是**资源与环境隔离，不是强安全沙箱**。子进程在独立临时工作目录中编译运行（源码先经 SHA-256 校验后复制进临时目录），环境为白名单式（`env_clear` + 显式允许 PATH/HOME/TMP 等，并额外清除 API key、daemon/sync token、OTEL 上报凭据），受 wall-clock 超时、stdout/stderr 大小上限、并发执行数与构建缓存总量上限约束，超时按进程组终止整个进程树。它**不**使用 bubblewrap、Landlock、Job Object 等系统级沙箱，也不限制子进程的网络访问或文件系统可见范围；安全性依赖源码哈希校验与"这些源码本身是固定且无害的"这一前提。不要用 iota-fun 运行不可信代码。

## 扩展点

| 目标 | 修改位置 |
| :--- | :--- |
| 新 ACP 后端 | `acp/`、`config/`、`nimia.yaml.template`、backend env/home 映射 |
| 新 CLI 命令 | `crates/iota-cli/src/cli/mod.rs` 和独立 handler |
| 新 TUI 组件 | `crates/iota-cli/src/tui/*`，状态和渲染下沉到子模块 |
| 新 RuntimeEvent | `runtime_event/` 和相关 producer/consumer |
| 新 MCP 工具 | `mcp/tool_dispatch.rs`、`mcp/server.rs`、必要时 `mcp/router.rs` |
| 新 memory 能力 | `memory/` 和 `mcp/tool_dispatch.rs` |
| 新 desktop daemon 消息 | `daemon/proto.rs`、`daemon/desktop.rs`、`src-tauri/src/daemon_client.rs`、frontend reducer/types |
| 新 Kanban 行为 | `iota-sympantos-kanban` domain/store/state machine，CLI 和 desktop commands 按需接入 |
