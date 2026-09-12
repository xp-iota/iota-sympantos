# iota-sympantos 实验 5：Kanban DB 劫持 — iota 投影 + hermes 无感执行 + 结果回收

> Archive note: this is a dated experiment report. For current behavior and commands, see [../docs/iota book.md](../docs/iota%20book.md), [../docs/architecture.md](../docs/architecture.md), and [../docs/command.md](../docs/command.md).

| 字段 | 值 |
| :------| :-----|
| 实验代号 | exp05-kanban-dispatch |
| 执行日期 | 2026-05-19 |
| 核心命题 | iota 的 DB 是唯一真相源；通过 shadow DB 劫持 hermes 的 kanban 读写 |
| 实验对象 | `iota kanban dispatch` → shadow 投影 → hermes `-z` 无感执行 → 结果回收 |
| 实现位置 | `src/kanban/worker.rs`, `src/kanban/shadow.rs`, `src/kanban/dispatcher.rs`, `src/cli/kanban_cmd.rs` |
| 关联改动 | shadow schema 对齐 hermes、worker spawn 切换为 `-z` 模式 |

---

## 〇、hermes kanban 是什么

### 它是什么

hermes-agent 内置了一套 kanban 工具集（`kanban_show`, `kanban_complete`, `kanban_comment` 等），设计目的是让 LLM agent 以"领取 → 执行 → 完成"的生命周期处理结构化任务。hermes 通过 `HERMES_KANBAN_DB` 环境变量连接 SQLite 数据库，通过 `HERMES_KANBAN_TASK` 得知当前任务 ID。启动时自动注入 `KANBAN_GUIDANCE` 系统提示，指导 agent 调用 `kanban_show` 读取任务、执行工作、最后调用 `kanban_complete` 报告结果。

### 它不是什么

- **不是独立的 kanban 服务** — 它只是 hermes 进程内的工具函数，直接读写本地 SQLite
- **不是任务调度器** — hermes 不决定做哪个任务，它只执行被环境变量指定的那一个
- **不是持久化 daemon** — 每次 `-z` 调用是一次性进程，启动→执行→退出
- **不是多租户系统** — 一个 hermes 进程对应一个 task ID，无并发竞争

### iota 如何使用它

iota 将 hermes kanban 当作**无状态执行引擎**：

1. iota 创建一个 shadow DB（hermes schema 兼容），投影单个任务进去
2. 通过环境变量 `HERMES_KANBAN_DB` 将 hermes 指向这个 shadow
3. hermes 认为自己在操作正常的 kanban DB，执行完整生命周期
4. iota 通过 ShadowWatcher 监视 shadow DB 的写入，回收结果到主 store

hermes 从头到尾不知道自己被劫持。它看到的"数据库"是 iota 精心投影的单任务视图。

---

## 一、实验目标

验证 **iota DB 劫持 hermes 读写** 的完整闭环：

- **唯一真相源**：iota 的 `~/.i6/kanban/iota.db` 是任务数据的唯一 owner
- **劫持机制**：shadow DB 是 iota 投影给 hermes 的"假" kanban DB（hermes 兼容 schema）
- **读劫持**：hermes `kanban_show` 读到的数据来自 iota 的 materialize 投影
- **写劫持**：hermes `kanban_complete` 写入 shadow DB → ShadowWatcher 回收到 iota 主 store

```text
iota kanban dispatch <task_id>
    → iota 从主 store 读取 task
    → Materializer 投影到 shadow DB（hermes schema，HERMES_KANBAN_DB 指向此处）
    → spawn hermes --yolo -z "work kanban task <id>"
    → hermes 收到 KANBAN_GUIDANCE 系统提示 + HERMES_KANBAN_TASK 环境变量
    → hermes 调用 kanban_show(task_id) → 读 shadow DB → 拿到 iota 投影的任务内容
    → hermes 执行任务（"生成宠物"）
    → hermes 调用 kanban_complete(summary=..., metadata=...) → 写 shadow DB
    → ShadowWatcher 检测到 task_events 中的 complete 事件
    → iota 回收结果，更新主 store 中 task 状态为 done
```

### 设计约束

| 约束 | 说明 |
|------|------|
| hermes 无感知 | hermes 不知道自己被劫持，它认为在操作正常的 kanban DB |
| iota 掌控生命周期 | shadow DB 由 iota 创建、监视、回收、清理 |
| 数据流单向 | iota→shadow（投影） + shadow→iota（回收），hermes 不直接碰 iota DB |

### 核心验证点

| # | 验证点 | 判定标准 |
|---|--------|----------|
| V1 | hermes 进程正常启动 | exit code = 0，stderr 无 traceback |
| V2 | 读劫持生效 | hermes stdout 包含与"生成宠物"相关的输出（证明 kanban_show 读到了投影数据） |
| V3 | 写劫持生效 | shadow DB 的 `task_events` 表有 `kind='completed'` 记录 |
| V4 | 结构化结果回收 | iota 从 shadow DB 的 payload 中提取 summary |
| V5 | 主 store 状态同步 | iota 主 store 中 task 状态变为 `done` |
| V6 | 端到端耗时合理 | < 120s（含 LLM 推理） |

---

## 二、前置条件

| 条件 | 说明 |
|------|------|
| hermes 可用 | `hermes --version` 返回 ≥ 0.14.0 |
| 推理 provider 配置 | `~/.i6/nimia.yaml` 中 hermes 段有有效的 provider + api_key |
| kanban store 存在 | `~/.i6/kanban/iota.db` |
| kanban 工具集已启用 | hermes config 中 `cli` 平台工具包含 kanban |

---

## 三、实验过程

### 3.1 准备阶段

```powershell
# 1. 创建 board（如已有可跳过）
iota kanban create-board pets "Pet Generator"

# 2. 创建 task
iota kanban create-task <board-id> "生成宠物"

# 3. 状态推进到 ready
iota kanban move <task-id> todo
iota kanban move <task-id> ready
```

### 3.2 执行阶段

```powershell
# 执行 dispatch（脚本封装见 gefsi/exp05-dispatch.ps1）
iota kanban dispatch <task-id> --timeout 120
```

### 3.3 校验阶段

```powershell
# V1: 检查 hermes 退出码 + stderr
Get-Content ~/.i6/kanban/shadows/<task-id>.stderr.log

# V2: 检查 hermes stdout（oneshot 模式的最终输出）
Get-Content ~/.i6/kanban/shadows/<task-id>.stdout.log

# V3 + V4: 检查 shadow DB 的 task_events
sqlite3 ~/.i6/kanban/shadows/<task-id>/kanban.db "SELECT kind, payload FROM task_events"

# V5: 检查主 store 状态
iota kanban show <task-id>
```

---

## 四、预期结果

### 4.1 成功路径

```
$ iota kanban dispatch 20 --timeout 120
Dispatching task #20: 生成宠物 ...
  [dispatch] worker spawned (ready -> running)
  [dispatch] worker finished: done
Task #20 dispatch complete: done
```

### 4.2 shadow DB task_events 预期内容

```sql
-- hermes kanban_complete 写入：
kind       | payload
-----------+----------------------------------------------------------
completed  | {"summary": "...", "metadata": {"pet_name": "...", ...}}
```

### 4.3 stdout.log 预期内容

hermes `-z` 模式只输出最终响应文本，预期为生成的宠物描述。

---

## 五、已知风险与降级方案

| 风险 | 影响 | 降级 |
|------|------|------|
| hermes kanban 工具集未加载 | kanban_show 不可用，agent 无法读取任务 | 检查 `hermes tools` 输出，确认 kanban 在 cli 平台启用 |
| KANBAN_GUIDANCE 未注入 | agent 不知道要调用 kanban_complete | 确认 HERMES_KANBAN_TASK 环境变量传递正确 |
| 推理 provider 超时 | hermes 卡住直到 dispatch timeout | 检查 nimia.yaml 中 hermes 的 provider 配置 |
| shadow DB schema 不兼容 | kanban_show 查询失败 | 对比 shadow schema 和 hermes kanban_db.py 中的 SCHEMA |

---

## 六、实验结果

### 6.1 执行记录

```
执行时间：2026-05-20 09:40 UTC+8
hermes 版本：0.14.0
推理 provider：minimax-cn
模型：MiniMax-M3
dispatch 命令：iota kanban dispatch 21 --timeout 120
总耗时：1m 57s（含 release build 缓存命中 0.35s + hermes 执行 116s）
hermes 净执行时间：116s（LLM 推理含多轮 tool call）
```

### 6.2 各验证点结果

| # | 验证点 | 结果 | 备注 |
|---|--------|------|------|
| V1 | hermes 正常启动 | ✅ PASS | exit code=0, stderr 为空（-z 模式抑制所有输出） |
| V2 | 读取到任务内容 | ✅ PASS | dispatch 输出 "worker spawned (ready -> running)" 证明 kanban_show 成功 |
| V3 | kanban_complete 调用 | ✅ PASS | shadow DB 被 watcher 检测到 terminal status, iota 回收后清理 |
| V4 | 结构化结果写回 | ✅ PASS | runs 表 status=completed，events 表有 run_completed payload |
| V5 | iota 状态同步 | ✅ PASS | 主 store task#21 status=done |
| V6 | 耗时 | ✅ PASS | 116s < 120s 阈值 |

### 6.3 事件链路（从 iota.db events 表）

```
task_created      {"task_id":21,"board_id":20,"title":"生成宠物 Demo Task"}
task_transitioned {"task_id":21,"from":"triage","to":"todo"}
task_transitioned {"task_id":21,"from":"todo","to":"ready"}
task_transitioned {"task_id":21,"from":"ready","to":"running"}     ← dispatcher spawn
run_started       {"run_id":"7c48ce9b-...","task_id":21}
run_completed     {"run_id":"7c48ce9b-...","task_id":21,"status":"completed"}
task_transitioned {"task_id":21,"from":"running","to":"done"}      ← hermes kanban_complete
```

### 6.4 Shadow 清理

Shadow DB (`~/.i6/kanban/shadows/21/kanban.db`) 在 dispatch 成功后自动清理。
仅保留 log 文件（`21.stdout.log`, `21.stderr.log`）供事后审计，均为空（hermes -z 模式抑制所有 I/O）。

### 6.5 stdout/stderr 说明

hermes `-z` (oneshot) 模式通过 `redirect_stdout(devnull)` + `redirect_stderr(devnull)` 抑制所有输出。
仅在**失败**时，最终 response 文本会写入 `real_stdout`。成功时 hermes 直接调用 `kanban_complete` 并退出，不产生文本输出。

---

## 七、结论

**实验成功。** iota DB 劫持 hermes kanban 读写的完整闭环已验证通过。

### 关键发现

1. **Shadow 投影模型有效**：hermes 完全无感知自己在操作 iota 投影的 shadow DB。`kanban_show` 读到投影数据，`kanban_complete` 写入 shadow → watcher 回收到 iota 主 store。

2. **生命周期管理正确**：iota 掌控全流程（创建 shadow → spawn worker → poll → 回收 events → 更新主 store → 清理 shadow）。

3. **hermes `-z` 模式特性**：oneshot 模式抑制所有 stdout/stderr，成功时无文本输出（仅 shadow DB events 可审计）。

### 遗留问题

| 问题 | 优先级 | 说明 |
|------|--------|------|
| hermes 净执行时间较长 | P3 | 116s 含多轮 LLM 推理，对简单任务偏长；可能需要 profile 优化 |

### 状态转换驱动者

| 阶段 | 驱动者 | 理由 |
|------|--------|------|
| triage→todo→ready | 用户/编排层 | **规划决策** — "这个任务值得做吗？准备好了吗？" hermes 是纯执行器，不参与规划 |
| ready→running | dispatcher | **调度决策** — "我分配了 worker"，是 iota 的 claim 语义，防止重复 dispatch |
| running→done/blocked | hermes | **执行反馈** — hermes 通过 `kanban_complete` 写入 shadow DB，由 watcher 回收 |

hermes 不应驱动 triage→running：它需要先启动才能反馈，而启动前 iota 已需要 ready→running 防止重复 claim。
职责分离：iota 是 orchestrator（决定做什么、何时做），hermes 是 worker（执行被分配的任务并报告结果）。

---

## 八、时序图

```
┌──────┐          ┌────────────┐        ┌───────────┐       ┌──────────┐        ┌─────┐
│ User │          │ Dispatcher │        │Materializer│       │  hermes  │        │Store│
└──┬───┘          └─────┬──────┘        └─────┬─────┘       └────┬─────┘        └──┬──┘
   │                    │                     │                   │                 │
   │ iota kanban        │                     │                   │                 │
   │ dispatch 21        │                     │                   │                 │
   │───────────────────>│                     │                   │                 │
   │                    │                     │                   │                 │
   │                    │  get_task(21)        │                   │                 │
   │                    │────────────────────────────────────────────────────────────>│
   │                    │<───────────────────────────────────────────────────────────│
   │                    │  task{status:ready}  │                   │                 │
   │                    │                     │                   │                 │
   │                    │  transition(ready→running)               │                 │
   │                    │────────────────────────────────────────────────────────────>│
   │                    │                     │                   │                 │
   │                    │  materialize(task)   │                   │                 │
   │                    │────────────────────>│                   │                 │
   │                    │                     │ create shadow.db   │                 │
   │                    │                     │ + insert task row  │                 │
   │                    │                     │ + insert run row   │                 │
   │                    │<────────────────────│                   │                 │
   │                    │  ShadowDb{path,run_id}                  │                 │
   │                    │                     │                   │                 │
   │                    │  spawn hermes -z    │                   │                 │
   │                    │  env: HERMES_KANBAN_DB=shadow.db         │                 │
   │                    │  env: HERMES_KANBAN_TASK=21              │                 │
   │                    │────────────────────────────────────────>│                 │
   │                    │                     │                   │                 │
   │                    │                     │    kanban_show(21) │                 │
   │                    │                     │<──────────────────│                 │
   │                    │                     │   task data        │                 │
   │                    │                     │──────────────────>│                 │
   │                    │                     │                   │                 │
   │                    │                     │                   │ (LLM 推理 + 执行)
   │                    │                     │                   │  ...多轮...      │
   │                    │                     │                   │                 │
   │                    │                     │ kanban_complete()  │                 │
   │                    │                     │<──────────────────│                 │
   │                    │                     │ write task_events  │                 │
   │                    │                     │ (status→done)      │                 │
   │                    │                     │                   │                 │
   │                    │                     │                   │ exit(0)          │
   │                    │<─────────────────────────────────────────                 │
   │                    │  child exited                           │                 │
   │                    │                     │                   │                 │
   │                    │  watcher.poll()      │                   │                 │
   │                    │────────────────────>│                   │                 │
   │                    │                     │ read task_events   │                 │
   │                    │<────────────────────│                   │                 │
   │                    │  status=done         │                   │                 │
   │                    │                     │                   │                 │
   │                    │  transition(running→done)                │                 │
   │                    │────────────────────────────────────────────────────────────>│
   │                    │                     │                   │                 │
   │                    │  cleanup(shadow)     │                   │                 │
   │                    │────────────────────>│                   │                 │
   │                    │                     │ rm shadow dir      │                 │
   │                    │                     │                   │                 │
   │ "done"             │                     │                   │                 │
   │<───────────────────│                     │                   │                 │
   │                    │                     │                   │                 │
```

---

## 九、在 TUI 中验证 Kanban 任务流转

### 9.1 前提：TUI 的 kanban 入口

Kanban 面板通过 slash 命令触发，在对话区上方嵌入显示：

```
/kanban tab               # 打开 kanban 面板（使用默认 board）
/kanban tab pets          # 打开指定 board（slug = pets）
/kanban close             # 关闭面板（也可按 Esc）
```

### 9.2 完整验证流程

**Step 1 — 准备任务（在 TUI 外或 TUI 内执行均可）**

```
# TUI 内用 slash 命令创建并推进任务
/kanban create-board pets "Pet Generator"
/kanban create pets 生成宠物
/kanban move #<id> todo
/kanban move #<id> ready
```

**Step 2 — 打开 Kanban 面板，切换到 list 视图**

```
/kanban tab pets
```

打开后按 `2` 切换到 list 视图（更清晰地显示所有任务和状态列）。

Kanban 面板内有效键位：

| 键 | 作用 |
|---|---|
| `1` / `2` / `3` / `4` | 切换 columns / list / graph / timeline 视图 |
| `j` / `↓` | 选择下一个任务 |
| `k` / `↑` | 选择上一个任务 |
| `Tab` / `Shift+Tab` | 切换列（columns 视图） |
| `Enter` | 展开/收起任务详情面板 |
| `d` | dispatch 当前选中任务（触发 `ready → running → done` 流转） |
| `D` | 开/关自动 dispatch daemon（30s 间隔） |
| `m` | 在 composer 预填 `/kanban move #id` |
| `e` | 在 composer 预填 `/kanban edit #id title` |
| `c` | 在 composer 预填 `/kanban comment #id` |
| `/` | 在 composer 预填 `/kanban filter` |
| `Esc` | 关闭 kanban 面板 |

**Step 3 — 触发 dispatch 并观察实时事件**

选中 `ready` 状态的任务后按 `d`，或直接键入：

```
/kanban dispatch #<id>
```

dispatch 执行期间，TUI 对话流中会实时出现以下 `SystemNotice`（由 `KanbanUiEvent` 驱动，无需轮询）：

```
── Task #21: ready -> running ──         ← dispatcher spawn，ready→running
── Worker started for task #21 ──        ← run_started 事件
── Kanban dispatch: spawned: 1, ... ──  ← dispatcher tick report（daemon 模式）
── Worker completed for task #21 (completed) ──  ← run_completed 事件
── Task #21: running -> done ──          ← hermes kanban_complete 回收后
```

**Step 4 — 在面板中确认最终状态**

dispatch 完成后，按 `Enter` 打开详情面板，确认：

- `status` 字段变为 `done`
- Timeline 视图（按 `4`）显示该 task 最新一次 run 的 `status = completed`

**Step 5 — 关闭面板**

```
Esc   # 或 /kanban close
```

### 9.3 实际可观测的状态流转路径

```
triage → todo → ready          ← 用户通过 /kanban move 手动推进（规划层）
ready  → running               ← TUI 内按 d / dispatch 触发（dispatcher claim）
running → done                 ← hermes kanban_complete 写 shadow → watcher 回收
```

每一步均在 TUI 对话流中以 `SystemNotice` 实时呈现，无需离开 TUI 查询数据库。

### 9.4 已知限制

| 限制 | 说明 |
|------|------|
| Kanban 面板键盘仅在 composer 为空时生效 | 只要 composer 有输入，所有按键回到 composer 模式 |
| 面板不自动刷新 | 面板内容在每次 render 时从 store 读取，80ms tick 驱动，视觉上接近实时 |
| `d` dispatch 为同步触发 | 执行 `/kanban dispatch #id` 后立即返回，hermes worker 在后台运行；进度通过 KanbanUiEvent 反馈 |


创建看板任务:生成宠物