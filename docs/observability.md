# Observability

本文记录当前实现：RuntimeEvent、token usage 持久化、`iota observability` 本地查询、OpenTelemetry、Loki 和 Jaeger 边界。

## 命令入口

| 命令 | 数据源 | 用途 |
| :--- | :--- | :--- |
| `iota run --timing ...` | `AcpPromptTiming` | 输出 route、spawn、init、session、prompt、total 耗时 |
| `iota run --log-events ...` | `AcpPromptOutput.events` | 输出本轮 normalized `RuntimeEvent` |
| `iota observability logging recent --limit N` | `ObservabilityStore` | 最近 execution-level token 记录 |
| `iota observability logging events <execution_id>` | `ObservabilityStore` | 单个 execution 的 raw token usage events |
| `iota observability tokens recent --limit N [--json]` | `ObservabilityStore` | 最近 token usage 明细 |
| `iota observability tokens summary --since 1h [--json]` | `ObservabilityStore` | backend 维度 token 均值、标准差、CV 和计数 |
| `iota observability tokens export --format json` | `ObservabilityStore` | 导出 token usage 明细 |
| `iota observability metrics --prometheus` | `ObservabilityStore` | 输出 Prometheus 文本格式的本地 token 聚合指标 |
| `iota observability logs <execution_id>` | Loki HTTP API | 按 `iota_execution_id` 查询远端日志 |
| `iota observability trace <trace_id>` | Jaeger HTTP API | 查询 trace span 并打印简要瀑布 |
| `iota logs <execution_id>` | Loki HTTP API | 顶层别名 |
| `iota trace <trace_id>` | Jaeger HTTP API | 顶层别名 |

环境变量：

- `IOTA_LOKI_URL`，默认 `http://localhost:3100`
- `IOTA_JAEGER_URL`，默认 `http://localhost:16686`
- `OTEL_ENABLED=true` 启用 OTLP 导出
- `OTEL_EXPORTER_OTLP_ENDPOINT`，默认 collector endpoint 为 `http://localhost:4317`

## RuntimeEvent

ACP update、complete、permission、usage、tool 和 error 会被归一化为 `RuntimeEvent`，随 `AcpPromptOutput.events` 返回。CLI 用 `--log-events` 打印；TUI 和 desktop 用它更新 transcript、approval、token breakdown、tool call 和 inspector 状态。

主要事件：

```text
Output
State
Log
ToolCall
ToolResult
Error
Extension
TokenUsage
Memory
ApprovalRequest
ApprovalDecision
```

## Token Usage

`RuntimeEvent::TokenUsage` 统一承载 OpenAI、Anthropic、Gemini 和 adapter-only usage 字段。

| 字段 | 含义 |
| :--- | :--- |
| `input_tokens` / `output_tokens` | 输入和输出 token |
| `cache_tokens` | `cache_read_input_tokens` 的别名字段 |
| `cache_read_input_tokens` | 缓存命中的输入 token |
| `cache_creation_input_tokens` | 缓存写入的输入 token |
| `thinking_tokens` | reasoning / thoughts / thinking token |
| `tool_use_prompt_tokens` | 工具结果回灌 token，provider 支持时填充 |
| `total_tokens` | 中间计算字段：`provider_reported_total_tokens` 或 `input + output + thinking` 的和 |
| `provider_reported_total_tokens` | provider 或 adapter 原样上报 total |
| `normalized_total_tokens` | iota 归一化后的 total，字段不足时为 `None` |
| `raw_payload` | 原始 usage JSON |

`normalized_total_tokens` 的计算逻辑因 provider 而异：

- **Anthropic**：`input + cache_read + cache_creation + output + thinking`
- **OpenAI / Gemini / adapter**：优先使用 `provider_reported_total_tokens`；不可用时回退为 `input + output + thinking + tool_use_prompt`

`ObservabilityStore` 使用 `~/.i6/context/events.sqlite`。同一 execution 中如果同时存在 streaming `usage_update` 和 final `usage`，查询层优先选择字段更完整的 final usage。字段缺失不按 0 计入 summary。

## Metrics

`telemetry::metrics` 注册进程内 OpenTelemetry meter：

全部指标定义在 `crates/iota-core/src/telemetry/metrics.rs` 的 `IotaMetrics` 一处，记录方法都是 `record_*`。

### 执行与 token

| 指标 | 类型 | 记录位置 | 含义 |
| :--- | :--- | :--- | :--- |
| `iota.execution.count` | Counter | engine | execution 结束计数，按 status 记录 |
| `iota.prompt.queued` | UpDownCounter | TUI | prompt 队列长度变化 |
| `iota.token.usage.count` | Counter | observability store | token usage 事件计数 |
| `iota.token.input` / `iota.token.output` / `iota.token.total` | Counter | observability store | 输入 / 输出 / 归一化 total token |
| `iota.prompt.duration` | Histogram | engine | prompt 处理耗时（秒） |
| `iota.init.duration` | Histogram | ACP | 后端 initialize 耗时（秒） |

### 上下文组装（§7）

| 指标 | 类型 | 属性 | 含义 |
| :--- | :--- | :--- | :--- |
| `iota.context.section_tokens` | Histogram | `section` | 每个胶囊区段的估算 token 数 |
| `iota.context.section_trimmed` | Counter | `section` | 该区段为放进预算丢弃了内容 |
| `iota.context.total_tokens` | Histogram | — | 整个注入胶囊的估算 token 数 |

`section` 取值：`skills` / `memory` / `handoff` / `working-memory` / `workspace`。
`section_trimmed` 持续增长说明对应的 `*_chars` 预算配得太小。

### 存储（§5 / §8）

| 指标 | 类型 | 属性 | 含义 |
| :--- | :--- | :--- | :--- |
| `iota.storage.degraded` | Counter | `store`, `category` | 写入失败后降级为 degraded 事件而未失败请求 |
| `iota.db.lock_wait` | Histogram | `statement`, `kind` | 等待池内连接的时间（秒） |
| `iota.db.slow_query` | Histogram | `statement`, `kind` | 超过慢查询阈值的语句耗时（秒） |
| `iota.db.reader_fallback` | Counter | `reason` | 读操作因无可用读连接而落到 writer 连接 |

`category` 取值：`constraint` / `locked` / `unavailable` / `other` / `maintenance`。
`reader_fallback` 增长说明进程的文件描述符预算偏紧，可用 `IOTA_SQLITE_READ_CONNECTIONS` 调整读池上限。

### daemon 引擎池（§4）与其他

| 指标 | 类型 | 属性 | 含义 |
| :--- | :--- | :--- | :--- |
| `iota.pool.size` | Gauge | — | daemon 当前持有的 engine 数 |
| `iota.pool.eviction` | Counter | `reason` | 为满足上限而回收的 engine |
| `iota.pool.queue_wait` | Histogram | — | turn 等待所属 workspace 执行槽的时间（秒） |
| `iota.sync.rejection` | Counter | `reason` | Kanban sync 在处理前被拒的请求（§2） |
| `iota.sandbox.limit` | Counter | `limit` | iota-fun 执行因资源上限被终止（§3） |

默认 CLI 只初始化本地 tracing，不要求 collector。Docker compose 会提供 OpenTelemetry Collector、Jaeger、Prometheus、Loki 和 Grafana。

## 日志边界

- 工程日志：`tracing`、file appender、stderr layer，用于排查程序自身行为。
- 日志脱敏：stderr layer 的 writer 是 `telemetry::redact::RedactingStderr`，每条日志出站前过一遍 `redact()`。
  两层机制：`register_secret()` 在密钥进入进程时登记字面值（后端 API key 写入子进程环境时、daemon token 加载时），
  之后无论以什么形式出现都会被替换；同时按键名形状匹配 `*_API_KEY=` / `"api_key":` / `authorization: Bearer` /
  `cookie:` 等，覆盖没经过 iota 配置的凭证。因为要按文本匹配，stderr layer 关闭了 ANSI 颜色。
- 运行事件：`RuntimeEvent`，用于 CLI/TUI/desktop 展示，并作为 token usage 落库输入。
- 本地观测：`iota observability` 读取 SQLite store，聚焦 token usage 和 Prometheus 文本指标。
- 外部观测：Loki/Jaeger 查询命令只读取外部服务，不依赖本地 SQLite 聚合。
