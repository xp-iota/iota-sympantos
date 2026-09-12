# iota-sympantos book

iota-sympantos 是用 Rust 写的一个轻量级 agent harness。它通过 ACP（Agent Control Protocol）把 Claude Code、Codex、Gemini CLI、Hermes、OpenCode 这五个 AI Agent统一驱动起来，视其为backend，并在其之上建设一层分布式服务的能力——上下文注入、持久记忆、技能加载、MCP 工具、运行时事件、可观测性，以及专为长任务设计的 Kanban 调度系统。

本文希望呈现的不是「如何使用iota」，而是「iota 为什么这样设计」。每个模块背后都有一组明确的取舍，知道它**不做什么**，往往比知道它做什么更能看清它的边界。

![iota desktop](../img_result/iota-desktop-snapshot.png)

*图：iota-desktop 桌面工作台界面——左侧对话区、右侧 inspector 与配置面板。*

![architecture overview](../img_result/architecture_overview.png)

*图：iota-sympantos 整体架构故事图——中央控制塔统一驱动 Claude Code、Codex、Gemini CLI、Hermes、OpenCode 五个后端，底层是 Context Fabric、SQLite 存储、ACP 管道、MCP sidecar、可观测性与 Kanban 工坊。*

站在用户这一侧，iota 就是一个 CLI / TUI / Desktop 工具。一条 `iota run codex "..."` 打一次后端，裸敲 `iota` 进 ratatui 交互终端，加个 `--daemon` 就复用常驻进程里已经热好的 ACP 连接。`crates/iota-desktop` 是 Tauri + React 的本地工作台，`iota kanban ...` 则把那些一次 prompt 干不完的活写进事件溯源任务板，丢给 Hermes worker 去跑。

它的本质是一个 agent runtime：

- ACP 后端只管模型推理和写代码。
- iota-core 管进程、协议、上下文、记忆、技能，以及把各类事件归一成同一种语言。
- SQLite 是所有本地状态唯一的落地点。
- MCP 是模型读写 iota 能力时的标准工具接口。
- RuntimeEvent 是 UI、日志、token 用量、inspector 共享的那门「事件语言」。

那为什么要单独造这么一层 runtime？因为今天每个 AI Agent都倾向于把配置、上下文、记忆、工具权限、可观测性一股脑塞进自己的产品里，彼此不通。换个后端，记忆就断了，工具授权要重来一遍。iota 把这些能力从单个产品里拆出来、沉到本地，让五个后端共享同一套工作记忆和任务系统。后端随时换，runtime 这层不动。

时下，业内开始对Harness工程有了初步清晰的定义，包含了九个要素：主循环、上下文管理、技能与工具、子智能体、内置原语与会话持久化、系统提示词汇编、生命周期钩子、执行权限与安全。iota 在这九条线上都画了同一条边界——**建哪一层、不建哪一层，不建的那一层如何交还给后端**。这条边界由四组判断准则决定。

**跨 session 的事实归 iota，单 session 内的滚动归后端。** memory taxonomy、session ledger、event-sourced Kanban、fencing token 是跨 session 资产，必须有唯一事实层；但一次 `session/prompt` 内的 token 滚动截断、压缩、cache eviction，各家策略不同，强插会和后端冲突。

**跨后端、跨进程的归 iota，后端私有的归后端。** `<iota-context>` capsule、ACP stream router、`<handoff>` 摘要是给所有后端共享的最大公约数；后端的 system/developer/context slot、原生工具 schema、私有 hook，iota 不碰。

**本地确定性归 iota，云端沙盒不去打开。** iota-fun sandbox、engine-run skill、approval 维度分类，都建在「单用户单机、可信」这一假设上；容器沙盒、egress 限制、跨网络鉴权交给云端 harness 或调用方。

**任务级子 agent 归 iota，思考级子 agent 归后端。** Kanban worker 跨进程、跨崩溃跑长任务，shadow DB 隔离主库；ReAct 单 session 子 agent 由各后端自管，颗粒度差着一个数量级。

把四条线收成一句：**iota 守的是跨 session、跨后端、跨进程这一层「事实层」——长任务的归宿、跨工具的事实、跨 session 的资产；单 session 内的推理循环、token 滚动、原生工具 schema、ReAct 子 agent、私有 hook、各家 session 文件——这些是后端的工作，iota 不抢。** 后端随时换，runtime 这层不动。

## 设计立场

iota 的每一处实现，背面都站着一组明确的取舍。这些立场不是事后总结的漂亮话，而是真正决定了 iota 的边界——也决定了它不去跟谁抢地盘。

**iota 的价值在编排，不在推理。** *↔ 9 要素 #1（主循环）。* 这是贯穿全书的地基。iota 把五个 AI Agent一视同仁地当成「推理 + 写代码」的执行器——同一次会话里，你可以从 codex 切到 claude 再切回来，上下文不丢。它刻意不去碰每个后端原生的工具调用协议、prompt 模板、权限体系，不自带 LLM 客户端，也不试图把各家的 reasoning 风格捏成一种。

这些工具本身就是成熟的 coding agent，重造一遍只会很快过时。iota 只在它们暴露出来的 ACP 边界上做拦截、归一和编排，把推理这件事整个交还给后端，自己专心做 runtime 该做的事。

**本地优先，干脆就按单用户单机来设计。** *↔ 9 要素 #5（内置原语）#9（权限与安全）。* 所有持久化落 SQLite，所有配置只读 `~/.i6/nimia.yaml`，daemon 默认监听 `127.0.0.1:47661`，可观测数据双写本地 SQLite 和可选的 OpenTelemetry 栈。不要求外部数据库、消息队列或中央配置服务，也不假设多租户；但本机控制面不是无鉴权的：敏感 daemon 请求必须通过 owner-only CSPRNG token 认证，并写入不含 token 的审计事件。

agent runtime 干的活天然是「贴着机器」的——直接读写本地源码、调本地 shell、起外部进程。本地优先换来三件实在事：部署就一行命令，备份就拷一个目录，调试就直接打开 sqlite。至于多用户、多租户，那是调用方在 iota 之上自己该搭的，不该让 runtime 背这个包袱。

**配置只有一个源——`~/.i6/nimia.yaml`，不搞项目级自动发现。** *↔ 9 要素 #6（持久化与记忆）。* backend、model、API key、MCP server、retention 这些 runtime 配置全部集中读一份文件，缺了哪项，`effective.rs` 给一个能讲清楚来历的默认值。它不读 `./iota.yaml`、`.iota/config.yaml`、`pyproject.toml` 里的同名配置，也不做「一路往上找最近一份配置」的自动发现。

理由很实际：agent 配置里塞着 API key、provider base URL、后端 home，集中到 user home 才能躲开误提交进版本库、跨项目互相污染，以及最阴的那种——「在 A 项目下不小心用了 B 项目的 key」。项目级的偏好应该靠 workspace skill（`./.iota/skills`）和 cwd-scoped memory 去表达，而不是再开一份配置文件。

**上下文是一个 capsule，不是协议字段。** *↔ 9 要素 #2（上下文管理）#7（提示词汇编）。* iota 把 memory、skill、working memory、workspace 状态、handoff、session 元数据统统拼成一段 XML 风格的文本（`<iota-context>`），跟用户的 prompt 一起发过去。不指望各后端的 system prompt slot、developer message 或专有 context API，也不奢望后端能「读懂 iota 的私有协议」。

capsule 这种形式的最大好处是兼容：它对所有后端都是可读纯文本；XML-like 的 tag 让模型一眼分清「背景数据」和「用户真正的请求」；拼好的 capsule 还能离线 diff、回放、审计——这些是埋进协议字段做不到的。

**工具调用必须可拦截、可解释、可审计。** *↔ 9 要素 #8（生命周期钩子）#9（权限与安全）。* Approval、tool call、tool result、token usage、memory 写入全都归一成 `RuntimeEvent` 落进 SQLite；MCP 工具同时挂在 stdio server 和 ACP stream router 两条路上，保证派发逻辑是同一套；外部 MCP 工具默认一律拒绝，并用 `isError:true` 的 envelope 把拒绝原因原样回传给模型。

它不让后端绕过 iota 直接动 memory DB 或 Kanban DB，既不静默批准，也不静默拒绝。只要后端有能力悄悄写本地状态，runtime 的「事实来源」当场就塌了。把事件归一、把调用拦下来，UI、可观测性、ledger 看到的才是同一个世界——否则你永远在追查「数据库里这条记录到底是谁写的」。

**长任务交给 Kanban，别交给对话历史。** *↔ 9 要素 #4（子智能体）#6（持久化与记忆）。* 凡是「一次 prompt 干不完」的活，都装进 `iota-sympantos-kanban` 的事件溯源任务板，由状态机推着走 triage → todo → ready → running → done，再由 dispatcher 调度 Hermes worker 去执行。Hermes 只能透过 shadow DB 间接碰主库。

这是从血的教训里来的：对话历史会被截断、被压缩、跨 session 直接丢掉，拿它当「待办清单」早晚出事。任务板才是结构化、能恢复、能同步的事实层。再加一层 shadow DB，外部 worker 崩了也脏不到 iota 主库。

**跨平台是硬约束，不是锦上添花。** *↔ 9 要素 #5（内置原语）。* home 路径一律走 `dirs::home_dir()`，命令过一遍 `normalize_command()`（Windows 上把 `npx` 改成 `npx.cmd`），子进程统一 `Stdio::piped()` + `kill_on_drop(true)`。runtime 代码里绝不硬编码 `~`、`/` 或 `\`，也不擅自接管外部 home 目录（比如 Hermes 的 `HERMES_HOME`）。

这层 runtime 要起外部进程、写本地库、注入 MCP server，还得常驻成 daemon，任何平台差异只要往后拖到「调用现场再处理」，最后都会变成最难查的那类 bug——进程泄漏、路径错乱。所以宁可在代码里一次性吃掉。

## iota-sympantos 总览

![layered architecture](../img_result/layered_architecture.png)

*图：四层架构与组件依赖——presentation（cli/tui）、orchestration & Kanban、protocol & tools（ACP/MCP/context/skill）、external boundaries（后端子进程、MCP sidecar、SQLite 存储），依赖自上而下单向流动。*

![runtime architecture](../img_result/runtime_architecture.png)

*图：完整运行时架构——入口/CLI/TUI、daemon、engine、context/memory、ACP、backend、skill/MCP、Kanban 八列模块图，配合带圈序列标记呈现一次请求的全链路数据流。*

iota-sympantos 包含四个 crate：

| Crate | 类型 | 职责 |
| :--- | :--- | :--- |
| `iota-cli` | Binary crate | CLI 命令、TUI、daemon client、Kanban CLI、observability CLI |
| `iota-core` | Library crate | Workspace 目录名；发布包名为 `iota-sympantos-core`，library target 为 `iota_core`。负责 ACP/MCP/daemon/engine/config/context/memory/skill/store/telemetry 核心运行时 |
| `iota-sympantos-kanban` | Library crate | 已发布到 crates.io 的独立 crate，负责 Event-sourced Kanban、状态机、Hermes worker、shadow DB、跨节点同步 |
| `iota-desktop/src-tauri` | Tauri crate | 桌面端 Rust 命令、daemon client、Kanban 命令绑定 |

核心目录结构：

```sh
crates/
├── iota-cli/src/
│   ├── cli/                 # run/check/bench/mcp/skill/kanban/observability/__daemon
│   └── tui/                 # ratatui terminal UI
├── iota-core/src/
│   ├── acp/                 # ACP process and JSON-RPC wire protocol
│   ├── config/              # ~/.i6/nimia.yaml schema and effective config
│   ├── context/             # <iota-context> capsule composer
│   ├── daemon/              # TCP daemon, legacy CLI protocol, desktop protocol v2
│   ├── engine/              # IotaEngine orchestration
│   ├── mcp/                 # iota-context MCP server, router, tool dispatch
│   ├── memory/              # SQLite memory, taxonomy, FTS/vector/hybrid search
│   ├── runtime_event/       # normalized runtime events
│   ├── skill/               # skill registry, pull/cache, engine-run skill, iota-fun
│   ├── store/               # cache, approvals, session ledger, observability stores
│   └── telemetry/           # tracing and OpenTelemetry
├── iota-kanban/src/         # Kanban domain, event sourcing, dispatcher, worker, sync
└── iota-desktop/            # React frontend + Tauri backend
```

`iota-core` 刻意不碰任何 UI，CLI、TUI、daemon、desktop 才能共用同一份 runtime，不会出现「同一件事四个地方各写一遍」；`iota-sympantos-kanban` 单独成一个领域库，是因为不想让任务板的状态机逻辑和 ACP 编排搅在一起；`iota-cli` 和 `iota-desktop` 纯粹是两层不同的 presentation，谁也不该知道对方的存在。

这两个库也有明确的发布边界。外部项目依赖 `iota-sympantos-core`；源码里的 crate 名仍是 `iota_core`。Kanban 作为独立包发布，core 默认不拉入它，只有启用 `kanban` feature 才增加 `iota-sympantos-kanban` 依赖和对应 MCP tools。workspace 内部同时写 `version` 和 `path`：本地构建走源码目录，`cargo package` 后则从 registry 校验版本依赖。

## 运行时主线

![execution flowchart](../img_result/execution_flowchart.png)

*图：一次 prompt turn 的执行时序——左列初始化与并发锁（request_hash、skill 路由、fencing token），右列执行与完成（session ledger、记忆召回、`<iota-context>` 组装、ACP 调用、读循环、回写 episodic 记忆）。*

一次 prompt turn 的核心路径在 `IotaEngine::run()`：

```text
prompt
  -> request_hash
  -> load and match skills
  -> ensure session ledger
  -> prepare backend handoff
  -> begin execution cache record
  -> recall memory and render workspace concurrently
  -> compose <iota-context>
  -> ensure ACP client
  -> session/prompt
  -> collect RuntimeEvent
  -> persist token usage
  -> finish execution
  -> record session turn
  -> push working memory
  -> write episodic memory
```

这条链路看着长，背后的设计意图就两点。

**先把家里收拾好，再请客。** 在碰后端之前，iota 先把 memory recall、skill index、working memory、workspace 的 git 状态、backend handoff 全部拼进 `<iota-context>`，连同用户原始 prompt 一起递过去。这样无论后端是谁，看到的背景信息都是同一套——后端不需要、也不应该知道这套背景是怎么攒出来的。

**进来的千奇百怪，出去的只有一种。** ACP 那边的 streaming update、permission request、tool call、usage、complete、error，形状各不相同，到了 iota 这层全部映射成 `RuntimeEvent`。CLI/TUI/Desktop/observability 谁都不用去理解五个后端的协议差异，它们只认 `RuntimeEvent` 这一种语言。

### 关键路径上的取舍

沿着上面那条主线，有几个选择值得单独讲一下。

**并发预计算。** memory recall 和 workspace 状态采集（`render_workspace`）之间没有任何依赖关系，而且都是阻塞 I/O。`engine/prompt.rs` 里把它们分别包成 `memory_task` 和 `workspace_task`，用 `tokio::join!` 并发跑，把 prompt 的前置成本压缩到单个最慢操作的耗时。

**Trivial 快速通道。** 当用户只发了一句简短的话——不超过 80 个字符，而且不含 `iota_memory`、`remember`、`recall`、`skill` 这些关键词——`compose_minimal_prompt()` 会直接跳过 memory、skill、workspace 段，只保留 memory-tools、session、model、handoff 这几个轻量 section。一个简单的 ping 或者「这段代码什么意思」，不值得为它准备完整的背景板，白白消耗 token 还会让 LLM cache 失效。

**Deterministic memory answer 短路。** 当用户问的是「我叫什么名字」「我偏好什么语言」这类纯粹靠 recall buckets 就能回答的问题时，`deterministic_memory_answer()` 根本不会调用任何 ACP 后端，直接以本地 engine 角色的身份返回结果。这种路径零成本、零网络、零外部依赖——问记忆就该这么快。

**Engine-run skill 优先于后端。** `SkillRegistry::match_skill` 命中后，如果 `runner::run_engine_skill` 返回了结果，整个 turn 就在 iota 内部完成，不需要惊动后端。只有 advisory skill 才会让 prompt 继续走向 ACP。这么分是因为 deterministic 工具能给出可测试、可缓存、权限边界更清晰的执行路径。

**Memory 持久化意图校验。** 这一条比较较真：用户的 prompt 里明确带着「记住」或「持久化」的意图，但 ACP 输出里一条成功的 `iota_memory_write` tool result 都没有时，`run()` 会直接把这次 execution 标成 `Failed` 并抛错。用户说「记住」是带着契约预期的。要是模型回一句「好的，我记住了」就糊弄过去、底下却什么都没写，这种静默失败比直接报错危险得多——错觉会一直延续到用户某天发现记忆是空的。

**后端切换走 handoff，不转发完整历史。** 切后端时，只把 working memory 的摘要塞进 `<handoff>` 给新后端，而不是把整段对话历史原样重放。不同后端的 session 本来就是各管各的，一份摘要既够新后端接上文，又不至于让 token 账单失控。

## ACP 后端适配

![backend ipc stages](../img_result/backend_ipc_stages.png)

*图：Backend 调用的三阶段——上下文预处理（记忆召回、skill 匹配、capsule 组装）、JSON-RPC IPC（子进程 stdin/stdout 与 daemon TCP 边界）、后处理与执行路由（MCP 拦截、记忆写回、skill executor）。*

ACP 层位于 `crates/iota-core/src/acp`。它负责启动外部后端进程，通过 stdin/stdout 的换行分隔 JSON-RPC 2.0 驱动整套交互：

```text
initialize
  -> session/new
  -> session/prompt
  -> session/update ...
  -> session/request_permission? ...
  -> session/complete
```

当前支持五个后端：

| Backend | 默认命令 | 别名 | 特殊点 |
| :--- | :--- | :--- | :--- |
| Claude Code | `npx -y @agentclientprotocol/claude-agent-acp@0.32.0` | `claude`, `claude-code`, `claudecode` | 支持 Anthropic env 映射 |
| Codex | `npx -y @zed-industries/codex-acp@0.12.0` | `codex` | 通过 `-c` 追加 Codex provider/model 配置 |
| Gemini CLI | `npx -y @google/gemini-cli@0.41.2 --acp` | `gemini`, `gemini-cli` | 支持 `GEMINI_API_KEY` / `GEMINI_MODEL` |
| Hermes | `hermes acp` | `hermes`, `hermes-agent` | 不覆盖 `HERMES_HOME` |
| OpenCode | `npx -y opencode-ai@1.14.40 acp` | `opencode`, `open-code` | 使用 `OPENCODE_MODEL` |

起外部进程而不是接 SDK，是为了让 iota 能跟着各家迭代的节奏走：这些工具本身就是成熟的 coding agent，iota 只需要在它们现成的 ACP 边界上把上下文、权限、事件统一掉，犯不着重新实现每家 provider 的工具调用协议——吃力不讨好，而且追不上人家迭代的速度。

ACP 进程用 `tokio::process::Command` 启动，stdin/stdout/stderr 全部 piped，并设 `kill_on_drop(true)`。这让 Rust runtime 能在跨平台环境下把子进程收干净，不留孤儿进程。Windows 上 `normalize_command()` 会把 `npx` 改写成 `npx.cmd`。

### 执行幂等性与 fencing

`CacheStore::begin_execution_with_id()`（`store/cache.rs`）在每次 prompt turn 开始时做三件事，三件事各自解决一类问题：

**回收僵尸 running。** 把同一 `(backend, request_hash)` 下、`started_at` 早已超过 `running_ttl_secs` 但仍挂着 running 的旧记录强制标为 `failed`。daemon 崩了、进程被 kill，都不会留下永久挂起的状态。下一次同请求进来，自动就把场子清了，不需要额外起个监控进程去扫尸。

**同 id 幂等返回。** 如果调用方传入了 `requested_execution_id` 且其 `request_hash` 完全匹配，直接复用旧 id。daemon 重试或前端重连时不会产生重复的 execution 记录。

**同 hash 拒绝并发。** 同一 `(backend, request_hash)` 已有 running 记录时直接 `bail!("execution already running")`，避免同一请求被并发提交两次。

每条 execution 还会被分配单调递增的 `fencing_token = MAX(fencing_token) + 1`。fencing token 不参与「内容是否相同」的判断，它的唯一职责是给运维和回放工具一个严格全序的标识：事件回放或长任务恢复时，可以判断哪条记录是更晚一次写入，从而拒绝陈旧状态覆盖。

execution_id 是 UUID，表达不了全序；request_hash 又可能撞重——同一个 prompt 被用户发了多次。所以这三者各管一摊：UUID 给身份，hash 回答「是不是同一个请求」，fencing token 给时序。

## 配置系统

![configuration env mapping](../img_result/configuration_env_mapping.png)

*图：配置层次与后端环境变量映射——`~/.i6/nimia.yaml` 单一配置源经 backend adapter 映射为五个后端各自的环境变量，并驱动 context 选项与 MCP 注入。*

配置只从 `~/.i6/nimia.yaml` 读，不读项目级配置，也不做自动发现。这个决定在上面讲过了——能最大限度躲开误提交和跨项目污染。

主要 schema 在 `crates/iota-core/src/config`：

| 文件 | 职责 |
| :--- | :--- |
| `schema.rs` | `NimiaConfig` 和 `StoreConfig` |
| `backend.rs` | `BackendConfig`、readiness、command/env 映射入口 |
| `adapters.rs` | 五个后端的 `BackendAdapter` 实现 |
| `context.rs` | Context engine、MCP server、embedding、budget、threshold 配置 |
| `effective.rs` | 把 raw YAML 解析成带默认值的 effective config |
| `paths.rs` | `~/.i6/context` 下的 store path |
| `loader.rs` | `config_path()`、`read_config()`、`save_config()` |

模型配置通过 backend adapter 映射为各后端期望的环境变量：

| Backend | 关键映射 |
| :--- | :--- |
| Claude Code | `ANTHROPIC_API_KEY`、`ANTHROPIC_AUTH_TOKEN`、`ANTHROPIC_BASE_URL`、`ANTHROPIC_MODEL` |
| Codex | `OPENAI_API_KEY`、`ROUTER_API_KEY`、`OPENAI_BASE_URL`、`OPENAI_MODEL` |
| Gemini | `GEMINI_API_KEY`、`GEMINI_MODEL` |
| Hermes | `HERMES_INFERENCE_PROVIDER`、`HERMES_MODEL` 和 provider 原生 key/base URL |
| OpenCode | `OPENCODE_MODEL` |

Hermes adapter 的 `home_env_key()` 返回 `None`，意味着即使配置里写了 home，也不会覆盖 Hermes 自己的默认目录。这样设计是为了不破坏 Hermes 在不同平台上的既有目录约定。

## Context Fabric

Context Fabric 的核心类型是 `ContextEngine`。它把多个本地来源组装成一个 XML 风格的 `<iota-context>` capsule，各 section 按固定顺序排列：

```text
<iota-context>
  <memory-tools>...</memory-tools>   <!-- 1. 静态工具提示，最大化 LLM cache prefix 命中 -->
  <model>...</model>                 <!-- 2. 当前模型名称（如已配置） -->
  <skills>...</skills>               <!-- 3. skill index（如有匹配 skill） -->
  <memory>...</memory>               <!-- 4. 召回的持久记忆 -->
  <session>...</session>             <!-- 5. session_id、backend、cwd -->
  <handoff>...</handoff>             <!-- 6. 后端切换摘要（如有） -->
  <working-memory>...</working-memory> <!-- 7. 最近多轮摘要（如非空） -->
  <workspace>...</workspace>         <!-- 8. git status 变更文件（如非空） -->
</iota-context>

User request:
...
```

这个顺序由 `compose_effective_prompt()` 的实现决定。一眼就能看出它的编排思路：静态的、低频变化的 section 排在前面，动态的、每轮都可能变的 section 挤到最后。

Anthropic、OpenAI 这些 provider 做 prompt cache 都是以「prefix 不变」为复用条件的。memory-tools 文案、model 名称、skill index 这些东西多轮之间几乎不动，session id 在同一会话里也不变；但 handoff、working-memory、workspace 是每轮都可能变的。把易变的部分挤到最后，前缀就有最大概率命中 cache，单 turn 成本能直接掉一个数量级。这也是为什么 capsule 永远不落磁盘缓存——每次都重新拼，靠的是 provider 那边的 cache，而不是 iota 自己再维护一套缓存跟它抢生意。

`memory-tools` section 包含持久记忆工具的使用指引。当 MCP 工具可用时，还会注入 Kanban 工具提示，指导模型使用 `iota_kanban_create_task`、`iota_kanban_ready_task`、`iota_kanban_list_tasks` 管理任务，并明确 iota 是 Kanban DB 的唯一事实来源。

把上下文渲染成一段普通文本，是所有后端都能消化的最大公约数。五个后端的 prompt API 未必有同一套 system/developer/context slot；XML-like 的 tag 又让模型能轻松分清「背景数据」和「用户真正的请求」，不至于把上下文当指令误读。

Context Fabric 有预算控制，每类内容的上限都可在配置中调整：

| Budget | 默认用途 |
| :--- | :--- |
| `memory_chars` | 记忆召回内容 |
| `skills_chars` | skill index |
| `working_memory_chars` | 最近多轮摘要 |
| `workspace_chars` | git/workspace 状态 |
| `handoff_chars` | 后端切换摘要 |

实现上还有一个低延迟优化：trivial prompt 走 minimal capsule，跳过 memory、skill 和 workspace 的大段注入。判定条件前面讲过了——≤80 字符且不包含关键触发词。普通路径中 memory recall 和 `git status --short` 会并发执行，减少 prompt 前置等待。

## Memory 系统

![memory taxonomy lifecycle](../img_result/memory_taxonomy_lifecycle.png)

*图：记忆分类与生命周期——SQLite schema、三维分类（type/facet/scope）到六桶召回的映射，以及去重、TTL 过期、episodic 压缩三类生命周期控制。*

Memory 系统位于 `crates/iota-core/src/memory`，默认数据库 `~/.i6/context/memory.sqlite`。它把记忆从三个维度切分：三类 type、四类 semantic facet、四种 scope。

| 维度 | 值 |
| :--- | :--- |
| Type | `semantic`、`episodic`、`procedural` |
| Semantic facet | `identity`、`preference`、`strategic`、`domain` |
| Scope | `user`、`project`、`session`、`global` |

召回时整理成六个 bucket：

![memory recall buckets](../img_result/memory_recall_buckets.png)

*图：六桶记忆召回机制——召回查询输入、六路并行分桶过滤（含阈值与 LIMIT），以及 keyword / vector / hybrid 三种搜索模式与输出组装。*

| Bucket | 来源 |
| :--- | :--- |
| `identity` | semantic + identity |
| `preference` | semantic + preference |
| `strategic` | semantic + strategic |
| `domain` | semantic + domain |
| `procedural` | procedural |
| `episodic` | episodic |

分桶是为了让 agent 分得清「用户是谁」「偏好什么」「项目长期目标是什么」「领域事实是什么」「哪些是可复用的流程」和「最近刚发生了什么」。这几件事的保质期完全不同——身份和偏好该长期稳定，某次会话的痕迹过几天就该过期。把所有记忆揉成一串无结构文本，模型根本分不清哪些该信、哪些该忘。

存储实现有几个值得细说的选择：

- 使用 SQLite，优先 FTS5 做全文检索。
- 用 SHA-256 content hash 做去重。
- 支持 `auto`、`add`、`update`、`none` 四种 merge mode。
- 支持 TF-IDF embedding 的 vector/hybrid search；配置了 embedding API 时，`EmbeddingEngine` 可走外部 embedding 服务。
- `search_vector()` 的混合评分公式为 `score = 0.65 × cosine_similarity + 0.20 × token_overlap + 0.15 × confidence`，过滤掉 score ≤ 0.05 的结果。
- `search_hybrid()` 合并 keyword 和 vector 两路结果，vector 结果权重 1.2×，keyword 结果权重 1.0×，按 reciprocal rank 加权后排序。
- 插入前先计算 embedding，再拿数据库 mutex——避免外部 embedding 调用阻塞 SQLite 连接。

关于这组权重：纯向量相似度容易漂，长得像、意思却不一样的记忆会被拉到最顶上。引入 token_overlap 是为了把关键词信号重新拉回来，引入 confidence 则是让用户或工具明确标过的高置信记忆优先浮上来。0.05 的阈值本质是一个噪声地板——低于它的结果几乎不可能被模型用上，不如全丢，还能给 capsule 减减负。vector 1.2× vs keyword 1.0× 的比值是经验值，可通过 `RecallThresholdsConfig` 调整。

至于「先 embedding 再拿 mutex」这个顺序——embedding 可能要走外网调用，耗时不可预测。要是先拿住 SQLite 的 mutex 再去调 embedding，那整段网络等待期间所有其他读写都得在外面排队。把网络 I/O 挪到临界区外面，是 SQLite 单连接架构下的常规自卫动作。

模型写 memory 的主入口不是直接改库，而是 MCP 工具 `iota_memory_write`。上下文里要求模型先加载 `iota-memory-taxonomy` skill，再按 taxonomy 写入原子 memory record。这样把「判断该记什么」的策略和「存储 schema 校验」分开，各自不越界。

## Skill 与 iota-fun

![skill system pipeline](../img_result/skill_system_pipeline.png)

*图：技能加载、匹配与执行——多个 skill 加载根、trigger 匹配与后端兼容性检查、模板占位符渲染，以及 advisory 与 MCP/engine-run 两种执行模式。*

Skill 层位于 `crates/iota-core/src/skill`。它加载 `.md` 或 `.yaml` skill manifest，根据 trigger 匹配 prompt，支持两种使用方式：

| 模式 | 说明 |
| :--- | :--- |
| Advisory | 把 skill metadata/body 注入上下文，由后端模型阅读并执行 |
| MCP / engine-run | iota runtime 本地执行确定性工具，必要时短路外部 ACP 调用 |

Skill roots 来自配置，默认包含 `~/.i6/skills` 和 `./.iota/skills`。用户级 skill 装个人习惯，workspace skill 装项目约定。仓库内还内置了 core skill `iota-memory-taxonomy`，用于指导 memory 的分类和写入粒度。

`iota-fun` 是一个 MCP stdio server，能跑七种语言的代码片段：C++、Go、Java、Python、Rust、TypeScript、Zig。它适合装那些确定性的小工具，比如示例 skill `skills/pet-generator`。为什么不干脆让所有 skill 都自由跑 shell？因为 deterministic 的工具能给出可测试、可缓存、权限边界更清楚的本地能力——这些是「让模型随便执行 shell」换不来的。

## MCP 工具层

MCP 层位于 `crates/iota-core/src/mcp`。它既提供 stdio server，也提供 ACP stream 中的 tool-call interceptor。

当前 `iota-context` MCP server 暴露的工具由 `tool_dispatch.rs` 的 registry 统一管理：

| Tool | 用途 |
| :--- | :--- |
| `iota_memory_search` | 搜索本地 memory |
| `iota_memory_write` | 写入一条 memory record |
| `iota_skill_search` | 搜索可用 skill index |
| `iota_skill_load` | 读取指定 skill 完整内容 |
| `iota_session_summary` | 读取 session 摘要 |
| `iota_handoff_publish` | 发布 backend handoff 摘要 |
| `iota_handoff_read` | 读取 handoff |

工具派发集中到 `tool_dispatch.rs`，是因为同一套业务逻辑要被两个入口复用：stdio MCP server 是后端通过 `mcpServers` 正常调用工具的那条路，ACP router 是后端输出中出现 iota tool-call 风格事件时 runtime 拦截执行的那条路。集中派发才能避免「stdio server 和 router 行为不一致」这种隐性 bug。

### Router 的拒绝策略

`mcp/router.rs` 的 `route_tool_call()` 对工具调用分四类处理：

| 工具来源 | 处理 | 返回 |
| :--- | :--- | :--- |
| `tool_dispatch::REGISTRY` 已注册的 iota 工具 | 本地执行，包成 MCP `content` envelope | `isError:false` + structured content |
| `iota-fun` 七语言 sandbox | 走 `skill::fun::run_tool` 本地执行 | `isError:false` |
| 以 `iota_` 前缀但未注册 | 拒绝 | `isError:true`，文案 `not routable in this context` |
| 其它任意外部 MCP 工具 | 拒绝 | `isError:true`，文案 `denied by iota policy` |

外部 MCP 工具默认拒绝看起来保守，但放在 ACP stream 这个上下文里其实是最自然的选择：在 stream 里拦到的工具调用，本质上是后端在「借 iota 的身份」去干某件事。但 iota 在这个位置并不是 MCP 客户端，既没有连到外部 server，也没有用户授权语义——这时候默认放行才是真正危险的。明确拒绝、并用 `isError:true` 把原因回传，让模型看得见为什么被拦、自己调整策略，远比静默吞掉调用要可解释。

## RuntimeEvent 与可观测性

![observability architecture](../img_result/observability_architecture.png)

*图：可观测性架构——iota 把 logs / traces / metrics 三类信号送入 OpenTelemetry Collector，再分发到 Loki、Jaeger、Prometheus、Grafana；左下角是无 Docker 时的本地 stderr、`~/.i6/logs/` 与 SQLite 回退路径。*

`RuntimeEvent` 是 iota 的统一事件语言，位于 `crates/iota-core/src/runtime_event`。它覆盖输出、状态、日志、工具调用、工具结果、错误、扩展事件、token usage、memory、approval request 和 approval decision。

让 UI 直接消费 ACP 原始事件看似简单，实际上要给 UI 层硬塞五套不同的协议适配。五个后端 adapter 的 usage、tool update、permission 形状都可能不一样，每加一个后端都得改一轮 UI。归一之后上层只面对一种事件：CLI 用 `--log-events` 打印，TUI 把 output、approval 和 token meta 渲染进 transcript，Desktop 把事件折叠到 inspector，ObservabilityStore 只关心 `TokenUsage`，OpenTelemetry 在同一套字段上打 span/metric/log——各取所需，互不干扰。

Token usage 是当前最重要的观测对象。`RuntimeEvent::TokenUsage` 会尽量归一化各 provider 的不同字段：

| 字段 | 含义 |
| :--- | :--- |
| `input_tokens` | 输入 token |
| `cache_read_input_tokens` | provider cache read |
| `cache_creation_input_tokens` | provider cache write |
| `output_tokens` | 输出 token |
| `thinking_tokens` | reasoning/thought token |
| `provider_reported_total_tokens` | provider 原始 total |
| `normalized_total_tokens` | iota 计算或归一化后的 total |

归一化的逻辑按 provider 不同：Anthropic 取 `input + cache_read + cache_creation + output + thinking` 之和，OpenAI/Gemini/adapter 优先用 `provider_reported_total_tokens`，没有的话用 `input + output + thinking + tool_use_prompt`。

`ObservabilityStore` 写入 `~/.i6/context/events.sqlite`。同一 execution 里如果同时有 streaming usage 和 final usage，查询层会按完整度选择 canonical record，避免 summary 被重复计数。

## CLI 与 TUI

CLI 入口在 `crates/iota-cli/src/cli/mod.rs`。当前命令族：

| 命令 | 作用 |
| :--- | :--- |
| `iota` | 进入 TUI |
| `iota run [backend] [options] <prompt>` | 单次 prompt |
| `iota check [--daemon]` | 输出后端配置和 readiness |
| `iota bench <cold\|warm>` | 冷/热启动 benchmark |
| `iota mcp <context\|fun>` | 启动 MCP server |
| `iota skill pull <source> [name]` | 拉取 skill |
| `iota kanban ...` | Kanban board/task/dispatch/sync |
| `iota observability ...` | token/log/trace/metric 查询 |
| `iota __daemon` | 内部 daemon 入口 |

TUI 在 `crates/iota-cli/src/tui`。它不是简单 stdin prompt，而是一个完整的 ratatui 应用：多行输入支持 Unicode grapheme 光标；有 kill buffer、Ctrl+U/Ctrl+W、Alt+B/Alt+F、Ctrl+R 历史搜索；输出做 markdown 渲染和 scrollback；流式输出增量渲染；approval 有浮层确认；Ctrl+T 进 pager、`?` 出帮助、两次 Ctrl+C 确认退出；还有 tab queue 让后端运行期间可以缓存下一条输入，以及 `/kanban` 和 `/memory` 本地 slash command。

TUI 和 desktop 面对 engine 的方式之所以不同，是因为各自的物理形态不一样：TUI 是单进程的终端工具，直接抱住 IotaEngine 能省掉一层 IPC；desktop 则要管 GUI 生命周期、要能自动拉起 daemon、还要跨窗口共享事件流，走 daemon-first 才接得上。

## Daemon 热路径

![code call chains](../img_result/code_call_chains.png)

*图：代码调用链路——一次 prompt 从入口 `main.rs` 出发，经命令分发后分为直连 ACP 路径与 daemon TCP 路径，跨越 git 子进程、ACP 子进程、MCP sidecar、SQLite 与 TCP socket 等运行时边界。*

Daemon 位于 `crates/iota-core/src/daemon`，默认监听 `127.0.0.1:47661`。它提供两套 JSON-line 协议：

| 协议 | 使用方 | 形状 |
| :--- | :--- | :--- |
| Legacy prompt protocol | CLI `--daemon`、bench、check warm | 一次请求，一次响应 |
| Desktop protocol v2 | Tauri desktop | Hello handshake，多消息 streaming turn |

核心类型是 `EnginePool`。它按 cwd 复用 `IotaEngine`，而 `IotaEngine` 内部按 `(backend, cwd)` 复用 ACP client。这样 warm path 能跳过昂贵的 process spawn、initialize 和 session/new。

按 cwd 分池是水到渠成的——coding agent 的上下文、权限、MCP server、workspace state 都是和目录绑在一起的。跨 cwd 复用同一个 backend session，很可能把项目 A 的上下文泄漏进项目 B。

`EnginePool::engine_for(cwd)` 用 `BTreeMap<EngineKey, Arc<Mutex<IotaEngine>>>` 做按 cwd 复用。同一 cwd 的多次请求复用同一个 engine，从而复用 ACP client、session_id、working memory 和 handoff 状态。它不按 backend 分池——engine 内部已经按 `(backend, cwd)` 二级 key 做了 ACP client 复用，外层再按 backend 分反而会破坏「同一 cwd 内切后端不丢上下文」的能力。它也不做 session-level GC——单用户 daemon 通常并发的 cwd 数量就那么几个，定期 GC 反而可能清掉马上要复用的热连接，不如让进程退出时 OS 一次性回收。

Desktop protocol v2 支持的消息类型包括 Hello、StartTurn、RespondApproval、CancelTurn、GetConfig、SaveBackendModel、CheckBackend、GetObservabilitySummary、GetMemoryContextSnapshot，server 侧回发 TextChunk、TurnEvent、ApprovalRequested、TurnCompleted、TurnFailed、TurnCancelled 等消息。

## Kanban 长任务系统

![kanban state machine event sourcing](../img_result/kanban_state_machine_event_sourcing.png)

*图：Kanban 状态机与事件溯源——左侧 triage→todo→ready→running→done→archived（含 blocked）状态迁移，中间 append-only 事件存储与物化表，右侧 dispatcher / worker / shadow 执行管线。*

`iota-sympantos-kanban` 是一个 event-sourced task board。它的状态机不长，但覆盖了真实任务流转的几种核心情况：

```text
triage -> todo -> ready -> running -> done -> archived
                         \-> blocked -> ready
                                      \-> done
running -> ready  # claim expired
```

AI 编程任务经常不是一次 prompt 能收工的——它们要拆分、排队、执行、观察、失败恢复、同步。Kanban 给 agent runtime 一个结构化的任务层，而不是把所有长期目标都填进随时会被截断的对话历史里。

核心模块各管一摊：

| 模块 | 职责 |
| :--- | :--- |
| `types.rs` | Task、Board、Run、Comment、Link、KanbanEvent |
| `store.rs` | `KanbanStore` trait |
| `sqlite_store.rs` | SQLite event-sourced implementation |
| `state_machine.rs` | 合法状态迁移 |
| `dispatcher.rs` | 扫描 ready task，调度 worker |
| `worker.rs` | 启动/杀死 Hermes `-z` worker |
| `shadow.rs` | 把单个 task materialize 到 Hermes 兼容 shadow DB |
| `bridge.rs` | `specify` / `decompose` 高级编排 |
| `event_sync.rs` | export/import/serve/pull/push event bundle |

Shadow DB 是整个设计里最巧妙的一层。

![kanban event sync bridge](../img_result/kanban_event_sync_bridge.png)

*图：Kanban 分布式同步与桥接——左侧跨节点 export/import/serve/pull/push 事件同步，中间 decompose/specify 高级桥接，右侧与 TUI、CLI、engine 的集成点。*

iota 的主库是事实来源，Hermes 不直接写主库。调度时流程是这样的：`ShadowMaterializer` 为 task 创建 `shadows/{task_id}/kanban.db`，`WorkerHandle` 启动 `hermes -z` 并把 `HERMES_KANBAN_DB` 指向这份 shadow DB，Hermes 在 shadow DB 里读取任务、写入完成事件，`ShadowWatcher` 轮询 shadow DB 把终态同步回主库，成功后清理 shadow directory。

这样既能复用 Hermes 现有的 Kanban 能力，又把 iota 主库和外部 worker 彻底隔开。Hermes 的 Kanban 实现假设自己拥有那个 DB——claim、心跳、worker_pid 写回都是直接更新行——让它直连主库等于把「事实来源」分一半给外部进程。Shadow + watcher 的回写路径让 iota 始终是 Kanban 主库的唯一 writer，从根本上避免了「两边事实漂移」和「主库被 worker crash 损坏」这两类风险。

## Desktop 工作台

![desktop tauri architecture](../img_result/desktop_tauri_architecture.png)

*图：iota-desktop（Tauri）架构与通信流——React 前端经 Tauri command 调用 daemon client，通过 TCP 协议连接 daemon 与 IotaEngine，事件再反向回流到前端 turnReducer 渲染。*

Desktop 在 `crates/iota-desktop`，由 React 前端和 Tauri Rust 后端组成，当前实现是 daemon-first：

```text
React ChatWorkbench
  -> src/api.ts invoke/listen wrappers
  -> Tauri commands in src-tauri/src/lib.rs
  -> daemon_client::connect_or_start()
  -> iota __daemon desktop protocol v2
  -> EnginePool / IotaEngine / ACP backend
  -> daemon-message / daemon-client-error window events
  -> turnReducer updates transcript and inspector state
```

前端主要组件：`ChatWorkbench.tsx` 是主 shell，管后端选择、prompt form、Chat/Config 视图、daemon 状态和 inspector 宽度；`ConfigPanel.tsx` 负责后端模型配置编辑（API key 打码显示）；`RightInspector.tsx` 承载 turn 详情、approval、cancel、observability、memory/context 各 tab；`MemoryContextWorkspace.tsx` 提供只读 memory bucket 和 runtime context capsule 浏览；`turnReducer.ts` 折叠 daemon stream message 与 RuntimeEvent；`api.ts` 封装 Tauri invoke/listen。

Tauri command 分两类：daemon-backed 的命令走 `get_config`、`save_backend_model`、`submit_prompt`、`cancel_turn`、`handle_approval` 等，direct Kanban 的命令走 `list_boards`、`list_tasks`、`create_task`、`transition_task` 等。当前 React workbench 已在 `RightInspector` 的 Kanban tab 挂载 `KanbanWorkspace`，可通过这些 Tauri commands 刷新 board、task、comment、link 和 run；数据库在 `~/.i6/kanban/iota.db`。

Desktop 选择走 daemon 而不是直接创建 `IotaEngine`，是因为 daemon 能把热路径、配置保存、approval registry、turn cancel、runtime event streaming 全部捏在一处。Tauri 只当个本地 UI bridge 就好，不该变成第二套 runtime——一个系统里两套 runtime，早晚要在状态上对不齐。

## 存储与数据边界

所有本地持久化都在 `~/.i6` 下。SQLite 文件现在很干净：

| Store | 默认路径 | 内容 |
| :--- | :--- | :--- |
| `MemoryStore` | `~/.i6/context/memory.sqlite` | memory taxonomy、FTS、embedding、recall |
| `CacheStore` | `~/.i6/context/events.sqlite` | execution lifecycle、running lock、fencing |
| `ObservabilityStore` | `~/.i6/context/events.sqlite` | token usage events、summary、percentiles |
| `SessionLedger` | `~/.i6/context/store.sqlite` | logical sessions、turns、backend handoff |
| `ApprovalStore` | `~/.i6/context/store.sqlite` | approval request/decision |
| `SqliteKanbanStore` | `~/.i6/kanban/iota.db` | board/task/comment/link/run/events |

SessionLedger 和 ApprovalStore 共享同一个 `store.sqlite`——各自在 `open()` 时跑自己的 DDL，`CREATE TABLE IF NOT EXISTS` 是幂等的，互不干扰。SQLite 连接通过 `Arc<Mutex<Connection>>` 共享，设了 WAL 和 `synchronous=NORMAL`。这套配置恰好契合本地单用户 agent runtime：部署简单、随手可备份、出事能直接打开库调，压根不需要外部数据库。

安全边界很明确：不提交 `~/.i6/nimia.yaml`；文档和调试输出里打码 API key/token；`--show-native` 可能暴露原生协议内容，只用于本地调试；approval 请求会通过 TUI/Desktop 交互确认；iota 自有工具可按白名单自动批准。

### Approval 的维度分离

`store/approvals.rs` 的 `ApprovalDimension` 把工具调用按风险维度拆开，而不是只给一个是或否：

| 维度 | 触发条件 |
| :--- | :--- |
| `Shell` | 任意命令执行 |
| `FileOutsideWorkspace` | 写到 cwd 之外的路径 |
| `Network` | 发起对外网络访问 |
| `McpExternal` | 调用非 iota 管理的外部 MCP 工具 |
| `PrivilegeEscalation` | 提权类操作 |

不同维度的风险等级、补救方式、用户教育成本都不一样。Shell 是高频动作，用户大多数时候会批准；写到工作区外的文件几乎总是误操作，应当醒目告警；网络访问要看目标是不是已知 provider。把维度暴露给 UI，UI 才能用差异化措辞告诉用户「这次为什么需要批准」，也才能基于维度建自动批准白名单。

`approval_requests` 和 `approval_decisions` 分两张表存：前者记录「被拦截过什么」，后者记录「用户/策略最终怎么决定」。即使没有 decision 记录，也能从 requests 表里看到模型曾经试图做什么——这是审计意义大于性能意义的设计。

## 跨平台设计

iota 要求 Windows/macOS/Linux 同时可用。代码中已经体现出一套铁律：home 目录走 `dirs::home_dir()`，路径用 `Path`/`PathBuf` 不手写分隔符，Windows 上 `npx` 归一化为 `npx.cmd`，子进程一律 `Stdio::piped()` + `kill_on_drop(true)`，Hermes home 不由 iota 覆盖，配置模板里的 `~/...` 运行时由 `expand_home_path()` 展开。

agent runtime 会起外部进程、读写本地库、注入 MCP server，还得常驻成 daemon。平台差异这东西，只要往后拖到「调用现场再处理」，最后几乎都会变成最难查的那类 bug。与其如此，不如在代码里一次性把它吃掉。

## Docker 与外部观测栈

Docker 方案不是运行时的必须条件，而是把 daemon 和观测依赖放进可重复环境中。它适合做集成测试、长时间运行 daemon、或在固定容器里连接宿主机 workspace。

仓库里两组 compose：`docker/docker-compose.yml` 启动 iota-daemon 和完整观测栈，`docker/observability/docker-compose.yml` 只启动 OpenTelemetry Collector、Jaeger、Prometheus、Loki、Grafana 这些观测服务。`docker/Dockerfile` 用 `rust:1.95-slim-bookworm` 构建 release 版 `iota`，runtime 镜像还装了 `git`、`curl`、`sqlite3`、`nodejs`、`npm`——因为 Claude Code、Codex、Gemini、OpenCode 的 ACP adapter 都可能是通过 `npx` 拉起来的，没有 Node 这几个后端根本启不了。

默认端口方面，iota daemon 在 `47661`，OTLP collector 在 `4317`/`4318`，Jaeger 在 `16686`，Prometheus 在 `9090`，Loki 在 `3100`，Grafana 在 `3000`。

外部观测和本地 SQLite observability 各有各的位置：SQLite 适合离线、低依赖、按 execution 查 token usage；OpenTelemetry/Loki/Jaeger/Prometheus 适合跨进程、跨时间窗口的运行时排障。问题不同，所以两者都留。

## 扩展开发指南

### 新增 ACP 后端

新增后端需要同时处理协议枚举、配置、命令、环境变量和文档。最小步骤：

1. 在 `crates/iota-core/src/acp/backend.rs` 添加 `AcpBackend` 变体、alias、`Display` 和 `ALL_BACKENDS`。
2. 在 `crates/iota-core/src/config/adapters.rs` 增加 `BackendAdapter`，定义默认 ACP command、home env、model env 映射和必要的追加参数。
3. 在 `crates/iota-core/src/config/schema.rs` / `backend.rs` 接入新的 backend config section。
4. 在 `nimia.yaml.template` 添加配置模板，注意不要写真实 API key。
5. 更新 `docs/command.md`、`docs/architecture.md`、本书和相关 `SKILL.md`。
6. 增加独立 `*_tests.rs`，覆盖 alias 解析、env 映射、command normalization 和 session/new 参数。

扩展点放在 config adapter 而不是散落在 engine 里，是因为 engine 只该知道一件事：「我要一个能启动的 ACP client」。后端差异本质上是边界适配问题，放进 adapter 才不会每加一个后端就去污染 prompt 主线。

### 新增 MCP 工具

新增 iota 工具应优先走 `crates/iota-core/src/mcp/tool_dispatch.rs`：实现 `McpTool`，在 `McpToolRegistry::new()` 注册，把真实依赖放进 `ToolContext` 避免工具内部自己随意打开数据库或读配置，为 stdio server 和 router 共用路径添加测试，更新 Context Fabric 中的工具提示并必要时新增 skill 指导模型何时调用。

扩展点必须落在 tool dispatch，是因为 stdio MCP server 和 ACP router 都依赖它。只改 server 会造成「后端通过 MCP 能用、router 拦截却不能用」这种隐性不一致——而这类 bug 恰恰是最难复现的。

### 新增 Skill

Skill 是最轻量的扩展方式。普通 advisory skill 只需要一个带 frontmatter 的 `SKILL.md`；engine-run skill 还需要声明 MCP server、工具和输出模板。建议 trigger 要具体避免普通 prompt 误命中，deterministic 能力放进 MCP sidecar、模型判断放进 skill body，输出模板保持稳定便于测试，需要持久化知识时复用 `iota-memory-taxonomy` 而不要在 skill 中发明另一套 memory 分类。

## 调试

![debugging workflow](../img_result/debugging_workflow.png)

*图：调试工作流——在 VS Code + CodeLLDB 下对 CLI、TUI、engine、ACP 设置断点，单步执行、检查变量并跨越 ACP 子进程边界排查问题。*

调试细节见 `docs/debugging.md`：`断点入口` 一节列出了 CLI 分发、engine prompt path、ACP client/wire/session、daemon server、Kanban store 等推荐断点位置；`RUST_LOG=debug`、`RUST_BACKTRACE=1`、`IOTA_LOG` 与 `~/.i6/logs/` 下的本地日志文件用于排查 ACP 协议与子进程边界问题。

## 测试与工程约束

本仓库有一条测试约束：Rust 单元测试放在独立 `*_tests.rs` 文件中，源文件用 `#[cfg(test)] #[path = "module_tests.rs"] mod tests;` 引用。测试文件使用 `use crate::...` 绝对路径，不使用 `use super::*`，测试函数直接放在文件顶层。

这个约束的出发点是：项目模块多、边界多，拆出独立测试文件后生产代码更短，测试命名更稳定，AI coding 工具改实现时也能第一时间定位到对应测试。

常用验证命令：

```bash
cargo test
cargo check --offline
cd crates/iota-desktop && npm test && npm run build
```

Desktop 开发用 `cd crates/iota-desktop && npm run dev:clean`。`dev:clean` 会停止旧 daemon，构建当前 workspace 的 `iota` CLI，并设置 `IOTA_CLI_PATH`，避免 Tauri 连到 PATH 中的旧 binary。

## 文档地图

当前文档分三层：

| 层级 | 位置 | 用法 |
| :--- | :--- | :--- |
| 当前手册 | `docs/iota book.md`、`architecture.md`、`code-call-chains.md`、`command.md`、`observability.md`、`debugging.md`、`docker.md` | 读当前系统行为和操作方式 |
| 模块上下文 | 各 crate/module 的 `SKILL.md` | 给 AI coding 工具快速理解局部模块 |
| 历史记录 | `gefsi/` | 保留实验结果 |

历史记录 `gefsi/` 留着是因为很多实现决策都是从实验和计划里长出来的——daemon-first desktop、token usage 归一化、Kanban shadow DB。它们不是最新的命令手册，却能解释当前代码为什么长成这个样子。

## 从代码继续阅读

如果你想从源码深入，建议按这个顺序来：

1. `crates/iota-cli/src/cli/mod.rs`：看用户命令如何进入 runtime。
2. `crates/iota-core/src/engine/prompt.rs`：看一次 prompt turn 的真实主线。
3. `crates/iota-core/src/acp/client.rs` 和 `stream_reader.rs`：看 ACP 进程与流式事件。
4. `crates/iota-core/src/context/mod.rs`：看 `<iota-context>` 如何被渲染。
5. `crates/iota-core/src/memory/store.rs`：看 memory taxonomy、去重、search 和 recall。
6. `crates/iota-core/src/mcp/tool_dispatch.rs`：看 MCP 工具的统一业务实现。
7. `crates/iota-core/src/runtime_event/mod.rs`：看原生协议如何归一成事件。
8. `crates/iota-core/src/daemon/proto.rs` 和 `desktop.rs`：看 daemon IPC。
9. `crates/iota-kanban/src/sqlite_store.rs`、`dispatcher.rs`、`shadow.rs`：看长任务系统。
10. `crates/iota-desktop/src/components/ChatWorkbench.tsx` 和 `src-tauri/src/lib.rs`：看桌面端如何复用 daemon runtime。

本书的目标不是替代源码，而是给出一张可靠的地图。说到底，iota-sympantos 的核心思想就一句话：把 AI Agent后端当成可替换的执行器，把上下文、记忆、技能、工具、权限、事件和任务管理都沉到本地这层 Rust runtime 里统一治理。后端可以换，你资产化的这套本地能力不会跟着走。
