# iota-sympantos 实验4：Backend Token Observability

> Archive note: this is a dated experiment report. For current behavior and commands, see [../docs/iota book.md](../docs/iota%20book.md), [../docs/architecture.md](../docs/architecture.md), and [../docs/command.md](../docs/command.md).

| 字段 | 值 |
| :------| :-----|
| 实验代号 | exp04-token-stats |
| 执行日期 | 2026-05-17 |
| 目标状态 | 统一从 `iota observability` 获取 token 数据 |
| 当前状态 | 已实现并完成 5 backend × 3 轮复验 |
| 测试 prompt | 请用一句话介绍 Rust 语言的主要特点。 |
| Binary | `target/release/iota` |
| 配置来源 | `~/.i6/nimia.yaml` |

---

## 1. 实验目标

exp04 验证以下链路：

```text
ACP usage -> RuntimeEvent::TokenUsage -> ObservabilityStore -> iota observability -> TUI -> 实验报告
```

本轮不再以 `--show-native` 作为实验数据源。`--show-native` 仅保留为 parser 对照，最终数据来自：

```bash
./target/release/iota observability tokens recent --limit 20 --json
./target/release/iota observability tokens summary --since 1h --json
./target/release/iota observability metrics --prometheus
```

---

## 2. 背景资料

### 2.1 Backend ACP 数据形态

| Backend | ACP Adapter | 模型 | 当前 token 数据形态 |
| :---------| :-------------| :------| :---------------------|
| claude-code | `@agentclientprotocol/claude-agent-acp` | MiniMax-M3 | `usage`，含 input/output/cache read/cache write/total |
| codex | `@zed-industries/codex-acp` | gpt-5.4-mini (medium) | `usage_update.used`，只有 adapter total |
| gemini | `@google/gemini-cli --acp` | gemini-2.5-flash | `_meta.quota.token_count` + `_meta.quota.model_usage`，含 input/output/model |
| hermes | `hermes acp` | MiniMax-M3 | `usage`，含 input/output/cache read/total |
| opencode | `opencode-ai acp` | MiniMax-M3 | `usage`，含 input/output/thinking/total |

### 2.2 协议

| 语义                      | iota 字段名                         | OpenAI Chat / Completions                          | OpenAI Responses API                           | Anthropic Messages API                         | Gemini / Google GenAI                        |
| ------------------------- | ----------------------------------- | -------------------------------------------------- | ---------------------------------------------- | ---------------------------------------------- | -------------------------------------------- |
| 输入 token                | `input_tokens`                      | `usage.prompt_tokens`                              | `usage.input_tokens`                           | `usage.input_tokens`                           | `usageMetadata.promptTokenCount`             |
| 输出 token                | `output_tokens`                     | `usage.completion_tokens`                          | `usage.output_tokens`                          | `usage.output_tokens`                          | `usageMetadata.candidatesTokenCount`         |
| 总 token                  | `normalized_total_tokens`           | `usage.total_tokens`                               | `usage.total_tokens`                           | 无直接字段，需计算                             | `usageMetadata.totalTokenCount`              |
| 缓存命中输入 token        | `cache_read_input_tokens`           | `usage.prompt_tokens_details.cached_tokens`        | `usage.input_tokens_details.cached_tokens`     | `usage.cache_read_input_tokens`                | `usageMetadata.cachedContentTokenCount`      |
| 缓存写入输入 token        | `cache_creation_input_tokens`       | 无常规同名字段                                     | 无常规同名字段                                 | `usage.cache_creation_input_tokens`            | 无常规同名字段                               |
| 推理 / thinking token     | `thinking_tokens`                   | `usage.completion_tokens_details.reasoning_tokens` | `usage.output_tokens_details.reasoning_tokens` | 通常无单独 usage 字段                          | `usageMetadata.thoughtsTokenCount`           |
| 工具结果回灌 token        | `tool_use_prompt_tokens`            | 工具/事件口径不统一                                | 工具/事件口径不统一                            | `usage.server_tool_use.web_search_requests` 等 | `usageMetadata.toolUsePromptTokenCount`      |
| 输入 token 按模态拆分     | -                                   | `prompt_tokens_details.audio_tokens` 等            | 部分接口有 details                             | 不同能力下字段不同                             | `usageMetadata.promptTokensDetails[]`        |
| 缓存 token 按模态拆分     | -                                   | 通常无统一字段                                     | 通常无统一字段                                 | 通常无统一字段                                 | `usageMetadata.cacheTokensDetails[]`         |
| 输出 token 按模态拆分     | -                                   | `completion_tokens_details.audio_tokens` 等        | 部分接口有 details                             | 不同能力下字段不同                             | `usageMetadata.candidatesTokensDetails[]`    |
| 工具回灌 token 按模态拆分 | -                                   | 不统一                                             | 不统一                                         | 不统一                                         | `usageMetadata.toolUsePromptTokensDetails[]` |
| 流式 usage                | -                                   | 需 `stream_options.include_usage`                  | final usage / usage event，视接口              | stream 事件中有 usage                          | 最后一个 chunk 才有 `usageMetadata`          |

---

## 3. 原始 observability 数据

数据源：`gefsi/logs/exp04-observability-recent.json`

| Backend | Run | execution_id | input | cache_read | cache_write | output | thinking | provider_total | normalized_total | source |
| :---| :---:| :---| :---:| :---:| :---:| :---:| :---:| :---:| :---:| :---|
| claude-code | 1 | `abeeb796` | 277 | 0 | 27693 | 65 | - | 28035 | 28035 | `usage` |
| claude-code | 2 | `cbd1e7e0` | 277 | 24335 | 3366 | 86 | - | 28064 | 28064 | `usage` |
| claude-code | 3 | `2b17941a` | 277 | 24335 | 3357 | 64 | - | 28033 | 28033 | `usage` |
| codex | 1 | `a870966a` | - | - | - | - | - | 23203 | - | `session_update.usage_update` |
| codex | 2 | `6fd03863` | - | - | - | - | - | 23211 | - | `session_update.usage_update` |
| codex | 3 | `7a209421` | - | - | - | - | - | 23192 | - | `session_update.usage_update` |
| gemini | 1 | `363efa21` | 15169 | - | - | 31 | - | 15200 | 15200 | `_meta.quota.token_count` + `model_usage` |
| gemini | 2 | `dcf09154` | 15178 | - | - | 31 | - | 15209 | 15209 | `_meta.quota.token_count` + `model_usage` |
| gemini | 3 | `a54fabc1` | 15178 | - | - | 34 | - | 15212 | 15212 | `_meta.quota.token_count` + `model_usage` |
| hermes | 1 | `29537a06` | 19029 | 0 | - | 70 | 0 | 19099 | 19099 | `usage` |
| hermes | 2 | `d56c36e6` | 19027 | 18459 | - | 71 | 0 | 19098 | 19098 | `usage` |
| hermes | 3 | `43e1f695` | 19035 | 18459 | - | 70 | 0 | 19105 | 19105 | `usage` |
| opencode | 1 | `6a21332e` | 19236 | - | - | 36 | 36 | 19308 | 19308 | `usage` |
| opencode | 2 | `c200ca93` | 19237 | - | - | 32 | 30 | 19299 | 19299 | `usage` |
| opencode | 3 | `cc9ff90a` | 19236 | - | - | 42 | 22 | 19300 | 19300 | `usage` |

---

## 4. 统计汇总

数据源：`gefsi/logs/exp04-observability-summary.json`

| Backend | Count | input mean±std | cache_read mean±std | cache_write mean±std | output mean±std | thinking mean±std | provider_total mean±std | normalized_total mean±std |
| :---| :---:| :---| :---| :---| :---| :---| :---| :---|
| claude-code | 3 | 277.0±0.0 | 16223.3±14049.8 | 11472.0±14047.8 | 71.7±12.4 | N/A | 28044.0±17.3 | 28044.0±17.3 |
| codex | 3 | N/A | N/A | N/A | N/A | N/A | 23202.0±9.5 | N/A |
| gemini | 3 | 15175.0±5.2 | N/A | N/A | 32.0±1.7 | N/A | 15207.0±6.2 | 15207.0±6.2 |
| hermes | 3 | 19030.3±4.2 | 12306.0±10657.3 | N/A | 70.3±0.6 | 0.0±0.0 | 19100.7±3.8 | 19100.7±3.8 |
| opencode | 3 | 19236.3±0.6 | N/A | N/A | 36.7±5.0 | 29.3±7.0 | 19302.3±4.9 | 19302.3±4.9 |

### 4.1 normalized total 排序

Codex 没有 normalized total，因为 adapter 只提供 `usage_update.used`，无法分解 input/output/cache。

| 排名 | Backend | normalized_total mean | 说明 |
| :------| :---------| :-----------------------| :------|
| 1 | claude-code | 28044.0 | 含 cache write/read 相关 adapter total |
| 2 | opencode | 19302.3 | total 包含 thinking |
| 3 | hermes | 19100.7 | Run 2/3 cache_read 明显命中 |
| 4 | gemini | 15207.0 | ACP quota 暴露 input/output，`model_usage` 可用于 provider total |
| N/A | codex | N/A | 只有 provider/adapter total |

### 4.2 provider reported total 排序

| 排名 | Backend | provider_total mean | 说明 |
| :------| :---------| :---------------------| :------|
| 1 | claude-code | 28044.0 | final usage total |
| 2 | codex | 23202.0 | `usage_update.used` |
| 3 | opencode | 19302.3 | final usage total |
| 4 | hermes | 19100.7 | final usage total |
| 5 | gemini | 15207.0 | 从 `_meta.quota.model_usage[].token_count` 汇总 |

---

## 5. 观察结论

1. **observability 链路已闭环**：15 条 execution-level token 记录全部来自 `iota observability tokens recent`。
2. **raw event 去重是必要的**：claude-code、hermes、opencode 会产生 `usage_update` 和 final `usage` 多条 token event；summary 必须按 execution 选择最佳事件。
3. **Codex 仍是字段最少的 backend**：当前只能报告 adapter total，不能计算 normalized total。
4. **Gemini 当前 ACP 字段不是标准 `usageMetadata`**：本轮从 `_meta.quota.token_count` 获取 input/output，并从同级 `_meta.quota.model_usage[].token_count` 汇总 provider total；旧版 parser 只读取 `token_count` 子对象，导致 provider total 被误报为 N/A。
5. **cache 行为可观测**：claude-code Run 1 写入大量 cache，Run 2/3 cache_read 明显命中；hermes Run 2/3 也出现 cache_read 命中。
6. **TUI 展示已支持完整字段**：有完整字段时显示 `in/cache/out/think/total/exec`，字段缺失时降级为 total。

---

## 6. 验证命令与结果

```bash
cargo test runtime_event
# 20 passed

cargo test observability
# 12 passed

cargo test tui
# 17 passed

cargo test
# 211 passed

cargo build --release
# finished release build

./target/release/iota observability metrics --prometheus
# iota_token_usage_count 15
```

---

## 7. 复验命令

```bash
cargo build --release
./target/release/iota check

PROMPT="请用一句话介绍 Rust 语言的主要特点。"
for backend in claude-code codex gemini hermes opencode; do
  for run in 1 2 3; do
    ./target/release/iota run --no-daemon --backend "$backend" --timeout-ms 180000 "$PROMPT"
    sleep 2
  done
done

./target/release/iota observability tokens recent --limit 20 --json
./target/release/iota observability tokens summary --since 1h --json
./target/release/iota observability metrics --prometheus
```

---

## 8. 验收矩阵

| # | 验收项 | 状态 | 说明 |
| :---| :--------| :------| :------|
| 1 | Token parser | 通过 | runtime_event tests 覆盖 OpenAI / Anthropic / Gemini / adapter-only |
| 2 | 持久化 | 通过 | token usage events 写入 `token_usage_events` |
| 3 | execution 去重 | 通过 | metrics 显示 15 条 execution-level 记录 |
| 4 | CLI 查询 | 通过 | `tokens recent/summary/export` 和 `metrics --prometheus` 可用 |
| 5 | Metrics | 通过 | 输出 input/cache/output/thinking/provider/normalized 聚合 |
| 6 | TUI 展示 | 通过 | 状态栏和 scrollback/pager 支持完整 token breakdown |
| 7 | exp04 数据 | 通过 | 5 backend × 3 轮均来自 `iota observability` |
| 8 | fallback 边界 | 通过 | `--show-native` 只作为 parser 对照 |

---
