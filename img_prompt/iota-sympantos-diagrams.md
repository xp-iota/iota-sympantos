# iota-sympantos 系统架构与设计图表集 (AI 生图提示词)

本文档包含 iota-sympantos 项目的完整架构图表生成提示词，涵盖系统架构、运行时流程、观测性和调试四个维度。

---

## 目录

1. [系统架构总览](#1-系统架构总览)
   - 1.1 [分层架构与组件依赖](#11-分层架构与组件依赖)
   - 1.2 [完整运行时架构图](#12-完整运行时架构图)
   - 1.3 [架构总览海报](#13-架构总览海报)
2. [运行时执行流程](#2-运行时执行流程)
   - 2.1 [Prompt 执行时序](#21-prompt-执行时序)
   - 2.2 [代码调用链路](#22-代码调用链路)
   - 2.3 [Backend 调用与 IPC](#23-backend-调用与-ipc)
3. [Context Fabric 与记忆系统](#3-context-fabric-与记忆系统)
   - 3.1 [记忆分类与生命周期](#31-记忆分类与生命周期)
   - 3.2 [六桶记忆召回机制](#32-六桶记忆召回机制)
4. [观测性与调试](#4-观测性与调试)
   - 4.1 [OpenTelemetry 观测性架构](#41-opentelemetry-观测性架构)
   - 4.2 [调试工作流](#42-调试工作流)
5. [Kanban 任务编排系统](#5-kanban-任务编排系统)
   - 5.1 [Kanban 状态机与事件溯源](#51-kanban-状态机与事件溯源)
   - 5.2 [Kanban 分布式同步与桥接](#52-kanban-分布式同步与桥接)
6. [Desktop (Tauri) 架构](#6-desktop-tauri-架构)
   - 6.1 [Desktop 应用架构与通信流](#61-desktop-应用架构与通信流)
7. [配置与环境变量映射](#7-配置与环境变量映射)
   - 7.1 [配置层次与后端环境变量映射](#71-配置层次与后端环境变量映射)
8. [技能系统 (Skill System)](#8-技能系统-skill-system)
   - 8.1 [技能加载、匹配与执行](#81-技能加载匹配与执行)

---

## 1. 系统架构总览

### 1.1 分层架构与组件依赖

**用途**: 展示 iota-sympantos 的四层架构和组件间的依赖关系

![分层架构与组件依赖](../img_result/layered_architecture.png)

**Prompt**:
```
Selected GPT-Image2 style-library template: Infographic Engine (ID: infographic-engine; category: Charts & Infographics; example cases: case 334, case 1, case 8).
Use case: infographic-diagram. Asset type: technical architecture diagram for project documentation. Output settings: high outputQuality, wide landscape 16:9 PNG, readable labels, clean layout.
Use a consistent technical infographic style for this entire wide landscape 16:9 image.
Use the shared visual system: clean technical infographic, warm off-white paper background, precise thin ink lines, subtle hand-drawn engineering paper texture, restrained iota magenta accent, muted navy / forest green / terracotta / cyan / teal / gray module colors, readable labels, clear arrows, generous whitespace, no 3D, no neon, no stock cloud icons, no decorative blobs.
Palette: iota magenta #C026D3; deep navy #1E3A5F; forest green #2F6B4F; terracotta orange #C46A3A; protocol cyan #0E7490; backend teal #0F766E; neutral gray #52525B; paper background #F8F5EE.
Technical diagram rules: warm off-white paper, thin ink vector lines, rounded module rectangles, solid arrows, compact labels, clean sans-serif typography, disciplined spacing; keep labels short and readable; do not invent file paths, module names, database columns, commands, or backend names.
Negative details: unreadable tiny text, fake modules, fake code, wrong file paths, obsolete modules, crowded arrows, messy layout, 3D render, neon cyberpunk, glossy corporate dashboard, stock cloud architecture icons, gradient blobs, random decorative symbols, Korean text, non-Chinese non-English labels.
Create a layered architecture diagram titled "iota-sympantos Component Layers and Dependencies".
The diagram is divided vertically into four distinct boxes representing layers with strict dependency flow:

1. Layer 1: "Presentation (cli & tui)" - outlined in deep navy blue
   - Blocks: "crates/iota-cli/src/main.rs", "cli/mod.rs", "tui/mod.rs (render/loop/input/state/status_bar/theme)"

2. Layer 2: "Orchestration & Kanban" - outlined in muted forest green
   - Large block: "IotaEngine (crates/iota-core/src/engine/mod.rs)"
   - Block: "daemon/mod.rs & pool.rs (EnginePool) & auth.rs (CSPRNG token / UDS peer credentials) & audit.rs"
   - Block: "runtime_event/mod.rs (RuntimeEvent normalization)"
   - Block: "runtime_snapshot (RuntimeContextSnapshot, ContextBudgetsSnapshot)"
   - Block: "crates/iota-kanban/src/ (dispatcher, worker, shadow system, bridge)"

3. Layer 3: "Protocol & Tools" - outlined in terracotta orange
   - Blocks: "AcpClient (crates/iota-core/src/acp/mod.rs)", "acp::permission", "acp::session", "acp::wire", "acp::stream_reader"
   - Blocks: "mcp::router", "mcp::client", "mcp::server", "mcp::tool_dispatch"
   - Blocks: "context::ContextEngine", "skill::SkillRegistry", "skill::runner (execution)"
   - Blocks: "ipc_client (autostart, backoff, version negotiation)", "fs_secure (atomic write, constant-time compare)"

4. Layer 4: "External Boundaries" - outlined in dark charcoal gray
   - Block: "Backend Subprocesses" (Claude Code, Codex, Gemini CLI, Hermes, OpenCode)
   - Block: "MCP Sidecars" (iota-context, iota-fun)
   - Block: "SQLite Stores" (events.sqlite, memory.sqlite, sessions.sqlite, approvals.sqlite in ~/.i6/context/; iota.db in ~/.i6/kanban/)

Connections and Flows:
- Purple solid arrows from "IotaEngine" and "Kanban system" to "SQLite Stores" labeled "SQL I/O"
- Terracotta arrow from "IotaEngine" to "AcpClient" labeled "injects EffectiveConfig"
- Terracotta arrow from "AcpClient" to "mcp::router" labeled "delegates tool_call filter"
- Dual connection between "tui" and "IotaEngine":
  - Blue arrow labeled "mpsc (streams output chunks)"
  - Red arrow labeled "oneshot (TUI approval decision)"
- Dark gray socket line between "cli/mod.rs" and "daemon" labeled "TCP 127.0.0.1:47661 + CSPRNG token auth"
- Blue pipe lines between "AcpClient" and "Backend Subprocesses" labeled "Stdio (stdin/stdout/stderr)"

Style instructions:
- Follow the technical diagram rules and palette stated at the top of this prompt
- Use compact, readable module labels and consistent font size within each layer
- Use continuous solid lines with no overlapping and generous white borders
- Do not invent module names, file paths, stores, backend names, or commands
```

---

### 1.2 完整运行时架构图

**用途**: 展示完整的运行时架构，包含所有模块、数据流和序列标记（双语版本）

![完整运行时架构图](../img_result/runtime_architecture.png)

**Prompt**:
```
Selected GPT-Image2 style-library template: Infographic Engine (ID: infographic-engine; category: Charts & Infographics; example cases: case 334, case 1, case 8).
Use case: infographic-diagram. Asset type: technical architecture diagram for project documentation. Output settings: high outputQuality, wide landscape 16:9 PNG, readable labels, clean layout.
Use a consistent technical infographic style for this entire wide landscape 16:9 image.
Use the shared visual system: clean technical infographic, warm off-white paper background, precise thin ink lines, subtle hand-drawn engineering paper texture, restrained iota magenta accent, muted navy / forest green / terracotta / cyan / teal / gray module colors, readable labels, clear arrows, generous whitespace, no 3D, no neon, no stock cloud icons, no decorative blobs.
Palette: iota magenta #C026D3; deep navy #1E3A5F; forest green #2F6B4F; terracotta orange #C46A3A; protocol cyan #0E7490; backend teal #0F766E; neutral gray #52525B; paper background #F8F5EE.
Technical diagram rules: warm off-white paper, thin ink vector lines, rounded module rectangles, solid arrows, compact labels, clean sans-serif typography, disciplined spacing; keep labels short and readable; do not invent file paths, module names, database columns, commands, or backend names.
Negative details: unreadable tiny text, fake modules, fake code, wrong file paths, obsolete modules, crowded arrows, messy layout, 3D render, neon cyberpunk, glossy corporate dashboard, stock cloud architecture icons, gradient blobs, random decorative symbols, Korean text, non-Chinese non-English labels.
Create a wide landscape technical architecture infographic titled "iota-sympantos Runtime Architecture / 运行时架构".
Use a precise module map, color-coded flow arrows, compact bilingual labels, and thin ink vector lines.

Canvas: wide landscape, 16:9 ratio, warm off-white paper background, rounded module columns, precise grid alignment.

Top legend:
- Pink: TUI / Presentation
- Orange: Daemon
- Blue: Engine
- Green: Context / Memory
- Cyan: ACP
- Teal: Backend
- Purple: Skill / MCP / Fn
- Gray: Store / Telemetry

Sequence suffixes: T=TUI, C=CLI, D=Daemon, M=Memory, A=ACP, B=Backend, K=Skill, F=Fn Runner, S=Store, O=Observability

Main layout: 8 vertical columns + wide bottom store/telemetry band

Column 1: Entry / CLI / TUI
Files: crates/iota-cli/src/main.rs, crates/iota-cli/src/cli/mod.rs, crates/iota-cli/src/tui/ (mod.rs, render.rs, loop.rs, input.rs, state.rs, status_bar.rs, theme.rs)
Show:
- User input enters CLI prompt or TUI input area
- main.rs -> cli::run()
- Default no-args path enters TUI
- Commands: iota run/check/bench/logs/trace/observability/kanban/mcp/skill
- TUI background engine task, streaming output, approval overlay, pager/help/quit overlays, prompt queue while engine running

Column 2: Daemon TCP Plane
Files: crates/iota-core/src/daemon/mod.rs, crates/iota-core/src/daemon/pool.rs, crates/iota-core/src/daemon/proto.rs, crates/iota-core/src/daemon/auth.rs, crates/iota-core/src/daemon/audit.rs, crates/iota-core/src/daemon/desktop.rs, crates/iota-core/src/ipc_client/ (autostart.rs, backoff.rs, version.rs), crates/iota-core/src/fs_secure.rs
Show:
- iota run --daemon
- Local TCP daemon at 127.0.0.1:47661 (overridable by IOTA_DAEMON_ADDR)
- Daemon auto-start through current_exe __daemon
- JSON line request/response
- Per-user CSPRNG-generated token authentication (32-byte hex, constant-time compare)
- Unix domain socket + SO_PEERCRED / LOCAL_PEERCRED UID verification (preferred)
- Structured daemon audit logging for auth failures and sensitive operations
- Loopback-only bind guard (IOTA_DAEMON_ALLOW_NON_LOOPBACK=1 to override)
- EnginePool reuses IotaEngine per cwd
- 8 connection concurrency limit, 10 MiB request cap
- Protocol version negotiation (PROTOCOL_VERSION_MIN / MAX)
- ipc_client: generic sidecar autostart, exponential-backoff-with-jitter reconnect, heartbeat
- fs_secure: atomic_write_secure, ensure_dir_owner_only, generate_csprng_token_hex
- Graceful Ctrl+C shutdown

Column 3: Engine Core
Files: crates/iota-core/src/engine/mod.rs, crates/iota-core/src/engine/prompt.rs, crates/iota-core/src/engine/memory_ops.rs, crates/iota-core/src/engine/telemetry.rs, crates/iota-core/src/runtime_event/mod.rs, crates/iota-core/src/runtime_snapshot/mod.rs, crates/iota-core/src/resources/mod.rs
Show:
- IotaEngine
- ACP client pool keyed by (backend, cwd) with dead-process eviction
- Concurrency fencing/locking via cache_executions table UNIQUE index
- Ephemeral sessions (create_ephemeral_session) with all durable stores disabled
- Session ledger and handoff with resume_session_state
- Memory extraction / deterministic memory answer
- Skill match and optional engine-run MCP skill
- Memory recall, context capsule composition
- ACP invocation with warm_all_enabled_backends concurrent startup
- OTel metrics/logs/spans
- Normalized RuntimeEvent stream
- RuntimeContextSnapshot with ContextBudgetsSnapshot and ContextSection parsing
- LocalResources for host-supplied skill roots

Column 4: Context Fabric & Memory Store
Files: crates/iota-core/src/context/mod.rs, crates/iota-core/src/memory/store.rs, crates/iota-core/src/memory/embedding.rs
Show:
- ContextEngine
- <iota-context> capsule, DialogueBuffer
- Workspace summary from git status --short
- Memory tools prompt, skill index, handoff
- Recall buckets
- Six memory taxonomy buckets: identity, preference, strategic, domain, procedural, episodic
- iota-context MCP stdio sidecar
- Memory search/write, skill search/load, session summary, handoff publish/read
- Resources, vector/hybrid search
- Ollama embeddings if configured, fallback 128-dimension local trigram embedding

Column 5: ACP Adapter
Files: crates/iota-core/src/acp/mod.rs, crates/iota-core/src/acp/wire.rs, crates/iota-core/src/acp/session.rs, crates/iota-core/src/acp/permission.rs, crates/iota-core/src/acp/stream_reader.rs, crates/iota-core/src/acp/client.rs
Show:
- AcpClient owns backend child process stdin/stdout
- JSON-RPC 2.0 newline-delimited protocol
- initialize, session/new, session/prompt, streaming session/update, session/request_permission, session/complete, session/cancel
- CancellationToken for cooperative turn cancellation (sends session/cancel to backend)
- PromptProgress tracking: line_count, update_count, tool_call_count, first_update_ms, first_tool_call_ms
- Session id reuse, mcpServers rendering
- Supports empty mcpServers, string_array and object env shapes
- Permission handling: auto-approve iota_*, mcp__iota-*, backend whitelist hits; otherwise route to TUI or stdin
- ACP-side MCP tool call intercept through router

Column 6: Backend Processes
Show five backend rows:
- Claude Code: command npx, aliases claude/claudecode
  Env: ANTHROPIC_API_KEY, ANTHROPIC_AUTH_TOKEN, ANTHROPIC_BASE_URL, ANTHROPIC_MODEL
- Codex: command npx, alias codex
  Env: OPENAI_API_KEY, ROUTER_API_KEY, OPENAI_BASE_URL, OPENAI_MODEL
- Gemini CLI: command npx, aliases gemini/gemini-cli
  Env: GEMINI_API_KEY, GEMINI_MODEL
- Hermes Agent: command hermes acp, alias hermes
  Env: HERMES_INFERENCE_PROVIDER, HERMES_MODEL, provider-native key and base URL variables
  Note: Do not show HERMES_HOME override. Hermes keeps its own default home.
- OpenCode: command npx, aliases opencode/open-code
  Env: OPENCODE_MODEL

Column 7: Skill & MCP System
Files: crates/iota-core/src/skill/mod.rs, crates/iota-core/src/skill/runner.rs, crates/iota-core/src/skill/cache.rs, crates/iota-core/src/skill/fun.rs, crates/iota-core/src/mcp/mod.rs, crates/iota-core/src/mcp/client.rs, crates/iota-core/src/mcp/router.rs
Show:
- SkillRegistry load roots: workspace skills/, workspace .iota/skills, configured skill roots, ~/.i6/skills
- Frontmatter parsing, trigger matching, backend compatibility
- SkillRunner, execution.mode = mcp, sequential or parallel MCP tool calls, simple placeholder rendering
- MCP client, ACP-side MCP router
- Intercept methods: tools/call, mcp/tools/call, mcp/tool_call
- Route iota memory/skill/session/handoff/kanban tools (kanban behind feature flag), reject external tools
- tool_dispatch submodules: memory (search/write), skill (search/load), session (summary), handoff (publish/read), kanban (create_task/list_tasks/ready_task)
- iota-fun MCP stdio server
- Seven Fn runners: Python, TypeScript, Rust, Go, Java, C++, Zig

Column 8: Kanban sub-system
Files: crates/iota-kanban/src/ (sqlite_store.rs, dispatcher.rs, worker.rs, shadow.rs, bridge.rs, event_sync.rs)
Show:
- SQLite event-sourced store (~/.i6/kanban/iota.db)
- Dispatcher polling ready tasks (default 30s)
- WorkerHandle spawning hermes -z child processes
- ShadowMaterializer creating temporary shadow directories with task.md and skills
- ShadowWatcher recycling shadow and updating task status to done
- AdvancedBridge decomposing and specifying tasks
- EventSyncManager pulling/pushing event syncs via serve-sync/pull/push

Bottom wide band: Store / Telemetry / Observability
Files: crates/iota-core/src/store/mod.rs, crates/iota-core/src/store/cache.rs, crates/iota-core/src/store/approvals.rs, crates/iota-core/src/store/ledger.rs, crates/iota-core/src/store/observability.rs, crates/iota-core/src/telemetry/mod.rs, crates/iota-core/src/telemetry/metrics.rs, crates/iota-core/src/telemetry/stderr.rs

Show store blocks:
- CacheStore: path ~/.i6/context/events.sqlite, concurrency fencing/locking only (fencing_token, status, request_hash), 30-day completed/failed retention
- MemoryStore: path ~/.i6/context/memory.sqlite (may be overridden by context_engine.memory_db), taxonomy (6 buckets), dedup, TTL, FTS5/LIKE, vector/hybrid search, memory_embedding table
- KanbanStore: path ~/.i6/kanban/iota.db, events table (event-sourcing), boards, tasks, runs, comments, links tables (materialized views)
- ApprovalStore: path ~/.i6/context/approvals.sqlite, request/decision recording, default risk classification
- SessionLedger: path ~/.i6/context/sessions.sqlite, iota session, backend session, turn, handoff
- Local logs: stderr tracing layer, daily files under ~/.i6/logs/, controlled by IOTA_LOG_FILE
- OpenTelemetry: default endpoint http://localhost:4317, OTEL_ENABLED=false disables export, traces/logs/metrics
- Docker observability stack: OTel Collector 4317/4318, Loki 3100, Jaeger 16686, Prometheus 9090, Grafana 3000

Flow arrows:
- Pink arrows from Entry/TUI to Engine
- Orange arrows from CLI daemon path to Daemon and then Engine
- Blue arrows through Engine core lifecycle
- Green arrows between Engine, ContextEngine, MemoryStore, and context MCP
- Cyan arrows between Engine and ACP Adapter
- Teal arrows between ACP Adapter and backend processes
- Purple arrows between Engine/ACP router and Skill/MCP/Fn runners
- Gray arrows from Engine and stores to Store/Telemetry bottom band

Sequence markers (use circled markers):
1C CLI entry, 1T TUI entry, 2C command dispatch, 3D daemon route, 4D daemon EnginePool, 5E engine request lifecycle, 6K skill registry load, 7M memory recall, 8C context capsule, 9A ensure ACP client, 10A initialize/session/new/session/prompt, 11B backend streaming update, 12A permission handling, 13K MCP/skill/fn tool route, 14S cache/memory/ledger writeback, 15O telemetry export, 16T TUI streaming render/approval overlay

Visual style:
- Follow the technical diagram rules and palette stated at the top of this prompt
- Wide landscape infographic, 16:9 ratio
- Warm off-white paper background, thin rounded rectangles, precise grid alignment
- Bilingual labels: Chinese first, English second, separated by /
- Keep labels readable and concise
- Use small icons only when they clarify meaning: terminal, database, gear, shield, book, network socket, telescope, chart
- The image must look like an updated version of a reference architecture diagram, not a new unrelated poster

Negative prompt:
Unreadable tiny text, random fake file paths, obsolete modules, src/store/events.rs, single context.db, Promtail, project-level config discovery, Hermes home override, unauthenticated daemon, excessive decorative art, messy arrows, 3D render, dark background, neon cyberpunk, stock cloud icons, blurry labels, incorrect backend names, Korean text, non-Chinese non-English labels
```

---

### 1.3 架构总览海报

**用途**: 以故事化的方式展示整体架构，适合用于文档封面或概览

![架构总览海报](../img_result/architecture_overview.png)

**Prompt**:
```
Selected GPT-Image2 style-library template: Poster Layout System (ID: poster-layout-system; category: Posters & Typography; example cases: case 345, case 5, case 10), with Infographic Engine constraints for structured technical labels.
Use case: productivity-visual and infographic-diagram. Asset type: technical story-board poster for project documentation. Output settings: high outputQuality, wide landscape 16:9 PNG, readable labels, clean layout.
Use a consistent technical story-board style for this entire wide landscape 16:9 image.
Use the shared visual system: clean technical infographic, warm off-white paper background, precise thin ink lines, subtle hand-drawn engineering paper texture, restrained iota magenta accent, muted navy / forest green / terracotta / cyan / teal / gray module colors, readable labels, clear arrows, generous whitespace, no 3D, no neon, no stock cloud icons, no decorative blobs.
Palette: iota magenta #C026D3; deep navy #1E3A5F; forest green #2F6B4F; terracotta orange #C46A3A; protocol cyan #0E7490; backend teal #0F766E; neutral gray #52525B; paper background #F8F5EE.
Story poster rules: pen-and-ink technical story poster, warm paper texture, precise black linework, light cross-hatching, miniature engineering cutaway details, restrained iota magenta highlights, readable labels, and a diagram-like composition.
Negative details: unreadable tiny text, fake modules, fake code, wrong file paths, obsolete modules, crowded arrows, messy layout, 3D render, neon cyberpunk, glossy corporate dashboard, stock cloud architecture icons, gradient blobs, random decorative symbols, Korean text, non-Chinese non-English labels.
Use a pen-and-ink technical story poster layout with a hand-drawn architectural cutaway, light cross-hatching, precise black ink, warm paper texture, and restrained iota magenta highlights.

Create a wide landscape story-board poster for the document "iota-sympantos architecture overview".

Scene: a compact Rust CLI/TUI control tower named "iota-sympantos" sits in the center like a tiny railway signal station. From the tower, five labeled rail lines run outward to five AI backend stations: Claude Code, Codex, Gemini CLI, Hermes, and OpenCode. Below the tower is a transparent underground cutaway showing Context Fabric, SQLite stores, ACP JSON-RPC pipes, MCP sidecars, telemetry instruments, and Kanban dispatcher/worker workshops as connected rooms. Small human engineers in simple work clothes carry context capsules, memory ledgers, skill scrolls, task cards, and approval stamps between rooms. The story should feel like a busy but orderly miniature city where every subsystem has a job.

Composition: wide landscape poster, 16:9 ratio. Strong left-to-right hierarchy: Entry on the left, Presentation and Service Orchestration in the center, Context/Protocol/Store/Runtime Events as connected chambers, External Boundaries on the right. Use arrows, pipes, labels, and little signs, but keep all text short and readable. Add the title "iota-sympantos Architecture" as hand-lettered text at the top.

Style: follow the story poster rules and palette stated at the top of this prompt. Keep the drawing precise, diagram-like, and legible; use subtle magenta only on the main tower beacon and active status rail. No photorealism, no 3D render, no glossy UI mockup, no gradient background.

Mood: curious, clever, organized, a little whimsical, showing complex orchestration as a navigable machine-city.

Negative prompt: unreadable tiny text, fake modules, fake code, wrong file paths, obsolete modules, crowded arrows, messy layout, 3D render, neon cyberpunk, glossy corporate dashboard, stock cloud architecture icons, gradient blobs, random decorative symbols, Korean text, non-Chinese non-English labels; also avoid blurry text, crowded random symbols, colored comic style, watercolor, oil paint, distorted terminals, broken arrows, and fake code blocks.
```

---

## 2. 运行时执行流程

### 2.1 Prompt 执行时序

**用途**: 展示从用户输入到输出返回的完整执行时序，包括快速路径和完整执行路径

![Prompt 执行时序](../img_result/execution_flowchart.png)

**Prompt**:
```
Selected GPT-Image2 style-library template: Infographic Engine (ID: infographic-engine; category: Charts & Infographics; example cases: case 334, case 1, case 8).
Use case: infographic-diagram. Asset type: technical architecture diagram for project documentation. Output settings: high outputQuality, wide landscape 16:9 PNG, readable labels, clean layout.
Use a consistent technical infographic style for this entire wide landscape 16:9 image.
Use the shared visual system: clean technical infographic, warm off-white paper background, precise thin ink lines, subtle hand-drawn engineering paper texture, restrained iota magenta accent, muted navy / forest green / terracotta / cyan / teal / gray module colors, readable labels, clear arrows, generous whitespace, no 3D, no neon, no stock cloud icons, no decorative blobs.
Palette: iota magenta #C026D3; deep navy #1E3A5F; forest green #2F6B4F; terracotta orange #C46A3A; protocol cyan #0E7490; backend teal #0F766E; neutral gray #52525B; paper background #F8F5EE.
Technical diagram rules: warm off-white paper, thin ink vector lines, rounded module rectangles, solid arrows, compact labels, clean sans-serif typography, disciplined spacing; keep labels short and readable; do not invent file paths, module names, database columns, commands, or backend names.
Negative details: unreadable tiny text, fake modules, fake code, wrong file paths, obsolete modules, crowded arrows, messy layout, 3D render, neon cyberpunk, glossy corporate dashboard, stock cloud architecture icons, gradient blobs, random decorative symbols, Korean text, non-Chinese non-English labels.
Create a technical execution flowchart representing the core prompt sequence of the "IotaEngine" with a left-to-right two-column layout.

Left Column: "Initialization & Concurrency Locking" (outlined in navy blue)
- Start Node: "TuiApp / CLI" triggers submit(prompt) and calls IotaEngine::run_with_timing()
- "Ephemeral Check" block (gray): if engine is_ephemeral() (create_ephemeral_session), skip all durable stores and concurrency dedup
- "request_hash" block (forest green): calculates SHA256 of backend + null-byte + cwd + null-byte + prompt as concurrency & fencing key
- Decision Diamond (terracotta orange): "Is matched skill execution mode == MCP?"
  - If YES: route (orange arrow) to skill::runner::run_engine_skill() which spawns stdio MCP server (iota-fun/iota-context), bypasses ACP backend and exit
  - If NO: proceed
- Decision Diamond (terracotta orange): "Is there a running execution with the same request_hash (UNIQUE constraint)?"
  - If YES: begin_execution_with_id() fails or updates status (fencing check), abort duplicate parallel execution
  - If NO: proceed to CacheStore::begin_execution_with_id() to allocate a fencing token, insert record with status 'running', and start execution

Right Column: "Execution & Completion" (outlined in forest green)
- "Session Ledger Setup" block (purple): calls SessionLedger::ensure_session() and preparse handoff text if backend switching occurs
- "Recall" block (purple): calls MemoryStore::recall_buckets_with_thresholds() concurrently with workspace git summary rendering
- "Compose Prompt" block (forest green): calls ContextEngine::compose_effective_prompt() to assemble the XML <iota-context> capsule with recalled memory, workspace state, and skill indexes (budget-aware)
- "AcpClient" block (terracotta): calls ensure_acp_client(). If CWD changed, sends session/new (injecting context and fun mcpServers list). Then sends session/prompt request to backend stdin
- "Read Loop" block (terracotta): polls stdout line-by-line using BufReader::lines():
  - If session/update -> stream text chunks to TUI mpsc channel (blue line)
  - If session/request_permission -> check tool_whitelist or request confirmation in TUI overlay (red line)
  - If tools/call -> call mcp::router::try_intercept_tool_call() to execute memory search/write locally (purple line), then write JSON-RPC result back to stdin
  - If session/complete -> exit loop
  - If CancellationToken fired -> send session/cancel to backend stdin, return TurnCancelled
- End Node: calls MemoryStore::insert(episodic), writes CacheStore::finish_execution() setting status to completed/failed, records ledger turn, and returns prompt output

Connections:
- Draw horizontal colored arrows from the Left Column decision paths to the Right Column execution blocks

Style instructions:
- Follow the technical diagram rules and palette stated at the top of this prompt
- Use compact labels, clean sans-serif type, and consistent font size within each hierarchy level
- Use continuous solid lines with no overlapping or broken strokes
- Do not add extra fast paths, cache semantics, or backend calls not listed above
```

---

### 2.2 代码调用链路

**用途**: 展示从入口点到运行时边界的完整代码调用链，包括直接路径和 Daemon 路径

![代码调用链路](../img_result/code_call_chains.png)

**Prompt**:
```
Selected GPT-Image2 style-library template: Poster Layout System (ID: poster-layout-system; category: Posters & Typography; example cases: case 345, case 5, case 10), with Infographic Engine constraints for structured technical labels.
Use case: productivity-visual and infographic-diagram. Asset type: technical story-board poster for project documentation. Output settings: high outputQuality, wide landscape 16:9 PNG, readable labels, clean layout.
Use a consistent technical story-board style for this entire wide landscape 16:9 image.
Use the shared visual system: clean technical infographic, warm off-white paper background, precise thin ink lines, subtle hand-drawn engineering paper texture, restrained iota magenta accent, muted navy / forest green / terracotta / cyan / teal / gray module colors, readable labels, clear arrows, generous whitespace, no 3D, no neon, no stock cloud icons, no decorative blobs.
Palette: iota magenta #C026D3; deep navy #1E3A5F; forest green #2F6B4F; terracotta orange #C46A3A; protocol cyan #0E7490; backend teal #0F766E; neutral gray #52525B; paper background #F8F5EE.
Story poster rules: pen-and-ink technical story poster, warm paper texture, precise black linework, light cross-hatching, miniature engineering cutaway details, restrained iota magenta highlights, readable labels, and a diagram-like composition.
Negative details: unreadable tiny text, fake modules, fake code, wrong file paths, obsolete modules, crowded arrows, messy layout, 3D render, neon cyberpunk, glossy corporate dashboard, stock cloud architecture icons, gradient blobs, random decorative symbols, Korean text, non-Chinese non-English labels.
Use a pen-and-ink sequential flow poster with hand-lettered annotations, light cross-hatching, mechanical process details, warm paper texture, and restrained iota magenta highlights.

Create a wide landscape story-board poster for the document "iota-sympantos code call chains".

Scene: depict a journey of one prompt as a small sealed message capsule traveling through a mechanical dispatch system. It begins at crates/iota-cli/src/main.rs, enters cli::run(), passes command switches for run, tui, check, bench, logs, trace, observability, kanban, mcp, and __daemon, then splits into two illustrated paths: the direct ACP route and the daemon TCP route.

Direct route moves through:
- Parsing, configuration
- Engine orchestration (crates/iota-core/src/engine/mod.rs)
- Cache lock check (UNIQUE constraint on request_hash)
- Memory recall (MemoryStore::recall_buckets_with_thresholds)
- Skill matching (SkillRegistry)
- Context capsule assembly (ContextEngine::compose_effective_prompt)
- ACP client creation (AcpClient::ensure_acp_client)
- Streaming updates (session/update)
- Final output

Daemon route shows:
- Local TCP gate at 127.0.0.1:47661
- Warm engine pooling (EnginePool keyed by cwd)
- Response returning to the user

Composition: wide landscape poster, 16:9 ratio.
Arrange the call chain as a large board-game-like path with numbered stations and arrows.
Put "initialize -> session/new -> session/prompt -> session/update -> session/complete" as a clear ribbon across the middle.
Show external boundaries as illustrated gates: git subprocess, ACP child process, MCP stdio sidecar, SQLite files, and TCP socket.
Add the title "Code Call Chains" at the top and a small subtitle "from entry point to runtime boundary".

Style: follow the story poster rules and palette stated at the top of this prompt. Keep crisp contour lines, readable miniature labels, and light magenta accents only on the active message capsule and protocol ribbon. Keep the journey metaphor technically accurate and structured.

Mood: adventurous debugging map, a prompt crossing checkpoints and machines, clear enough to teach the runtime path at a glance.

Negative prompt: unreadable tiny text, fake modules, fake code, wrong file paths, obsolete modules, crowded arrows, messy layout, 3D render, neon cyberpunk, glossy corporate dashboard, stock cloud architecture icons, gradient blobs, random decorative symbols, Korean text, non-Chinese non-English labels; also avoid unreadable spaghetti arrows, random pseudo-code, fantasy map cliches, excessive colors, cluttered icons, and distorted terminal text.
```

---

### 2.3 Backend 调用与 IPC

**用途**: 展示 Backend 调用的三个阶段：预处理、IPC 调用、后处理

![Backend 调用与 IPC](../img_result/backend_ipc_stages.png)

**Prompt**:
```
Selected GPT-Image2 style-library template: Infographic Engine (ID: infographic-engine; category: Charts & Infographics; example cases: case 334, case 1, case 8).
Use case: infographic-diagram. Asset type: technical architecture diagram for project documentation. Output settings: high outputQuality, wide landscape 16:9 PNG, readable labels, clean layout.
Use a consistent technical infographic style for this entire wide landscape 16:9 image.
Use the shared visual system: clean technical infographic, warm off-white paper background, precise thin ink lines, subtle hand-drawn engineering paper texture, restrained iota magenta accent, muted navy / forest green / terracotta / cyan / teal / gray module colors, readable labels, clear arrows, generous whitespace, no 3D, no neon, no stock cloud icons, no decorative blobs.
Palette: iota magenta #C026D3; deep navy #1E3A5F; forest green #2F6B4F; terracotta orange #C46A3A; protocol cyan #0E7490; backend teal #0F766E; neutral gray #52525B; paper background #F8F5EE.
Technical diagram rules: warm off-white paper, thin ink vector lines, rounded module rectangles, solid arrows, compact labels, clean sans-serif typography, disciplined spacing; keep labels short and readable; do not invent file paths, module names, database columns, commands, or backend names.
Negative details: unreadable tiny text, fake modules, fake code, wrong file paths, obsolete modules, crowded arrows, messy layout, 3D render, neon cyberpunk, glossy corporate dashboard, stock cloud architecture icons, gradient blobs, random decorative symbols, Korean text, non-Chinese non-English labels.
Create a technical column-based layout diagram titled "Backend Call Stages and IPC Interface".
The diagram is split into three vertical sections showing the exact execution and tool invocation pipelines:

Column 1: "Preprocessing Stage & Context Loading" (outlined in forest green)
- Labeled "Memory Recall Pipeline": Shows MemoryStore::recall_buckets_with_thresholds() reading memory.sqlite to load RecallBuckets
- Labeled "Skill Registry Pipeline": Shows SkillRegistry loading and matching skill triggers
- Labeled "Prompt Assembly": Shows ContextEngine::compose_effective_prompt() combining matched skills, recalled memory buckets, and workspace git status into the XML <iota-context> capsule, with character-level budget trimming
- Labeled "Telemetry Span": Shows global tracer initializing and binding execution context

Column 2: "Backend Call (IO/IPC) Stage" (outlined in terracotta orange)
- AcpClient spawning subprocess via tokio::process::Command with stdin/stdout/stderr piped and kill_on_drop(true)
- Labeled pipelines showing structured JSON-RPC messages written to child stdin and read from child stdout
- Labeled "Daemon IPC boundary" (purple block) showing TCP Socket at 127.0.0.1:47661, regulated by Semaphore(8) concurrency control and matched via EnginePool CWD lookup

Column 3: "Postprocessing Stage & Execution Routes" (outlined in charcoal gray)
- Labeled "MCP Interception Route": Shows mcp::router intercepting tools/call (memory search/write tools) to bypass backend and call local handlers
- Labeled "Memory Write Pipeline": Shows MemoryStore::insert(episodic) persisting session memories back to memory.sqlite
- Labeled "Skill Executor Route": Shows skill::runner::run_engine_skill() launching local script tools via mcp::client
- Labeled exception handler (red block) showing terminate_child_tree cleaning up residual process trees

Connections and Flow:
- Distinct colored arrows show trace pipelines (purple for memory data flow, orange for skill loading/execution, blue for MCP tool routing, red for process execution)

Style instructions:
- Follow the technical diagram rules and palette stated at the top of this prompt
- Use compact labels, clean sans-serif type, and consistent font size within each hierarchy level
- Use continuous solid lines with no overlapping or broken strokes
- Do not invent additional IPC protocols, tools, stores, or cleanup paths
```

---

## 3. Context Fabric 与记忆系统

### 3.1 记忆分类与生命周期

**用途**: 展示记忆系统的数据库模式、分类体系和生命周期管理

![记忆分类与生命周期](../img_result/memory_taxonomy_lifecycle.png)


**Prompt**:
```
Selected GPT-Image2 style-library template: Infographic Engine (ID: infographic-engine; category: Charts & Infographics; example cases: case 334, case 1, case 8).
Use case: infographic-diagram. Asset type: technical architecture diagram for project documentation. Output settings: high outputQuality, wide landscape 16:9 PNG, readable labels, clean layout.
Use a consistent technical infographic style for this entire wide landscape 16:9 image.
Use the shared visual system: clean technical infographic, warm off-white paper background, precise thin ink lines, subtle hand-drawn engineering paper texture, restrained iota magenta accent, muted navy / forest green / terracotta / cyan / teal / gray module colors, readable labels, clear arrows, generous whitespace, no 3D, no neon, no stock cloud icons, no decorative blobs.
Palette: iota magenta #C026D3; deep navy #1E3A5F; forest green #2F6B4F; terracotta orange #C46A3A; protocol cyan #0E7490; backend teal #0F766E; neutral gray #52525B; paper background #F8F5EE.
Technical diagram rules: warm off-white paper, thin ink vector lines, rounded module rectangles, solid arrows, compact labels, clean sans-serif typography, disciplined spacing; keep labels short and readable; do not invent file paths, module names, database columns, commands, or backend names.
Negative details: unreadable tiny text, fake modules, fake code, wrong file paths, obsolete modules, crowded arrows, messy layout, 3D render, neon cyberpunk, glossy corporate dashboard, stock cloud architecture icons, gradient blobs, random decorative symbols, Korean text, non-Chinese non-English labels.
Create a technical data-mapping and database schema diagram titled "Memory Taxonomy, Scopes and Lifecycles".
The diagram contains three interconnected schema blocks:

Left Block: "SQLite Table Schema & Visibility Candidates" (outlined in purple)
- Table "memory": columns (id [TEXT PRIMARY KEY], type [TEXT CHECK(type IN ('semantic','episodic','procedural'))], facet [TEXT CHECK(facet IN ('identity','preference','strategic','domain'))], scope [TEXT CHECK(scope IN ('session','project','user','global'))], scope_id [TEXT], content [TEXT], content_hash [TEXT], confidence [REAL], expires_at [INTEGER], created_at, updated_at, supersedes, source_backend, source_session_id, source_execution_id, metadata_json, ttl_days, owner, visibility)
- Table "memory_embedding": columns (memory_id [TEXT PRIMARY KEY, FK], vector_blob [BLOB], updated_at)
- Table "sessions": columns (iota_session_id [TEXT PRIMARY KEY], cwd [TEXT], active_backend [TEXT], model [TEXT], turn_count [INTEGER], created_at, last_used_at)
- Table "backend_sessions", table "turns", and table "handoffs" under sessions.sqlite
- Shows user_scope_candidates(user_id) and project_scope_candidates(project_id) filtering scope_id

Center Block: "Recall Buckets Mapping" (outlined in forest green)
Map lines connect filtered memory records into 6 output buckets of RecallBuckets based on RecallThresholds:
- "identity" bucket (scope=User, type=Semantic, facet=Identity, threshold >= 0.85) -> injected as <memory type="identity"> (blue outline)
- "preference" bucket (scope=User, type=Semantic, facet=Preference, threshold >= 0.80) -> injected as <memory type="preference"> (blue outline)
- "strategic" bucket (scope=Project, type=Semantic, facet=Strategic, threshold >= 0.80) -> injected as <memory type="strategic"> (blue outline)
- "domain" bucket (scope=Project, type=Semantic, facet=Domain, threshold >= 0.80) -> injected as <memory type="domain"> (blue outline)
- "procedural" bucket (scope=Project, type=Procedural, threshold >= 0.75) -> injected as <memory type="procedural"> (blue outline)
- "episodic" bucket (scope=Session/Project, type=Episodic, threshold >= 0.70) -> injected as <memory type="episodic"> (blue outline)

Right Block: "Lifecycle Controls" (outlined in terracotta orange)
- "Deduplication" (orange): ON CONFLICT(scope, scope_id, type, facet, content_hash) DO UPDATE SET updated_at, expires_at, confidence=MAX(memory.confidence, excluded.confidence)
- "TTL Expiry" (orange): filters expires_at > now
- "Compaction" (orange): compact_episodic_scope() deletes episodic records offset by episodic_compaction_keep

Solid lines with arrows trace the lifecycles from DB tables, through scope-filtering, into the XML prompt injection.

Style instructions:
- Follow the technical diagram rules and palette stated at the top of this prompt
- Use compact labels, clean sans-serif type, and consistent font size within each hierarchy level
- Use continuous solid lines with no overlapping or broken strokes
- Show schema snippets in monospace while keeping them readable
- Do not invent columns, scopes, facets, or table names
```

---

### 3.2 六桶记忆召回机制

**用途**: 展示六桶记忆系统的召回流程和向量/混合搜索机制

![六桶记忆召回机制](../img_result/memory_recall_buckets.png)


**Prompt**:
```
Selected GPT-Image2 style-library template: Infographic Engine (ID: infographic-engine; category: Charts & Infographics; example cases: case 334, case 1, case 8).
Use case: infographic-diagram. Asset type: technical architecture diagram for project documentation. Output settings: high outputQuality, wide landscape 16:9 PNG, readable labels, clean layout.
Use a consistent technical infographic style for this entire wide landscape 16:9 image.
Use the shared visual system: clean technical infographic, warm off-white paper background, precise thin ink lines, subtle hand-drawn engineering paper texture, restrained iota magenta accent, muted navy / forest green / terracotta / cyan / teal / gray module colors, readable labels, clear arrows, generous whitespace, no 3D, no neon, no stock cloud icons, no decorative blobs.
Palette: iota magenta #C026D3; deep navy #1E3A5F; forest green #2F6B4F; terracotta orange #C46A3A; protocol cyan #0E7490; backend teal #0F766E; neutral gray #52525B; paper background #F8F5EE.
Technical diagram rules: warm off-white paper, thin ink vector lines, rounded module rectangles, solid arrows, compact labels, clean sans-serif typography, disciplined spacing; keep labels short and readable; do not invent file paths, module names, database columns, commands, or backend names.
Negative details: unreadable tiny text, fake modules, fake code, wrong file paths, obsolete modules, crowded arrows, messy layout, 3D render, neon cyberpunk, glossy corporate dashboard, stock cloud architecture icons, gradient blobs, random decorative symbols, Korean text, non-Chinese non-English labels.
Create a technical flow diagram titled "Six-Bucket Memory Recall and Search Mechanisms".

The diagram shows three interconnected sections:

Left Section: "Recall Query Input" (outlined in navy blue)
- Input parameters: user_id, project_id, session_id, RecallThresholds
- user_scope_candidates(user_id) -> ["user-sympantos", "local-user", user_id]
- project_scope_candidates(project_id) -> [project_id, "iota-sympantos", basename(project_id)]

Center Section: "Six-Bucket Filtering" (outlined in forest green)
Shows six parallel query paths, each with:
1. Identity: query(scope=User, scope_id IN user_ids, type=Semantic, facet=Identity, confidence >= 0.85, limit=20)
2. Preference: query(scope=User, scope_id IN user_ids, type=Semantic, facet=Preference, confidence >= 0.80, limit=30)
3. Strategic: query(scope=Project, scope_id IN project_ids, type=Semantic, facet=Strategic, confidence >= 0.80, limit=30)
4. Domain: query(scope=Project, scope_id IN project_ids, type=Semantic, facet=Domain, confidence >= 0.80, limit=50)
5. Procedural: query(scope=Project, scope_id IN project_ids, type=Procedural, confidence >= 0.75, limit=10)
6. Episodic: query(scope=Session, scope_id=session_id, type=Episodic, confidence >= 0.70, limit=20) + query(scope=Project, scope_id IN project_ids, type=Episodic, confidence >= 0.70, limit=20)

Each query path shows:
- SQL WHERE clause filters
- ORDER BY confidence DESC, updated_at DESC, created_at DESC
- LIMIT enforcement
- TTL expiry filter (expires_at > now)

Right Section: "Search Modes" (outlined in terracotta orange)
Shows three search strategies:
1. Keyword Search:
   - FTS5 (if available): memory_fts MATCH query, ORDER BY rank
   - Fallback LIKE: content LIKE '%query%' ESCAPE '\'
2. Vector Search:
   - Embed query using EmbeddingEngine (Ollama or local trigram)
   - Load memory_embedding.vector_blob for all candidates
   - Calculate cosine similarity
   - Score = 0.65 * similarity + 0.20 * token_overlap + 0.15 * confidence
   - Filter score > 0.05
3. Hybrid Search:
   - Run both keyword and vector searches with 3x limit
   - Merge results using reciprocal rank fusion (RRF)
   - Weight vector results 1.2x higher
   - Return top limit results

Bottom: "Output Assembly" (outlined in purple)
- RecallBuckets struct with six Vec<MemoryRecord> fields
- Injected into <iota-context> capsule as XML <memory type="..."> blocks
- Character budget enforcement by ContextEngine

Style instructions:
- Follow the technical diagram rules and palette stated at the top of this prompt
- Use compact labels, clean sans-serif type, and consistent font size within each hierarchy level
- Show SQL snippets in monospace font
- Use continuous solid lines with no overlapping or broken strokes
- Do not invent query modes, thresholds, or scoring formulas
```

---

## 4. 观测性与调试

### 4.1 OpenTelemetry 观测性架构

**用途**: 展示完整的观测性架构，包括本地日志和 Docker 观测性栈

![OpenTelemetry 观测性架构](../img_result/observability_architecture.png)


**Prompt**:
```
Selected GPT-Image2 style-library template: Poster Layout System (ID: poster-layout-system; category: Posters & Typography; example cases: case 345, case 5, case 10), with Infographic Engine constraints for structured technical labels.
Use case: productivity-visual and infographic-diagram. Asset type: technical story-board poster for project documentation. Output settings: high outputQuality, wide landscape 16:9 PNG, readable labels, clean layout.
Use a consistent technical story-board style for this entire wide landscape 16:9 image.
Use the shared visual system: clean technical infographic, warm off-white paper background, precise thin ink lines, subtle hand-drawn engineering paper texture, restrained iota magenta accent, muted navy / forest green / terracotta / cyan / teal / gray module colors, readable labels, clear arrows, generous whitespace, no 3D, no neon, no stock cloud icons, no decorative blobs.
Palette: iota magenta #C026D3; deep navy #1E3A5F; forest green #2F6B4F; terracotta orange #C46A3A; protocol cyan #0E7490; backend teal #0F766E; neutral gray #52525B; paper background #F8F5EE.
Story poster rules: pen-and-ink technical story poster, warm paper texture, precise black linework, light cross-hatching, miniature engineering cutaway details, restrained iota magenta highlights, readable labels, and a diagram-like composition.
Negative details: unreadable tiny text, fake modules, fake code, wrong file paths, obsolete modules, crowded arrows, messy layout, 3D render, neon cyberpunk, glossy corporate dashboard, stock cloud architecture icons, gradient blobs, random decorative symbols, Korean text, non-Chinese non-English labels.
Use a pen-and-ink observability story poster with a signal routing diagram, hand-drawn infrastructure map, light cross-hatching, warm paper texture, and restrained iota magenta highlights.

Create a wide landscape story-board poster for the document "iota observability".

Scene: show iota as a small command desk sending three kinds of signals into an old-fashioned observatory: logs, traces, and metrics. The signals travel through brass-labeled tubes into an OpenTelemetry Collector at the center. From there, three paths branch to Loki as a log archive library, Jaeger as a trace telescope charting spans across the sky, and Prometheus as a metric gauge wall with moving needles. Grafana appears as a large wall screen that combines the three views. A separate lower-left corner shows local operation without Docker: stderr, daily files under ~/.i6/logs/, and SQLite stores under ~/.i6/context/.

Composition: wide landscape poster, 16:9 ratio. Central hub-and-spoke layout with the OTel Collector as the main switching lens. Put Docker observability stack on the right side and local fallback behavior on the left side. Include readable short labels: OTLP :4317, Loki :3100, Jaeger :16686, Prometheus :9090, Grafana :3000, OTEL_ENABLED=false, and iota logs / iota trace / iota metrics. Add the title "iota Observability" at the top.

Style: follow the story poster rules and palette stated at the top of this prompt. Keep precise arrows, readable infrastructure labels, and minimal magenta accents on live telemetry pulses. Avoid generic cloud architecture slide aesthetics.

Mood: investigative, transparent, a control room where every runtime signal can be followed from source to storage.

Negative prompt: unreadable tiny text, fake modules, fake code, wrong file paths, obsolete modules, crowded arrows, messy layout, 3D render, neon cyberpunk, glossy corporate dashboard, stock cloud architecture icons, gradient blobs, random decorative symbols, Korean text, non-Chinese non-English labels; also avoid blurry dashboards, random graphs without meaning, fake brand logos, and colorful SaaS illustration.
```

---

### 4.2 调试工作流

**用途**: 展示使用 VS Code 和 CodeLLDB 调试 iota-sympantos 的完整工作流

![调试工作流](../img_result/debugging_workflow.png)


**Prompt**:
```
Selected GPT-Image2 style-library template: Poster Layout System (ID: poster-layout-system; category: Posters & Typography; example cases: case 345, case 5, case 10), with Infographic Engine constraints for structured technical labels.
Use case: productivity-visual and infographic-diagram. Asset type: technical story-board poster for project documentation. Output settings: high outputQuality, wide landscape 16:9 PNG, readable labels, clean layout.
Use a consistent technical story-board style for this entire wide landscape 16:9 image.
Use the shared visual system: clean technical infographic, warm off-white paper background, precise thin ink lines, subtle hand-drawn engineering paper texture, restrained iota magenta accent, muted navy / forest green / terracotta / cyan / teal / gray module colors, readable labels, clear arrows, generous whitespace, no 3D, no neon, no stock cloud icons, no decorative blobs.
Palette: iota magenta #C026D3; deep navy #1E3A5F; forest green #2F6B4F; terracotta orange #C46A3A; protocol cyan #0E7490; backend teal #0F766E; neutral gray #52525B; paper background #F8F5EE.
Story poster rules: pen-and-ink technical story poster, warm paper texture, precise black linework, light cross-hatching, miniature engineering cutaway details, restrained iota magenta highlights, readable labels, and a diagram-like composition.
Negative details: unreadable tiny text, fake modules, fake code, wrong file paths, obsolete modules, crowded arrows, messy layout, 3D render, neon cyberpunk, glossy corporate dashboard, stock cloud architecture icons, gradient blobs, random decorative symbols, Korean text, non-Chinese non-English labels.
Use a pen-and-ink debugging workshop poster with a hand-drawn developer desk, annotated troubleshooting map, light cross-hatching, warm paper texture, and restrained iota magenta highlights.

Create a wide landscape story-board poster for the document "iota-sympantos debugging guide".

Scene: an engineer sits at a VS Code workbench with CodeLLDB tools arranged like precision instruments. A large Rust CLI machine is open for inspection: main.rs, cli/mod.rs, engine.rs, acp/mod.rs, and tui.rs appear as labeled access panels. Breakpoints glow as tiny red pinheads on the machine. A side panel shows debug configurations as selectable brass tags: Debug TUI, Debug Run, Debug Run with Daemon, Debug Check, Debug Context MCP Sidecar, Debug Fun MCP Server, Debug Bench Cold, and Debug Daemon. At the bottom, a terminal recovery station shows RUST_LOG=debug, RUST_BACKTRACE=1, local log files, and a reset lever for raw terminal recovery.

Composition: wide landscape poster, 16:9 ratio. Make the story read from left to right: prerequisites, configurations, breakpoints, stepping controls, variable inspection, TUI debugging, ACP subprocess boundary. Include a small keyboard strip with F5, F10, F11, Shift+F11, and Shift+F5. Add the title "Debugging iota-sympantos" at the top.

Style: follow the story poster rules and palette stated at the top of this prompt. Keep crisp technical linework, readable labels, subtle magenta accents on breakpoint dots and the active debug path, and a practical repair-manual feeling without becoming cartoonish.

Mood: focused troubleshooting, a repair manual for a complex but understandable runtime.

Negative prompt: unreadable tiny text, fake modules, fake code, wrong file paths, obsolete modules, crowded arrows, messy layout, 3D render, neon cyberpunk, glossy corporate dashboard, stock cloud architecture icons, gradient blobs, random decorative symbols, Korean text, non-Chinese non-English labels; also avoid chaotic code walls, photorealistic office scenes, glossy app screenshots, excessive red, fantasy laboratory details, distorted keyboards, fake stack traces, and low-detail doodles.
```

---

---

## 5. Kanban 任务编排系统

### 5.1 Kanban 状态机与事件溯源

**用途**: 展示 Kanban 任务的完整状态机、事件溯源架构和状态转换规则

![Kanban 状态机与事件溯源](../img_result/kanban_state_machine_event_sourcing.png)


**Prompt**:
```
Selected GPT-Image2 style-library template: Infographic Engine (ID: infographic-engine; category: Charts & Infographics; example cases: case 334, case 1, case 8).
Use case: infographic-diagram. Asset type: technical architecture diagram for project documentation. Output settings: high outputQuality, wide landscape 16:9 PNG, readable labels, clean layout.
Use a consistent technical infographic style for this entire wide landscape 16:9 image.
Use the shared visual system: clean technical infographic, warm off-white paper background, precise thin ink lines, subtle hand-drawn engineering paper texture, restrained iota magenta accent, muted navy / forest green / terracotta / cyan / teal / gray module colors, readable labels, clear arrows, generous whitespace, no 3D, no neon, no stock cloud icons, no decorative blobs.
Palette: iota magenta #C026D3; deep navy #1E3A5F; forest green #2F6B4F; terracotta orange #C46A3A; protocol cyan #0E7490; backend teal #0F766E; neutral gray #52525B; paper background #F8F5EE.
Technical diagram rules: warm off-white paper, thin ink vector lines, rounded module rectangles, solid arrows, compact labels, clean sans-serif typography, disciplined spacing; keep labels short and readable; do not invent file paths, module names, database columns, commands, or backend names.
Negative details: unreadable tiny text, fake modules, fake code, wrong file paths, obsolete modules, crowded arrows, messy layout, 3D render, neon cyberpunk, glossy corporate dashboard, stock cloud architecture icons, gradient blobs, random decorative symbols, Korean text, non-Chinese non-English labels.
Create a technical state machine and event sourcing diagram titled "Kanban Task State Machine & Event Sourcing Architecture".

The diagram is divided into three interconnected sections:

Left Section: "Task State Machine" (outlined in navy blue)
Shows a state transition diagram with the following states and transitions:
- States (rounded rectangles):
  - triage (entry state, light gray)
  - todo (yellow)
  - ready (green)
  - running (blue)
  - blocked (red)
  - done (purple)
  - archived (dark gray)

- Transitions (labeled arrows):
  - triage → todo: "transition"
  - todo → ready: "transition"
  - ready → running: "claim/start" (dispatcher or manual dispatch)
  - running → done: "complete"
  - running → blocked: "blocked/failure"
  - running → ready: "claim expired"
  - blocked → ready: "unblock/retry"
  - blocked → done: "manual resolution"
  - done → archived: "archive"

- Annotations:
  - "Dispatcher watches ready queue"
  - "Worker spawns hermes -z process"
  - "Shadow materializer projects files"
  - "Shadow watcher recycles on completion"

Center Section: "Event Sourcing Store" (outlined in forest green)
Shows the event-sourced architecture:
- Table "events": columns (id [INTEGER PRIMARY KEY], event_type [TEXT], payload [TEXT JSON], created_at [INTEGER])
- Table "tasks": columns (id [INTEGER PRIMARY KEY], board_id [INTEGER], title [TEXT], body [TEXT], status [TEXT], assignee [TEXT], priority [INTEGER], tags [TEXT JSON], workspace_kind [TEXT], workspace_path [TEXT], created_at, updated_at, claimed_at, claim_ttl_secs)
- Table "boards": columns (id [INTEGER PRIMARY KEY], slug [TEXT UNIQUE], name [TEXT], created_at)
- Table "runs": columns (id [TEXT PRIMARY KEY], task_id [INTEGER], profile [TEXT], status [TEXT], started_at, finished_at, last_heartbeat, exit_code, output_summary)
- Table "comments": columns (id [INTEGER PRIMARY KEY], task_id [INTEGER], author [TEXT], body [TEXT], created_at)
- Table "links": columns (from_id [INTEGER], to_id [INTEGER], kind [TEXT], PRIMARY KEY(from_id, to_id, kind))
- Table "event_sync_cursors": columns (source [TEXT PRIMARY KEY], cursor [INTEGER], updated_at)

Event types shown:
- board_created, task_created, task_updated, task_deleted
- task_transitioned
- link_created, link_removed
- comment_added
- run_started, run_completed

Arrows show:
- Events append-only to events table
- Structured payload JSON can be replayed to rebuild state
- Current SQLite store writes events plus normalized tables for boards/tasks/runs/comments/links

Right Section: "Dispatcher & Worker Pipeline" (outlined in terracotta orange)
Shows the execution pipeline:
- Dispatcher (top):
  - Polls ready tasks every tick_interval (default 30 seconds)
  - Checks worker capacity (max concurrent workers)
  - Assigns task to available worker
  - Emits RunStarted event

- WorkerHandle (middle):
  - Spawns hermes -z <task_id> subprocess
  - Captures stdout/stderr to output file
  - Monitors process health
  - Emits run_completed event with status completed, failed, timed_out, or cancelled

- ShadowMaterializer (bottom left):
  - Projects task context to shadow directory
  - Creates task.md, context files, skill files
  - Injects task-specific memory and skills

- ShadowWatcher (bottom right):
  - Watches for task completion
  - Recycles shadow directory
  - Extracts artifacts and logs
  - Updates task status to done

Connections and Flow:
- Blue arrows from state machine to event store (state changes → events)
- Green arrows from event store to dispatcher (event replay → state reconstruction)
- Orange arrows from dispatcher through worker to shadow system (execution pipeline)
- Purple arrows from shadow watcher back to event store (completion → events)

Style instructions:
- Follow the technical diagram rules and palette stated at the top of this prompt
- Use compact labels, clean sans-serif type, and consistent font size within each hierarchy level
- Show SQL table schemas in monospace font
- Use continuous solid lines with no overlapping or broken strokes
- Do not invent extra state transitions, event names, or table columns
```

---

### 5.2 Kanban 分布式同步与桥接

**用途**: 展示 Kanban 的跨节点事件同步、任务分解和规格化桥接机制

![Kanban 分布式同步与桥接](../img_result/kanban_event_sync_bridge.png)


**Prompt**:
```
Selected GPT-Image2 style-library template: Infographic Engine (ID: infographic-engine; category: Charts & Infographics; example cases: case 334, case 1, case 8).
Use case: infographic-diagram. Asset type: technical architecture diagram for project documentation. Output settings: high outputQuality, wide landscape 16:9 PNG, readable labels, clean layout.
Use a consistent technical infographic style for this entire wide landscape 16:9 image.
Use the shared visual system: clean technical infographic, warm off-white paper background, precise thin ink lines, subtle hand-drawn engineering paper texture, restrained iota magenta accent, muted navy / forest green / terracotta / cyan / teal / gray module colors, readable labels, clear arrows, generous whitespace, no 3D, no neon, no stock cloud icons, no decorative blobs.
Palette: iota magenta #C026D3; deep navy #1E3A5F; forest green #2F6B4F; terracotta orange #C46A3A; protocol cyan #0E7490; backend teal #0F766E; neutral gray #52525B; paper background #F8F5EE.
Technical diagram rules: warm off-white paper, thin ink vector lines, rounded module rectangles, solid arrows, compact labels, clean sans-serif typography, disciplined spacing; keep labels short and readable; do not invent file paths, module names, database columns, commands, or backend names.
Negative details: unreadable tiny text, fake modules, fake code, wrong file paths, obsolete modules, crowded arrows, messy layout, 3D render, neon cyberpunk, glossy corporate dashboard, stock cloud architecture icons, gradient blobs, random decorative symbols, Korean text, non-Chinese non-English labels.
Create a technical distributed system diagram titled "Kanban Event Sync & Advanced Bridge".

The diagram is divided into three interconnected sections:

Left Section: "Event Synchronization" (outlined in navy blue)
Shows the distributed event sync architecture:
- Node A (local):
  - KanbanStore with iota.db (located at ~/.i6/kanban/iota.db)
  - EventSyncManager
  - HTTP TCP server on :8080 (serve-sync mode)
  - Export to JSON file

- Node B (remote):
  - KanbanStore with iota.db
  - EventSyncManager
  - Pull from Node A HTTP TCP endpoint
  - Import from JSON file

- Sync operations (labeled arrows):
  - export: events → JSON file
  - import: JSON file → events (with conflict resolution)
  - serve-sync: HTTP TCP server exposes event sync stream
  - pull: HTTP TCP client fetches events from remote
  - push: HTTP TCP POST events to remote

- Conflict resolution strategies:
  - Last-write-wins by timestamp
  - Event ID deduplication
  - Board/task ID namespace isolation

Center Section: "Advanced Bridge" (outlined in forest green)
Shows the task decomposition and specification bridge:
- AdvancedBridge orchestrator:
  - decompose(task) → subtasks
  - specify(task) → detailed requirements
  - Uses requirement-detailer sub-agent
  - Integrates with IotaEngine for AI-powered analysis

- Decomposition flow:
  1. Input: high-level task description
  2. AI analysis: identify subtasks, dependencies, priorities
  3. Output: task_created events for each subtask
  4. link_created events for parent-child relationships

- Specification flow:
  1. Input: task with vague requirements
  2. AI analysis: QA-based requirement detailing
  3. Output: task_updated event with detailed description
  4. Metadata: acceptance criteria, technical notes

Right Section: "Integration Points" (outlined in terracotta orange)
Shows how Kanban integrates with the rest of iota:
- TUI Kanban View:
  - /kanban or /kb command enters kanban mode
  - Board list, task list, task detail views
  - Keyboard navigation (j/k/h/l to move focus, Tab to cycle view modes, Enter to inspect details, Esc/q to close)
  - Keyboard hotkeys (d to dispatch/start task, m to prefill /kanban move command, c to comment, a to assign, s to specify, x to decompose)

- CLI Kanban Commands:
  - iota kanban create-board <slug> <name>
  - iota kanban create-task <board-id> <title>
  - iota kanban move <id> <status>
  - iota kanban dispatch <id> [--timeout <secs>]
  - iota kanban specify <id>
  - iota kanban decompose <id>
  - iota kanban export <path> [cursor]
  - iota kanban import <path>
  - iota kanban serve-sync [addr]
  - iota kanban pull <addr> [cursor]
  - iota kanban push <addr> [cursor]

- Engine Integration:
  - Worker uses IotaEngine for task execution
  - Task context injected into memory system
  - Task-specific skills loaded from shadow directory
  - Execution results captured as task artifacts

Connections and Flow:
- Blue arrows between nodes showing event sync
- Green arrows from bridge to event store (decompose/specify → events)
- Orange arrows from integration points to core Kanban system
- Purple arrows showing feedback loops (execution results → task updates)

Style instructions:
- Follow the technical diagram rules and palette stated at the top of this prompt
- Use compact labels, clean sans-serif type, and consistent font size within each hierarchy level
- Use continuous solid lines with no overlapping or broken strokes
- Do not invent sync endpoints, bridge commands, or keyboard shortcuts
```

---

## 6. Desktop (Tauri) 架构

### 6.1 Desktop 应用架构与通信流

**用途**: 展示 iota-desktop Tauri 应用的前后端架构、Daemon 通信和状态管理

![Desktop 应用架构与通信流](../img_result/desktop_tauri_architecture.png)


**Prompt**:
```
Selected GPT-Image2 style-library template: Infographic Engine (ID: infographic-engine; category: Charts & Infographics; example cases: case 334, case 1, case 8).
Use case: infographic-diagram. Asset type: technical architecture diagram for project documentation. Output settings: high outputQuality, wide landscape 16:9 PNG, readable labels, clean layout.
Use a consistent technical infographic style for this entire wide landscape 16:9 image.
Use the shared visual system: clean technical infographic, warm off-white paper background, precise thin ink lines, subtle hand-drawn engineering paper texture, restrained iota magenta accent, muted navy / forest green / terracotta / cyan / teal / gray module colors, readable labels, clear arrows, generous whitespace, no 3D, no neon, no stock cloud icons, no decorative blobs.
Palette: iota magenta #C026D3; deep navy #1E3A5F; forest green #2F6B4F; terracotta orange #C46A3A; protocol cyan #0E7490; backend teal #0F766E; neutral gray #52525B; paper background #F8F5EE.
Technical diagram rules: warm off-white paper, thin ink vector lines, rounded module rectangles, solid arrows, compact labels, clean sans-serif typography, disciplined spacing; keep labels short and readable; do not invent file paths, module names, database columns, commands, or backend names.
Negative details: unreadable tiny text, fake modules, fake code, wrong file paths, obsolete modules, crowded arrows, messy layout, 3D render, neon cyberpunk, glossy corporate dashboard, stock cloud architecture icons, gradient blobs, random decorative symbols, Korean text, non-Chinese non-English labels.
Create a technical application architecture diagram titled "iota-desktop (Tauri) Architecture & Communication Flow".

The diagram is divided into four vertical sections:

Left Section: "React Frontend" (outlined in cyan)
Shows the React UI architecture:
- Components:
  - App.tsx (root)
  - ChatWorkbench.tsx (main chat panel, streams text chunks, handles approvals, displays context state)
  - ConfigPanel.tsx (model configurations and backend switching)
  - MemoryContextWorkspace.tsx (persistent memory taxonomy view & context inspection)
  - RightInspector.tsx (individual turn details, timing stats, event trace inspector)

- State Management:
  - turnReducer.ts (Redux-style reducer managing conversation history)
  - Turn states: pending, streaming, completed, failed
  - Message types: user, assistant, system
  - Event accumulation per turn

- UI Features:
  - Markdown rendering with syntax highlighting
  - Code block copy buttons
  - Approval request overlays
  - Real-time streaming updates
  - Turn detail expansion/collapse

Center-Left Section: "Tauri Commands" (outlined in purple)
Shows the Rust backend commands exposed to frontend:
- submit_prompt(prompt, backend_str, turn_id, window) → turn_id
- cancel_turn(turn_id, window) → success
- handle_approval(req_id, approved) → success
- get_config() → DesktopConfigSnapshot
- save_backend_model(backend_str, model) → DesktopConfigSnapshot
- check_backend(backend_str) → DaemonServerMessage
- get_observability_summary() → Value
- get_memory_context_snapshot(scope_mode) → DesktopMemoryContextSnapshot
- current_workspace() → String
- Kanban commands: list_boards(), list_tasks(filter), create_task(req), transition_task(task_id, to_status), list_comments(task_id), add_comment(task_id, author, body)

- Window Events (Tauri emit):
  - "daemon-message" → DaemonServerMessage
    - TextChunk(turn_id, text)
    - TurnEvent(turn_id, event)
    - ApprovalRequested(turn_id, request)
    - TurnComplete(turn_id, timing)
    - TurnFailed(turn_id, error)

Center-Right Section: "Daemon Client" (outlined in orange)
Shows the daemon communication layer:
- DaemonClient:
  - connect_or_start() → auto-start daemon if not running via ipc_client::autostart
  - CSPRNG token authentication on Hello (constant-time compare)
  - Protocol version negotiation (PROTOCOL_VERSION_MIN / MAX)
  - start_turn(window, turn_id, cwd, backend, prompt)
  - cancel_turn(turn_id)
  - respond_approval(turn_id, decision)
  - Exponential-backoff-with-jitter reconnect (ipc_client::backoff)

- Protocol:
  - TCP connection to 127.0.0.1:47661
  - Newline-delimited JSON messages
  - DaemonClientMessage (Hello, StartTurn, CancelTurn, RespondApproval, etc.)
  - DaemonServerMessage (TextChunk, TurnEvent, ApprovalRequested, TurnComplete, TurnFailed, etc.)

- ApprovalRegistry:
  - Tracks pending approvals per turn
  - Routes approval responses to correct turn
  - Timeout handling for stale approvals
  - deny_for_turn() cancels all pending approvals on turn cancel

Right Section: "Daemon & Engine" (outlined in forest green)
Shows the backend execution:
- Daemon:
  - handle_desktop_connection()
  - Spawns tokio task per turn
  - Streams messages back to client
  - Manages EnginePool per cwd

- IotaEngine:
  - run_with_timing() execution
  - Streams RuntimeEvent to daemon
  - Approval requests routed through daemon
  - Turn completion with timing stats

- EnginePool:
  - Reuses IotaEngine per cwd
  - Config hot-reload support
  - Engine reset on config change

Connections and Flow:
- Cyan arrows from React components to Tauri commands (invoke)
- Purple arrows from Tauri backend to daemon client (TCP)
- Orange arrows from daemon client to daemon server (JSON protocol)
- Green arrows from daemon to engine (execution)
- Red arrows showing reverse flow (events → frontend)

Sequence markers (use circled markers):
1. User types prompt in ChatWorkbench
2. Click Send → submit_prompt Tauri command
3. Tauri backend → DaemonClient.start_turn()
4. DaemonClient → TCP StartTurn message
5. Daemon → spawn turn task
6. Turn task → IotaEngine.run_with_timing()
7. Engine → stream RuntimeEvent
8. Daemon → DaemonServerMessage
9. DaemonClient → emit "daemon-message" window event
10. React → turnReducer processes message
11. ChatWorkbench → re-renders with new content

Style instructions:
- Follow the technical diagram rules and palette stated at the top of this prompt
- Use compact labels, clean sans-serif type, and consistent font size within each hierarchy level
- Show TypeScript/Rust type signatures in monospace font
- Use continuous solid lines with no overlapping or broken strokes
- Do not invent Tauri commands, daemon messages, or frontend components
```

---

## 7. 配置与环境变量映射

### 7.1 配置层次与后端环境变量映射

**用途**: 展示 nimia.yaml 配置结构和各后端的环境变量映射规则

![配置层次与后端环境变量映射](../img_result/configuration_env_mapping.png)


**Prompt**:
```
Selected GPT-Image2 style-library template: Infographic Engine (ID: infographic-engine; category: Charts & Infographics; example cases: case 334, case 1, case 8).
Use case: infographic-diagram. Asset type: technical architecture diagram for project documentation. Output settings: high outputQuality, wide landscape 16:9 PNG, readable labels, clean layout.
Use a consistent technical infographic style for this entire wide landscape 16:9 image.
Use the shared visual system: clean technical infographic, warm off-white paper background, precise thin ink lines, subtle hand-drawn engineering paper texture, restrained iota magenta accent, muted navy / forest green / terracotta / cyan / teal / gray module colors, readable labels, clear arrows, generous whitespace, no 3D, no neon, no stock cloud icons, no decorative blobs.
Palette: iota magenta #C026D3; deep navy #1E3A5F; forest green #2F6B4F; terracotta orange #C46A3A; protocol cyan #0E7490; backend teal #0F766E; neutral gray #52525B; paper background #F8F5EE.
Technical diagram rules: warm off-white paper, thin ink vector lines, rounded module rectangles, solid arrows, compact labels, clean sans-serif typography, disciplined spacing; keep labels short and readable; do not invent file paths, module names, database columns, commands, or backend names.
Negative details: unreadable tiny text, fake modules, fake code, wrong file paths, obsolete modules, crowded arrows, messy layout, 3D render, neon cyberpunk, glossy corporate dashboard, stock cloud architecture icons, gradient blobs, random decorative symbols, Korean text, non-Chinese non-English labels.
Create a technical configuration mapping diagram titled "Configuration Hierarchy & Backend Environment Variable Mapping".

The diagram is divided into three sections:

Top Section: "Configuration Hierarchy" (outlined in navy blue)
Shows the configuration file structure:
- ~/.i6/nimia.yaml (single source of truth):
  - Global settings:
    - context_engine (enabled, injection [auto/off/prompt/mcp], memory_db, skill_roots, budgets, recall_thresholds, episodic_compaction_keep, mcp, fun, embedding)
    - context_engine_backend (maps backends: "claude-code", "codex", "gemini", "hermes", "opencode" to context options)
    - store (cache_retention_days, cache_running_ttl_secs, observability_retention_days, approvals_max_pending_age_secs)

  - Backend sections (5 backends in nimia.yaml):
    - claude_code, codex, gemini, hermes, opencode
    - Each with: enabled, acp (command, args), version_mapping, home, model (provider, name, base_url, api_key), tool_whitelist

- No project-level config discovery
- No automatic config merging
- Template: nimia.yaml.template

Center Section: "Backend Environment Variable Mapping" (outlined in forest green)
Shows five backend mapping tables:

1. Claude Code (cyan box):
   - api_key → ANTHROPIC_API_KEY, ANTHROPIC_AUTH_TOKEN
   - base_url → ANTHROPIC_BASE_URL
   - name → ANTHROPIC_MODEL, ANTHROPIC_SMALL_FAST_MODEL, ANTHROPIC_DEFAULT_*_MODEL
   - Additional: API_TIMEOUT_MS=3000000, CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1
   - home → CLAUDE_CONFIG_DIR (if override_home=true)

2. Codex (blue box):
   - api_key → OPENAI_API_KEY, ROUTER_API_KEY
   - base_url → OPENAI_BASE_URL
   - name → OPENAI_MODEL
   - Additional: -c args (model, model_provider, base_url, env_key, wire_api)
   - home → not mapped

3. Gemini CLI (yellow box):
   - api_key → GEMINI_API_KEY
   - name → GEMINI_MODEL
   - home → GEMINI_CONFIG_DIR (if override_home=true)

4. Hermes Agent (purple box):
   - provider → HERMES_INFERENCE_PROVIDER
   - name → HERMES_MODEL
   - api_key + base_url → provider-native env vars (e.g., MINIMAX_CN_API_KEY, MINIMAX_CN_BASE_URL)
   - home → NOT MAPPED (Hermes uses its own default HERMES_HOME)
   - Note: Do not override HERMES_HOME

5. OpenCode (orange box):
   - name → OPENCODE_MODEL
   - home → OPENCODE_CONFIG_DIR (if override_home=true)

Bottom Section: "Context Options & MCP Injection" (outlined in terracotta orange)
Shows per-backend context configuration from context_engine_backend:
- mcp_session_new:
  - "try": inject mcpServers for Claude Code and Codex only
  - "always": inject for all backends
  - "never": never inject

- always_send_empty_mcp_servers:
  - true: send empty mcpServers array even when no servers configured
  - false: omit mcpServers field when empty

- mcp_env_shape:
  - "string_array": env as ["KEY=value", ...]
  - "object": env as {"KEY": "value", ...}

- override_home:
  - true: map backend home to corresponding env var (e.g., CLAUDE_CONFIG_DIR)
  - false: let backend use its default home

- MCP server injection:
  - iota-context: memory search/write, skill search/load, session summary, handoff publish/read
  - iota-fun: 7 language function runners (Python, TypeScript, Rust, Go, Java, C++, Zig)

Connections and Flow:
- Blue arrows from config file to backend sections
- Green arrows from backend sections and context_engine_backend to environment variable mappings
- Orange arrows from context options to MCP injection logic
- Purple arrows showing runtime config → EffectiveConfig → backend process env

Style instructions:
- Follow the technical diagram rules and palette stated at the top of this prompt
- Use compact labels, clean sans-serif type, and consistent font size within each hierarchy level
- Show YAML structure and env var names in monospace font
- Use continuous solid lines with no overlapping or broken strokes
- Do not invent config files, environment variables, or backend home mappings
```

---

## 8. 技能系统 (Skill System)

### 8.1 技能加载、匹配与执行

**用途**: 展示技能系统的完整生命周期：加载、触发匹配、模板渲染和执行

![技能加载、匹配与执行](../img_result/skill_system_pipeline.png)


**Prompt**:
```
Selected GPT-Image2 style-library template: Infographic Engine (ID: infographic-engine; category: Charts & Infographics; example cases: case 334, case 1, case 8).
Use case: infographic-diagram. Asset type: technical architecture diagram for project documentation. Output settings: high outputQuality, wide landscape 16:9 PNG, readable labels, clean layout.
Use a consistent technical infographic style for this entire wide landscape 16:9 image.
Use the shared visual system: clean technical infographic, warm off-white paper background, precise thin ink lines, subtle hand-drawn engineering paper texture, restrained iota magenta accent, muted navy / forest green / terracotta / cyan / teal / gray module colors, readable labels, clear arrows, generous whitespace, no 3D, no neon, no stock cloud icons, no decorative blobs.
Palette: iota magenta #C026D3; deep navy #1E3A5F; forest green #2F6B4F; terracotta orange #C46A3A; protocol cyan #0E7490; backend teal #0F766E; neutral gray #52525B; paper background #F8F5EE.
Technical diagram rules: warm off-white paper, thin ink vector lines, rounded module rectangles, solid arrows, compact labels, clean sans-serif typography, disciplined spacing; keep labels short and readable; do not invent file paths, module names, database columns, commands, or backend names.
Negative details: unreadable tiny text, fake modules, fake code, wrong file paths, obsolete modules, crowded arrows, messy layout, 3D render, neon cyberpunk, glossy corporate dashboard, stock cloud architecture icons, gradient blobs, random decorative symbols, Korean text, non-Chinese non-English labels.
Create a technical skill system diagram titled "Skill Loading, Matching & Execution Pipeline".

The diagram is divided into four sections:

Left Section: "Skill Loading Roots" (outlined in navy blue)
Shows the skill loading hierarchy:
- SkillRegistry loads from multiple roots (in order):
  1. Workspace skills/ directory
  2. Workspace .iota/skills/ directory
  3. Configured skill_roots from nimia.yaml
  4. ~/.i6/skills/ (user-global skills)

- Skill structure:
  - .md / .yaml / .yml file with YAML frontmatter:
    - name, description, version
    - summary, tags, triggers
    - backends: [claude, codex, gemini, hermes, opencode]
    - execution:
      - mode: "mcp" or "advisory"
      - server: MCP server name (for mode=mcp)
      - tools: [tool calls] (for mode=mcp)
      - parallel: true/false (for mode=mcp)
    - output.template for optional rendered response text
  - Additional files: scripts, data, documentation

- Skill cache:
  - iota skill pull <source> [name] [--sha256 <hex-digest>]
  - Copies from a local path or downloads only from HTTPS; plain HTTP is rejected
  - Applies a size limit and timeout, optionally verifies SHA-256, then atomically writes the cache file and provenance metadata

Center-Left Section: "Trigger Matching" (outlined in forest green)
Shows the skill matching logic:
- Match criteria:
  1. Backend compatibility check through `backends`
  2. Trigger keyword match (case-insensitive substring)
  3. Skill is eligible when `backends` is empty or contains current backend

- Matching algorithm:
  - Iterate through all loaded skills
  - Check `backends` contains current backend or is empty
  - Check if any trigger matches prompt or context
  - Return the first compatible matched skill from registry order

- Context injection:
  - Matched skills injected into <iota-context> capsule
  - Skill index shows available skills to backend LLM
  - Backend can reference skills in its reasoning

Center-Right Section: "Template Rendering" (outlined in purple)
Shows the template rendering logic:
- Template variables (simple placeholder replace, no Handlebars engine):
  - {{prompt}}: user's original prompt string
  - {{skill.name}}: matched skill name
  - {{alias}}: (for mode=mcp) maps tool result placeholders
  - {{tool_results}}: (for mode=mcp) maps all aggregated tool results

- Rendering engine:
  - Pure string replacement (.replace) on metadata templates
  - Prepares final advisory text or tool payload strings

Right Section: "Execution Modes" (outlined in terracotta orange)
Shows the two execution modes:

1. Mode: "advisory" (top):
   - Skill body matches and acts as system-prompt instruction
   - Injected into <iota-context> capsule advisor context
   - Backend LLM processes as part of runtime context
   - No separate script execution step

2. Mode: "mcp" (bottom):
   - Skill bypasses ACP backend execution
   - skill::runner::run_engine_skill() executes directly
   - Spawns target MCP server (stdio)
   - Performs sequential or parallel tools/call requests
   - Tool results captured as RuntimeEvent and replaced in template
   - Output returned directly to CLI / TUI

Connections and Flow:
- Blue arrows from loading roots to SkillRegistry
- Green arrows from SkillRegistry to trigger matching
- Purple arrows from matched skills to template rendering
- Orange arrows from execution modes to output
- Red arrows showing mode=mcp bypassing ACP backend

Style instructions:
- Follow the technical diagram rules and palette stated at the top of this prompt
- Use compact labels, clean sans-serif type, and consistent font size within each hierarchy level
- Show YAML structure and template placeholders in monospace font
- Use continuous solid lines with no overlapping or broken strokes
- Do not invent skill metadata fields, matching rules, or execution modes
```

---

## 附录：图表使用指南

### 图表索引

| 编号 | 标题 | 类型 | 用途 |
| :---| :---| :---| :---|
| 1.1 | 分层架构与组件依赖 | 技术图 | 展示四层架构和模块依赖关系 |
| 1.2 | 完整运行时架构图 | 技术图 | 展示所有模块、数据流和序列标记 |
| 1.3 | 架构总览海报 | 海报 | 故事化展示整体架构 |
| 2.1 | Prompt 执行时序 | 技术图 | 展示从输入到输出的完整时序 |
| 2.2 | 代码调用链路 | 海报 | 故事化展示调用链路 |
| 2.3 | Backend 调用与 IPC | 技术图 | 展示三阶段执行流程 |
| 3.1 | 记忆分类与生命周期 | 技术图 | 展示数据库模式和分类体系 |
| 3.2 | 六桶记忆召回机制 | 技术图 | 展示召回流程和搜索机制 |
| 4.1 | OpenTelemetry 观测性架构 | 海报 | 故事化展示观测性栈 |
| 4.2 | 调试工作流 | 海报 | 故事化展示调试流程 |
| 5.1 | Kanban 状态机与事件溯源 | 技术图 | 展示状态机和事件溯源架构 |
| 5.2 | Kanban 分布式同步与桥接 | 技术图 | 展示事件同步和任务分解 |
| 6.1 | Desktop 应用架构与通信流 | 技术图 | 展示 Tauri 应用架构 |
| 7.1 | 配置层次与后端环境变量映射 | 技术图 | 展示配置结构和环境变量映射 |
| 8.1 | 技能加载、匹配与执行 | 技术图 | 展示技能系统完整生命周期 |

### 使用场景

| 场景 | 推荐图表 |
| :---| :---|
| 新人入职，了解整体架构 | 1.3, 1.2, 1.1 |
| 理解执行流程 | 2.1, 2.2, 2.3 |
| 开发记忆功能 | 3.1, 3.2 |
| 配置观测性 | 4.1 |
| 调试问题 | 4.2, 2.2 |
| 开发 Kanban 功能 | 5.1, 5.2 |
| 开发 Desktop 应用 | 6.1 |
| 配置后端 | 7.1 |
| 开发技能 | 8.1 |
| 技术分享 | 1.3, 2.2, 4.1, 4.2 |
| 文档封面 | 1.3 |
