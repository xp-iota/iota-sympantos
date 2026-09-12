# iota-sympantos 调试指南

## 前置要求

- Rust toolchain 和 Cargo 可用。
- `~/.i6/nimia.yaml` 已配置，且不要把 API key、token、密码写入日志、截图或提交。
- 调试 Rust 代码建议安装 VS Code CodeLLDB。
- 调试桌面端需在 `crates/iota-desktop` 执行 `npm install`。

## 常用命令

```bash
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p iota-cli -- check
cargo run -p iota-cli -- run hermes "ping"
cargo run -p iota-cli -- run --daemon hermes "ping"
cd crates/iota-desktop && npm test && npm run build
```

## iota-core 单元测试布局

`crates/iota-core` 不使用源文件内联测试模块。新增或迁移测试时：

1. 在源模块同目录创建 `<module>_tests.rs`；
2. 在源文件中使用 `#[cfg(test)]`、`#[path = "<module>_tests.rs"]` 和 `mod tests;` 引入；
3. 测试文件只使用 `crate::...` 绝对导入，不使用 `use super::*`；
4. `#[test]` 函数直接位于测试文件顶层，不再嵌套第二层模块。

当前可参考：

- `src/acp/client_tests.rs`：分别覆盖 `loadSession`、`sessionCapabilities.resume` 解析，并验证缺失能力时 fail closed；
- `src/resources_tests.rs`：覆盖显式本地 skill root 与标准 workspace 本地资源路径。

定向排查可运行：

```bash
cargo test -p iota-sympantos-core parses_load_and_resume_capabilities_independently
cargo test -p iota-sympantos-core workspace_resources_are_derived_without_network_configuration
```

## 断点入口

| 场景 | 文件 |
| :--- | :--- |
| CLI 命令分发 | `crates/iota-cli/src/cli/mod.rs` |
| 单次执行 | `crates/iota-cli/src/cli/run_cmd.rs` |
| daemon client | `crates/iota-cli/src/cli/daemon_cmd.rs` |
| TUI event loop | `crates/iota-cli/src/tui/loop.rs` |
| TUI 输入组件 | `crates/iota-cli/src/tui/input.rs` |
| TUI 终端生命周期 | `crates/iota-cli/src/tui/terminal_lifecycle.rs` |
| Engine 编排 | `crates/iota-core/src/engine/mod.rs` |
| Engine prompt path | `crates/iota-core/src/engine/prompt.rs` |
| ACP client | `crates/iota-core/src/acp/client.rs` |
| ACP wire | `crates/iota-core/src/acp/wire.rs` |
| ACP session params | `crates/iota-core/src/acp/session.rs` |
| Permission | `crates/iota-core/src/acp/permission.rs` |
| Daemon server | `crates/iota-core/src/daemon/mod.rs` |
| Desktop daemon protocol | `crates/iota-core/src/daemon/desktop.rs` |
| Desktop Tauri commands | `crates/iota-desktop/src-tauri/src/lib.rs` |
| Desktop daemon client | `crates/iota-desktop/src-tauri/src/daemon_client.rs` |
| Kanban store | `crates/iota-kanban/src/sqlite_store.rs` |

## 日志和环境变量

```bash
RUST_LOG=debug
RUST_BACKTRACE=1
IOTA_LOG=iota_core::acp=debug,iota_core::engine=debug
IOTA_LOG_DIR=/tmp/iota-logs
IOTA_DAEMON_ADDR=127.0.0.1:47661
IOTA_DESKTOP_DAEMON_ADDR=127.0.0.1:47662
IOTA_CLI_PATH=/absolute/path/to/iota
```

说明：

- `RUST_LOG` 控制 stderr tracing。
- `IOTA_LOG` 控制文件日志过滤规则。
- 工程日志默认写入 `~/.i6/logs/`。
- desktop 会先尝试 `IOTA_DAEMON_ADDR` 或默认 daemon 地址，再尝试 `IOTA_DESKTOP_DAEMON_ADDR` 或 `127.0.0.1:47662` fallback。
- desktop autostart 需要 `IOTA_CLI_PATH` 指向 `iota` binary，或 `iota` 位于 `PATH`。

## TUI 调试注意事项

TUI 使用 crossterm raw mode、mouse capture 和 terminal guard。断点暂停时终端可能停留在 raw mode。

建议：

- 优先在进入 TUI 之前的命令分发阶段设置断点。
- 调试 key handling 时设置条件断点，避免每帧暂停。
- 终端异常时执行 `reset`。
- stdout 不是 terminal 时，TUI 会拒绝启动。

## ACP 子进程调试

ACP 后端由 `npx` 或 `hermes acp` 启动，是外部进程。Rust 侧通常在以下位置断点：

- `AcpClient::start()`：子进程启动、stdin/stdout/stderr 管道。
- `wire::read_next_line()`：读取 backend stdout。
- `wire::parse_message_line()`：JSON-RPC parse。
- `runtime_event::map_acp_events()`：事件归一化。
- `permission::answer_permission_request()`：工具授权。

`--show-native` 会暴露原始协议内容，可能包含敏感信息，只用于本地调试。

## Desktop 调试

```bash
cd crates/iota-desktop
npm run dev:clean
```

常见检查：

- `npm test` 覆盖 reducer、layout 和 memory/context workspace。
- `npm run build` 覆盖 TypeScript 和 Vite build。
- Tauri command 通过 daemon JSON-line protocol 与 core 交互。
- 前端监听 `daemon-message` 和 `daemon-client-error` window events。
- `dev:clean` 会停止已有 `iota __daemon`，构建当前 workspace 的 CLI，并设置 `IOTA_CLI_PATH`，避免连接到旧 binary。
- 如果只调 React/Vite，可用 `npm run dev:frontend`，但 daemon-backed invoke 需要 Tauri 环境。

## 常见问题

| 问题 | 排查 |
| :--- | :--- |
| 断点不命中 | 确认 debug profile、无 `--release`，async 断点放在 `.await` 后也试一次 |
| `iota run --daemon` 连接失败 | 检查 `IOTA_DAEMON_ADDR`，手动运行 `cargo run -p iota-cli -- __daemon` |
| Desktop 无法启动 daemon | 设置 `IOTA_CLI_PATH` 或把 `iota` 放进 `PATH` |
| 后端不可用 | 运行 `iota check`，确认 `nimia.yaml`、adapter command、API key 和 model |
| Hermes 配置异常 | 不要覆盖 `HERMES_HOME`，只通过 provider 原生环境变量和 `HERMES_MODEL`/`HERMES_INFERENCE_PROVIDER` 配置 |
| SQLite 文件不可写 | 检查 `~/.i6/context`、`~/.i6/kanban` 权限 |
