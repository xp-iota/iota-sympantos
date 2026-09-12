# iota-sympantos 代码调用链

本文按入口和运行时边界梳理当前调用链，重点标注 IPC、子进程、网络和持久化边界。分层说明见 [architecture.md](architecture.md)。

## 入口总览

```text
crates/iota-cli/src/main.rs
  -> cli::run()

crates/iota-desktop/src-tauri/src/main.rs
  -> iota_desktop_lib::run()
```

## CLI 命令分发

```text
cli::run()
  -> telemetry::init()
  -> std::env::args().skip(1)
  -> match first arg:
       "run"                -> direct or daemon prompt
       "mcp context|fun"     -> MCP stdio server
       "context-mcp"         -> compatibility alias
       "fun-mcp"             -> compatibility alias
       "observability"       -> local token/metrics or Loki/Jaeger query
       "logs" / "trace"      -> top-level observability aliases
       "skill"               -> skill pull
       "kanban"              -> Kanban CLI
       "__daemon"            -> daemon::run_daemon()
       "__bench_cache"       -> internal cache benchmark
       "check"               -> optional daemon warm + combined JSON info
       "bench"               -> cold/warm benchmark
       "bench-cold"          -> compatibility command
       "bench-warm"          -> compatibility command
       no args               -> TUI
```

Global constraints:

- Config is read only from `~/.i6/nimia.yaml`.
- `iota run --daemon` cannot be combined with `--show-native`.
- `--log-events` prints normalized `RuntimeEvent`.
- `--timing` prints route and ACP timing JSON.

## 链路 1：CLI 直接运行

```text
iota run [backend] [options] <prompt>
  -> acp::parse_acp_args()
  -> config::read_config()
  -> run_cmd::run_direct()
  -> IotaEngine::create_session(config, show_native, timeout_ms, None)
       -> EffectiveConfig::from_config()
       -> ContextEngine::from_config()
       -> MemoryStore::open_with_embedding()
       -> CacheStore::open()
       -> SessionLedger::open()
  -> IotaEngine::run_with_timing(backend, cwd, prompt)
  -> print output text
  -> optional events/timing stderr
  -> IotaEngine::shutdown()
```

Engine prompt path:

```text
IotaEngine::run_with_timing()
  -> request_hash()
  -> SkillRegistry::load_cached()
  -> SkillRegistry::match_skill()
  -> ensure_session_ledger()
  -> prepare_handoff()
  -> CacheStore::begin_execution_with_id()
  -> record RuntimeEvent::State(started)
  -> extract_structured_memories()
  -> optional memory-write-only short circuit
  -> optional engine-run MCP skill short circuit
  -> MemoryStore::recall_buckets_with_thresholds()
  -> ContextEngine::compose_effective_prompt()
       -> render_workspace()
            -> child process: git status --short
  -> ensure_acp_client()
  -> AcpClient::execute()
  -> collect RuntimeEvent list
  -> ObservabilityStore::record_token_usage()
  -> CacheStore::finish_execution()
  -> SessionLedger::record_turn()
  -> WorkingMemoryBuffer::push_turn()
  -> MemoryStore::insert(episodic prompt/output memory)
```

External boundaries:

- ACP backend is a child process using stdin/stdout JSON-RPC 2.0.
- Workspace summary calls `git status --short`.
- Store writes go to SQLite files under `~/.i6/context`.

## 链路 2：ACP Client

Startup:

```text
IotaEngine::ensure_acp_client()
  -> effective_config.backend_config(backend)
  -> backend_process_env_with_context()
  -> normalized_acp_command()
  -> context_mcp_servers()
  -> context_session_options()
  -> context_tool_whitelist()
  -> AcpClient::start()
       -> TokioCommand::new(command)
            .args(args)
            .envs(env)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
       -> stderr reader task
       -> send_request("initialize")
       -> wait_for_response()
```

Prompt:

```text
AcpClient::execute()
  -> ensure_session_timed()
       -> session_new_params_with_options()
       -> send_request("session/new")
       -> wait_for_response()
  -> send_request("session/prompt")
  -> read_prompt_events_for_id()
       -> wire::read_next_line()
       -> wire::parse_message_line()
       -> response id match?
       -> method event?
            -> runtime_event::map_acp_events()
            -> stream output chunks to optional sender
       -> session/request_permission?
            -> acp::permission::answer_permission_request()
       -> tools/call style method?
            -> mcp::router::try_intercept_tool_call()
       -> stop on session/complete or prompt response
```

Protocol order:

```text
initialize
  -> session/new
  -> session/prompt
  -> session/update ...
  -> session/request_permission? ...
  -> session/complete
```

`session/request_permission` is optional and only occurs when the backend requests permission to invoke a tool. If no tool permission is needed during a turn, this step is skipped entirely.

## 链路 3：CLI 经 daemon

Client:

```text
iota run --daemon [backend] <prompt>
  -> daemon_cmd::run_prompt_via_daemon()
       -> daemon::auth::load_or_create_token()  # owner-only CSPRNG token
       -> DaemonPromptRequest { auth_token, ... }
  -> send_prompt_autostart_daemon()
       -> daemon::send_prompt(addr, DaemonPromptRequest)
            -> loopback TCP connect
            -> write one authenticated JSON line
            -> read one JSON line DaemonPromptResponse
       -> if connect failed:
            -> start_daemon_silently()
                 -> child process: current_exe __daemon
            -> wait_for_daemon()
            -> retry
```

Daemon:

```text
iota __daemon
  -> config::read_config()
  -> daemon::run_daemon(config, addr, DEFAULT_TIMEOUT_MS, warm_on_start=false)
       -> EnginePool::new()
       -> TcpListener::bind(addr)
       -> Semaphore::new(8)
       -> Ctrl+C CancellationToken
       -> accept loop
            -> handle_connection()
```

Legacy prompt connection:

```text
handle_connection()
  -> read one line, max 10 MiB
  -> if request.type == "warm":
       -> handle_warm()
     else:
       -> handle_prompt()
  -> write DaemonPromptResponse JSON line

handle_prompt()
  -> AcpBackend::parse()
  -> EnginePool::engine_for(cwd)
  -> IotaEngine::run()
```

Warm path:

```text
iota check --daemon / bench-* --daemon
  -> daemon_cmd::warm_daemon_for_current_dir()
  -> daemon::send_warm(DaemonWarmRequest)
  -> daemon::handle_warm()
       -> warm selected or all enabled backends
       -> IotaEngine::warm_backend()
       -> ensure_acp_client()
```

## 链路 4：TUI

Initialization:

```text
iota
  -> config::read_config()
  -> tui::run(config)
       -> stdout is_terminal check
       -> TuiApp::new()
            -> IotaEngine::create_session(...)
       -> CacheStore::open(default path)
       -> acp::permission::install_tui_approval_channel()
       -> panic hook
       -> raw mode
       -> mouse capture
       -> TerminalGuard cleanup
       -> loop::run_loop()
```

Event loop:

```text
loop::run_loop()
  -> crossterm EventStream
  -> frame tick limiter
  -> keyboard/mouse/resize events
  -> Composer::handle_key()
       -> submit/newline/history/search/word motion/kill/yank
  -> slash_command handling
  -> TuiApp::submit()
       -> enqueue or start prompt
  -> tokio::spawn(engine task)
       -> IotaEngine::set_stream_output_sender(Some(tx))
       -> IotaEngine::run_with_timing()
       -> send result to UI
  -> stream_rx receives output chunks
  -> approval_rx receives ApprovalRequest
  -> render()
       -> history/composer/status
       -> markdown::render()
       -> overlays: help / pager / quit confirm / approval
```

TUI does not use daemon. It owns an in-process `IotaEngine`.

## 链路 5：Desktop

Frontend:

```text
ChatWorkbench
  -> getConfig()
  -> currentWorkspace()
  -> refreshBackendChecks()
  -> getObservabilitySummary()
  -> listenDaemonMessages()
  -> listenDaemonClientErrors()
  -> render Chat / Config central view
  -> render RightInspector tabs: Observability / Kanban / Memory / Context
```

Prompt:

```text
User submits prompt
  -> api.submitPrompt()
  -> Tauri command submit_prompt()
  -> daemon_client::start_turn(window, turn_id, cwd, backend, prompt)
       -> connect_or_start()
            -> connect_and_handshake(primary daemon addr)
                 -> send Hello { protocol version, auth_token }
                 -> daemon validates the owner-only CSPRNG token
            -> fallback IOTA_DESKTOP_DAEMON_ADDR or 127.0.0.1:47662
            -> autostart_daemon(fallback)
       -> write DaemonClientMessage::StartTurn
       -> spawn reader task
            -> parse DaemonServerMessage lines
            -> emit "daemon-message"
            -> emit "daemon-client-error" on stream failure
  -> turnsReducer handles messages
```

Daemon desktop protocol:

```text
DaemonClientMessage (each request carries optional request_id; Hello also
carries schema_version + capabilities):
  Hello { protocol_version, min_version, max_version, schema_version, capabilities, auth_token }
  StartTurn
  RespondApproval
  CancelTurn
  GetConfig
  SaveBackendModel
  CheckBackend
  GetObservabilitySummary
  GetMemoryContextSnapshot

DaemonServerMessage:
  HelloAccepted
  ProtocolError
  TurnStarted
  TextChunk
  TurnEvent
  ApprovalRequested
  ApprovalResponded
  TurnCompleted
  TurnFailed
  TurnCancelled
  ConfigSnapshot
  BackendCheckResult
  ObservabilitySummary
  MemoryContextSnapshot
```

Config:

```text
ConfigPanel save
  -> save_backend_model()
  -> DaemonClientMessage::SaveBackendModel
  -> daemon updates NimiaConfig
  -> ConfigSnapshot with masked DesktopModelConfig
```

Memory/context:

```text
RightInspector tab Memory or Context
  -> MemoryContextWorkspace(mode)
  -> get_memory_context_snapshot(scope_mode)
  -> DaemonClientMessage::GetMemoryContextSnapshot
  -> DesktopMemoryContextSnapshot
       -> memory buckets
       -> runtime context preview
       -> context budgets
       -> snapshot errors
```

KanbanWorkspace is mounted in the RightInspector Kanban tab. It uses Tauri commands in `src-tauri/src/lib.rs` and `SqliteKanbanStore` under `~/.i6/kanban/iota.db` to refresh boards, tasks, comments, links, and runs.

## 链路 6：Context Fabric

```text
IotaEngine::run()
  -> MemoryStore::recall_buckets_with_thresholds()
       -> identity
       -> preference
       -> strategic
       -> domain
       -> procedural
       -> episodic
  -> WorkingMemoryBuffer::render()
  -> prepare_handoff()
  -> ContextEngine::compose_effective_prompt()
       -> <iota-context>
            <session>
            <memory-tools>
            <model>
            <memory>
            <working-memory>
            <workspace>
            <skills>
            <handoff>
          </iota-context>
       -> original prompt
```

Disabled path:

```text
context_engine.enabled = false
or context_engine.injection = off
  -> compose_effective_prompt() returns original prompt
```

## 链路 7：Memory

Automatic write:

```text
completed output
  -> persist_turn_as_episodic_memory()
  -> MemoryStore::insert()
       -> insert_with_merge(..., Auto)
       -> validate taxonomy
       -> dedup by scope/scope_id/type/facet/content_hash
       -> upsert_embedding()
```

LLM tool write:

```text
backend calls iota_memory_write
  -> session/request_permission
  -> auto approve if iota tool or whitelist hit
  -> backend calls MCP sidecar tool
  -> mcp::server or mcp::router
  -> tool_dispatch::dispatch_tool()
  -> MemoryStore::insert_with_merge()
```

Search:

```text
iota_memory_search { query, limit, mode }
  -> MemoryStore::search_with_mode()
       keyword: FTS5 or LIKE
       vector: Ollama /api/embeddings or local trigram fallback
       hybrid: merge keyword and vector ranking
```

## 链路 8：Engine-run MCP Skill

```text
SkillRegistry::load_cached()
  -> workspace skills/
  -> workspace .iota/skills
  -> configured skill_roots
  -> ~/.i6/skills
  -> parse frontmatter
  -> match_skill(backend, prompt)

matched skill with execution.mode = "mcp"
  -> skill::runner::run_engine_skill()
       -> server_command()
            iota-fun     -> current_exe fun-mcp
            iota-context -> current_exe context-mcp
            custom       -> configured command
       -> mcp::client::call_stdio()
            -> child process + stdio JSON-RPC
            -> initialize
            -> tools/call
       -> render_template()
  -> engine records ToolCall/ToolResult/Output
  -> ACP backend prompt is skipped
```

## 链路 9：MCP Sidecars

iota-context:

```text
iota mcp context / iota context-mcp
  -> mcp::server::run_stdio()
  -> MemoryStore::open()
  -> SkillRegistry::load()
  -> SessionLedger::open()
  -> stdin JSON-RPC loop
       initialize
       tools/list
       tools/call
       resources/list
       resources/read
```

Tools:

```text
iota_memory_search
iota_memory_write
iota_skill_search
iota_skill_load
iota_session_summary
iota_handoff_publish
iota_handoff_read
```

iota-fun:

```text
iota mcp fun / iota fun-mcp
  -> skill::fun::run_stdio()
  -> tools/list
  -> tools/call
       fun.python
       fun.typescript
       fun.rust
       fun.go
       fun.java
       fun.cpp
       fun.zig
```

Function tools call interpreters, compilers, or compiled binaries as child processes. Compile cache is under `~/.i6/fun-cache`.

## 链路 10：Permission And Router

Permission:

```text
ACP backend -> session/request_permission
  -> runtime_event::map_acp_events()
  -> permission::answer_permission_request()
       -> extract tool name
       -> auto approve iota tools or backend whitelist
       -> else TUI approval channel
       -> else stdin yes/no
       -> ApprovalStore request/decision
       -> send ACP JSON-RPC response
```

Router:

```text
mcp::router::try_intercept_tool_call(method, params)
  -> handles tools/call, mcp/tools/call, mcp/tool_call
  -> route_tool_call(name, arguments)
       -> iota tools
       -> fun.* tools
       -> unknown iota_* returns routable error
       -> external unknown denied by iota policy
```

## 链路 11：Kanban

CLI:

```text
iota kanban <subcommand>
  -> kanban_cmd::run_kanban_command()
  -> SqliteKanbanStore::open(~/.i6/kanban/iota.db)
  -> AdvancedBridge::new("hermes", ~/.i6/kanban/shadows)
  -> execute_kanban_command()
```

Store:

```text
create_board / create_task / transition / comment / link / run
  -> SqliteKanbanStore
  -> append event
  -> apply_event projection
```

Dispatch:

```text
iota kanban dispatch <id>
  -> Dispatcher
  -> WorkerHandle
  -> child process: hermes -z
  -> poll until done/blocked/timeout
```

Event sync:

```text
export/import
serve-sync
pull
push
  -> event bundle
  -> cursor-based sync
```

## 链路 12：Observability

Token usage:

```text
RuntimeEvent::TokenUsage
  -> engine::telemetry::record_runtime_event()
  -> ObservabilityStore::record_token_usage()
  -> token_usage_events
```

Queries:

```text
iota observability tokens recent
  -> recent_token_executions()

iota observability tokens summary --since 1h
  -> token_summary_since()

iota observability metrics --prometheus
  -> token_summary_since(None)
  -> Prometheus text

iota logs <execution_id>
  -> Loki HTTP API

iota trace <trace_id>
  -> Jaeger HTTP API
```

## 外部调用清单

| 位置 | 类型 | 发起方 | 目标 | 用途 |
| :--- | :--- | :--- | :--- | :--- |
| `start_daemon_silently()` | child process | CLI | `iota __daemon` | daemon autostart |
| `daemon::send_prompt()` | TCP | CLI | daemon | JSON-line prompt |
| `daemon_client::connect_or_start()` | TCP/child process | desktop | daemon / `iota __daemon` | desktop streaming protocol |
| `AcpClient::start()` | child process + stdio | engine | ACP backend | ACP JSON-RPC |
| `permission::send_response()` | stdio | engine | ACP backend | permission decision |
| `session_new_params_with_options()` | delegated child process | ACP backend | MCP servers | backend starts sidecars |
| `mcp::client::call_stdio()` | child process + stdio | skill runner | MCP server | engine-run skill |
| `mcp::server::run_stdio()` | stdio server | backend/skill | iota-context | MCP tools/resources |
| `skill::fun::run_stdio()` | stdio server | backend/skill | iota-fun | function tools |
| `skill::fun` runners | child process | iota-fun | language runtimes | execute snippets |
| `context::render_workspace()` | child process | context engine | `git` | workspace summary |
| `skill::cache::pull_skill()` | network/filesystem | CLI | HTTPS/local path | size- and timeout-bounded skill install with optional SHA-256 verification |
| `EmbeddingEngine::embed_api()` | network | memory store | Ollama-compatible API | embeddings |
| SQLite stores | filesystem | engine/MCP/CLI/desktop | `~/.i6/**/*.sqlite` | persistence |
