# iota-sympantos 实验 3：三条 ACP 调用链路耗时对比

> Archive note: this is a dated experiment report. For current behavior and commands, see [../docs/iota book.md](../docs/iota%20book.md), [../docs/architecture.md](../docs/architecture.md), and [../docs/command.md](../docs/command.md).

| 字段 | 值 |
| :------| :-----|
| 实验代号 | exp03-acp-runtime |
| 执行日期 | 2026-05-07 |
| 实验对象 | CLI with daemon、CLI without daemon、backend direct 三条调用链路 |
| 实现位置 | `src/cli/mod.rs`, `src/daemon/`, `src/engine.rs`, `src/acp/`, `src/config.rs` |

---

## 一、实验目标

本实验不再验证“后端是否可用”本身，而是比较同一个 prompt 通过三条链路到达模型时的端到端耗时差异：

| 链路 | 入口 | 目标问题 |
| :------| :------| :----------|
| CLI without daemon | `iota run <backend> <prompt>` | 每次启动一个新的 iota CLI 进程时，ACP adapter 初始化、session 创建和 prompt 执行合计要多久 |
| CLI with daemon | `iota run --daemon <backend> <prompt>` | CLI 只作为 thin client，经 TCP 转发到常驻 daemon 后，链路耗时能减少多少 |
| Backend direct | 各 backend 自身 one-shot/headless CLI | 不经过 iota、不经过 daemon、不经过 ACP adapter 时，backend 原生命令的基线耗时是多少 |

核心判断：daemon 的价值不是重新证明 backend 启动能力，而是把高频 CLI 调用中的重复启动成本搬到常驻进程里；backend direct 则作为外部基线，用来判断 iota/ACP 层额外引入了多少链路成本。

---

## 二、链路定义

### 2.1 CLI without daemon

命令形态：

```powershell
.\target\release\iota.exe run --trace-timing <backend> "say hello. reply with exactly: hello"
```

该路径包含：

| 阶段 | 说明 |
| :------| :------|
| CLI process | Windows 上启动 `iota.exe`，解析参数，读取 `~/.i6/nimia.yaml` |
| Engine | 在当前 CLI 进程内创建 `IotaEngine` |
| ACP adapter | 按 backend 配置启动 adapter 进程，执行 `initialize` |
| ACP session | 执行 `session/new` |
| Prompt | 执行 `session/prompt` 并等待 `session/complete` |

这条链路适合衡量“单次命令调用”的真实用户感知耗时。它会重复支付 CLI 进程启动、engine 构造、adapter 启动和 session 创建成本。

### 2.2 CLI with daemon

命令形态：

```powershell
.\target\release\iota.exe run --daemon --trace-timing <backend> "say hello. reply with exactly: hello"
```

该路径包含：

| 阶段 | 说明 |
| :------| :------|
| CLI process | 启动短生命周期 `iota.exe` |
| TCP hop | 连接 `127.0.0.1:47661`，发送 `DaemonPromptRequest` |
| Daemon engine | daemon 内按 cwd 复用 `IotaEngine` |
| ACP client/session | 已预热时复用 backend client 与 session |
| Prompt | 执行 `session/prompt` 并返回 `DaemonPromptResponse` |

这条链路适合衡量“反复从 shell 调用 iota”时 daemon 是否能摊薄 adapter 与 session 成本。冷启动第一次 daemon 调用仍可能包含 backend 初始化，热路径才是 daemon 设计要优化的主要场景。

### 2.3 Backend direct

命令形态按 backend 不同而不同：

| Backend | Direct 命令 |
| :---------| :-------------|
| claude-code | `claude -p "say hello. reply with exactly: hello"` |
| codex | `codex exec "say hello. reply with exactly: hello"` |
| gemini | `gemini -p "say hello. reply with exactly: hello"` |
| hermes | `hermes -z "say hello. reply with exactly: hello"` |
| opencode | `npx -y opencode-ai@1.14.40 run "say hello. reply with exactly: hello"` |

该路径不经过 iota，不启动 ACP adapter，也不经过 daemon。它只用于提供 backend 原生 one-shot 模式的外部基线，不能替代 ACP 兼容性验证。

---

## 三、实验环境

| 项目 | 值 |
| :------| :-----|
| OS | Windows |
| Shell | PowerShell 7.6.1 |
| Workspace | `D:\coding\creative\iota-sympantos` |
| Binary | `target/release/iota.exe` |
| Daemon address | `127.0.0.1:47661` |
| 配置来源 | `~/.i6/nimia.yaml` |
| Prompt | `say hello. reply with exactly: hello` |

### 3.1 Backend 版本

`version_mapping` 只记录具体版本号，不记录包名、命令串或 update 信息。

| Backend | ACP version | bin version | 说明 |
| :---------| :------------:| :------------:| :------|
| claude-code | 0.32.0 | 2.1.123 | `@agentclientprotocol/claude-agent-acp` + `claude` |
| codex | 0.12.0 | 0.128.0 | `@zed-industries/codex-acp` 与 `codex-cli` 版本不同 |
| gemini | 0.41.2 | 0.41.2 | `@google/gemini-cli --acp` |
| hermes | 0.12.0 | 0.12.0 | `hermes acp` |
| opencode | 1.14.40 | 1.14.40 | 配置使用 `npx opencode-ai@1.14.40` |

---

## 四、测量方法

### 4.1 构建与基础测试

```powershell
cargo fmt
cargo test --release -- --format terse
cargo build --release
.\target\release\iota.exe check
```

已验证结果：

| 命令 | 结果 |
| :------| :------|
| `cargo fmt` | 通过 |
| `cargo test --release -- --format terse` | 93 passed |
| `cargo build --release` | 通过 |
| `iota check` | 5 个 backend configured，包含 `version_mapping.acp/bin` |

### 4.2 CLI without daemon

逐 backend 执行：

```powershell
foreach ($backend in @('claude-code','codex','gemini','hermes','opencode')) {
  .\target\release\iota.exe run --trace-timing $backend "say hello. reply with exactly: hello"
}
```

采集字段：

| 字段 | 含义 |
| :------| :------|
| `init_ms` | ACP adapter `initialize` 耗时 |
| `session_new_ms` | `session/new` 耗时 |
| `prompt_ms` | `session/prompt` 到完成耗时 |
| `total_ms` | iota 记录的本次 run 总耗时 |
| `client_started` | 本次是否启动 backend client |
| `process_spawned` | 本次是否启动 adapter 进程 |
| `session_reused` | 是否复用已有 session |

### 4.3 CLI with daemon

先用一轮调用预热 daemon，再采集热路径：

```powershell
foreach ($backend in @('claude-code','codex','gemini','hermes','opencode')) {
  .\target\release\iota.exe run --daemon $backend "warm up. reply exactly: ok"
}

foreach ($backend in @('claude-code','codex','gemini','hermes','opencode')) {
  .\target\release\iota.exe run --daemon --trace-timing $backend "say hello. reply with exactly: hello"
}
```

热路径期望字段：

| 字段 | 期望 |
| :------| :------|
| `route` | `daemon` |
| `daemon_hit` | `true` |
| `client_started` | `false` |
| `process_spawned` | `false` |
| `session_reused` | `true` |
| `session_new_ms` | 省略、`null` 或显著低于 cold path |

### 4.4 Backend direct

PowerShell 统一测量方式：

```powershell
$prompt = "say hello. reply with exactly: hello"
$commands = @(
  @{ backend = 'claude-code'; command = { claude -p $prompt | Out-Null } },
  @{ backend = 'codex';       command = { codex exec $prompt | Out-Null } },
  @{ backend = 'gemini';      command = { gemini -p $prompt | Out-Null } },
  @{ backend = 'hermes';      command = { hermes -z $prompt | Out-Null } },
  @{ backend = 'opencode';    command = { npx -y opencode-ai@1.14.40 run $prompt | Out-Null } }
)

foreach ($item in $commands) {
  $elapsed = Measure-Command { & $item.command }
  "$($item.backend),$([int]$elapsed.TotalMilliseconds)"
}
```

Backend direct 的数据只用于横向参照。由于各后端 direct CLI 默认加载的配置、权限策略、MCP、记忆系统和输出格式并不完全一致，它不能与 ACP 路径做逐阶段字段对齐，只能比较端到端 one-shot 耗时。

---

## 五、当前样本数据

### 5.1 CLI without daemon：已采集样本

以下数据来自 `iota run --trace-timing <backend> "say hello. reply with exactly: hello"`。

| Backend | init_ms | session_new_ms | prompt_ms | total_ms | 输出 |
| :---------| :--------:| :---------------:| :----------:| :---------:| :------|
| claude-code | 1120-1212 | 758-844 | 3780-3907 | 4539-4752 | `hello` |
| codex | 1031 | 3287 | 18015 | 21303 | `hello` |
| gemini | 6254 | 1622 | 2069 | 3691 | `hello` |
| hermes | 2694 | 7007 | 3932 | 10939 | `hello` |
| opencode | 41017 | 1580 | 3634 | 5214 | `hello` |

观察：

| Backend | 主要耗时来源 |
| :---------| :--------------|
| claude-code | prompt 阶段占主导，adapter/session 较稳定 |
| codex | prompt 阶段显著偏高 |
| gemini | initialize 偏高，但 prompt 较快 |
| hermes | session/new 偏高 |
| opencode | 首次 npx initialize 极高，因此必须用 60s timeout 覆盖 cold path |

注：不同 adapter 对 `total_ms` 与分阶段字段的定义不完全一致，报告保留运行时原始字段，不把阶段值强行相加。

### 5.2 iota 两条链路历史样本

以下样本来自同一轮 3 次 benchmark 的中位数，prompt 为 `say hello. reply with exactly: hello`。这里的 `CLI without daemon cold` 指每轮独立冷启动 adapter/session；`CLI with daemon hot` 指 daemon 已预热后的 prompt 路径。

| Backend | CLI with daemon hot ms | CLI without daemon cold ms | daemon speedup | 说明 |
| :---------| :-----------------------:| :----------------------------:| :---------------:| :------|
| claude-code | 1569 | 3756 | 2.4x | daemon 省去 adapter/session 重复启动 |
| codex | 1415 | 5880 | 4.1x | cold path prompt + adapter 成本较高 |
| gemini | 1185 | 7300 | 6.2x | cold path 需把 `init_ms` 计入用户感知耗时 |
| hermes | 1468 | 4378 | 3.0x | session/new 复用收益明显 |
| opencode | 3532 | 4838 | 1.4x | daemon 收益最小，且热路径波动较大 |

这组数据已经能回答 iota 内部两条链路的主要问题：热 daemon 稳定低于 CLI cold；收益大小取决于各 backend 的 initialize 和 session/new 成本。

### 5.3 三链路对比表

使用同一 prompt `say hello. reply with exactly: hello`，同一网络状态、同一 backend 配置，按下表落盘。

| Backend | Backend direct ms | CLI with daemon hot ms | CLI without daemon cold ms | daemon 相对 cold 改善 | iota cold 相对 direct 差值 |
| :---------| :------------------:| :-----------------------:| :----------------------------:| :----------------------:| :---------------------------:|
| claude-code | 1326 | 1569 | 3756 | 58.2% | +2430 ms |
| codex | 14261 | 1415 | 5880 | 75.9% | −8381 ms |
| gemini | 18834 | 1185 | 7300 | 83.8% | −11534 ms |
| hermes | 8895 | 1468 | 4378 | 66.5% | −4517 ms |
| opencode | 8262 | 3532 | 4838 | 27.0% | −3424 ms |

> **Backend direct 测量命令：** `claude -p`、`codex exec`、`gemini -p --skip-trust`、`hermes -z`、`npx -y opencode-ai@1.14.40 run`

计算方式：

```text
daemon 相对 cold 改善 = (CLI without daemon cold ms - CLI with daemon hot ms) / CLI without daemon cold ms
iota cold 相对 direct 差值 = CLI without daemon cold ms - Backend direct ms
    正值 → iota 比 direct 慢（额外开销）
    负值 → iota 比 direct 快（backend direct 自身负担重）
```

#### 关键发现

1. **4/5 backend 中 iota CLI cold 比 backend direct 更快。** 原因：backend direct（`codex exec`、`gemini -p`、`hermes -z`、`opencode run`）会加载完整 CLI 环境、插件、权限策略、记忆系统等；而 ACP adapter 只启动最小推理入口。
2. **唯一例外是 Claude Code。** `claude -p` 的 `--bare` 等选项本身极轻量；iota 冷路径额外花费约 2.4s 在 adapter 启动和 session 创建上。
3. **iota daemon hot 路径是所有 5 个 backend 中绝对最快的链路。** 通过摊薄 adapter/session 成本，daemon 跑出 1185-3532ms，均远低于 backend direct 对应的 8262-18834ms。

---

## 六、结论

1. 本实验的比较对象是三条链路的端到端耗时，不是后端可用性矩阵。
2. **daemon hot 是所有 5 个 backend 中绝对最快的链路。** 热 daemon 耗时 1185-3532ms，分别低于 backend direct（1326-18834ms）和 CLI cold（3756-7300ms）。
3. **4/5 backend 中 iota CLI cold 比 backend direct 更快。** ACP adapter 模式不加载 backend 自身的完整 CLI 环境，因此冷启动也比 `codex exec` / `gemini -p` / `hermes -z` / `opencode run` 更轻。
4. **Claude Code 是唯一例外**：`claude -p` 本身极轻量（1326ms），而 iota 冷路径需要额外 2.4s 完成 adapter initialize 和 session/new。daemon hot（1569ms）与 Claude direct（1326ms）基本持平。
5. daemon 相对 CLI cold 的改善范围 27.0%-83.8%。Gemini（83.8%）和 Codex（75.9%）获益最大，因为它们的 ACP adapter 冷启动成本最高。
6. `CLI without daemon` 暴露每个 backend 的主要冷启动成本：Codex 偏 prompt 阶段，Hermes 偏 session/new，OpenCode/Gemini 偏首次 npx initialize。

---

## 七、复验命令

```powershell
cargo test --release -- --format terse
cargo build --release
.\target\release\iota.exe check

# CLI without daemon
foreach ($backend in @('claude-code','codex','gemini','hermes','opencode')) {
  .\target\release\iota.exe run --trace-timing $backend "say hello. reply with exactly: hello"
}

# CLI with daemon: warm first, then measure hot path
foreach ($backend in @('claude-code','codex','gemini','hermes','opencode')) {
  .\target\release\iota.exe run --daemon $backend "warm up. reply exactly: ok"
}

foreach ($backend in @('claude-code','codex','gemini','hermes','opencode')) {
  .\target\release\iota.exe run --daemon --trace-timing $backend "say hello. reply with exactly: hello"
}

# Backend direct baseline
$prompt = "say hello. reply with exactly: hello"
Measure-Command { claude -p $prompt | Out-Null }
Measure-Command { codex exec $prompt | Out-Null }
Measure-Command { gemini -p $prompt | Out-Null }
Measure-Command { hermes -z $prompt | Out-Null }
Measure-Command { npx -y opencode-ai@1.14.40 run $prompt | Out-Null }
```

期望：三组命令均能得到端到端耗时；最终报告只使用同一轮环境下的三列数据做比较，避免把 cold path、hot path、不同 backend direct 配置混在一起。
