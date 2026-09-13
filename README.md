# iota sympantos

Cross-platform Rust CLI/TUI，将 prompt 路由到五个 ACP 后端（claude-code / codex / gemini / hermes / opencode），共享统一的记忆、技能与上下文层。内置 Kanban 任务看板，支持 Agent 长期任务的调度、追踪与多节点同步。

## 核心功能

| 功能 | 说明 |
| :------| :------|
| **跨后端记忆** | SQLite 存储（SHA-256 去重、FTS5、6 召回桶），任一后端写入的记忆可在其他后端召回注入 |
| **确定性技能** | YAML 声明，由 Rust 引擎分发；触发匹配与输出模板与后端无关 |
| **iota-fun** | 7 语言片段运行器（C++ / TypeScript / Rust / Zig / Java / Python / Go），含编译缓存与 `parallel: true` |
| **Kanban** | 内置任务看板：状态机、Dispatcher、Shadow 工作区、Event Sourcing、事件包同步 |
| **Daemon 热路径** | 仅 loopback 的 TCP daemon 保持 ACP 客户端预热；敏感请求以 owner-only CSPRNG token 鉴权并审计，`--daemon/-d` 路由 |
| **TUI** | ratatui 内联视图，多行编辑器、Markdown 渲染、流式输出、Ctrl+C 双击退出 |

## 快速开始

```bash
rustup install 1.95.0
rustup default 1.95.0 && rustup toolchain uninstall stable
cargo build -p iota-cli -p iota-core -p iota-sympantos-kanban

iota                                    # 交互式 TUI
iota run codex "ping"                   # 单次 prompt
iota run --backend claude "解释递归"    # 指定后端
iota check                              # 检查后端配置
```

## 开发阶段脚本

脚本按平台提供 `.sh` 和 `.ps1` 两套入口。macOS/Linux 使用 `.sh`，Windows
PowerShell 使用 `.ps1`。所有后端和模型配置只从 `~/.i6/nimia.yaml` 读取。

首次开发先初始化模型配置，然后在文件中填写 provider、model、base URL 和
API key：

```bash
./scripts/configure-model.sh --init --open
```

Windows：

```powershell
.\scripts\configure-model.ps1 -Init -Open
```

配置文件已经存在时，脚本不会覆盖它。也可以直接使用 `--open` / `-Open`
打开已有配置。`--open` 优先使用 `$VISUAL` 或 `$EDITOR`；未设置时，
macOS 使用系统默认文本编辑器，Linux 使用 `xdg-open`。不要把真实的
`nimia.yaml` 提交到 Git。

启动 CLI/TUI：

```bash
./scripts/dev-cli.sh
./scripts/dev-cli.sh check
./scripts/dev-cli.sh run codex "ping"
```

Windows：

```powershell
.\scripts\dev-cli.ps1
.\scripts\dev-cli.ps1 check
.\scripts\dev-cli.ps1 run codex "ping"
```

启动桌面 App：

```bash
./scripts/run-desktop.sh
```

该脚本会停止旧的 iota daemon 和桌面开发服务器，构建当前 debug CLI，
必要时执行 `npm install`，再启动 Tauri dev。只停止旧进程：

```bash
./scripts/run-desktop.sh --stop-only
```

Windows PowerShell 使用同一个跨平台 Node 入口：

```powershell
cd crates\iota-desktop
npm run dev:clean
```

`npm run dev:clean -- --stop-only` 只停止旧 daemon 和桌面开发服务器。

Windows/macOS/Linux 的桌面前端依赖都从 `crates/iota-desktop/package-lock.json`
安装。首次或依赖变更时也可以手动执行：

```bash
cd crates/iota-desktop && npm ci
```

常用的直接开发命令仍然可用：

```bash
cargo test               # 运行全部测试
cargo check --offline
RUST_LOG=debug cargo run -p iota-cli --quiet
cargo run -p iota-cli --quiet -- run codex "ping" --timing
```

## 构建 CLI 和桌面安装包

构建 release CLI：

```bash
./scripts/build-cli.sh
# 产物：target/release/iota
```

Windows：

```powershell
.\scripts\build-cli.ps1
# 产物：target\release\iota.exe
```

构建 Tauri 桌面安装包：

```bash
./scripts/build-app.sh
# 产物目录：target/release/bundle
```

Windows：

```powershell
.\scripts\build-app.ps1
# 产物目录：target\release\bundle
```

`build-app` 会在 `crates/iota-desktop/node_modules` 不存在时执行
`npm ci`，然后运行 `tauri build`。Tauri 根据当前平台生成 `.app`、`.dmg`、
`.AppImage`、`.deb`、`.msi` 或 `.exe` 等安装包。

## 安装和卸载

安装 release CLI 到用户目录：

```bash
./scripts/install.sh --cli
```

默认安装到 `~/.local/bin/iota`。可以通过 `IOTA_INSTALL_DIR` 指定目录。
安装桌面包时显式传入构建产物：

```bash
./scripts/install.sh --app target/release/bundle/macos/iota-desktop.app
./scripts/install.sh --app target/release/bundle/appimage/iota-desktop.AppImage
./scripts/install.sh --app target/release/bundle/deb/iota-desktop_*.deb
```

`--all` 会同时安装 CLI，并按当前平台自动查找桌面包：

```bash
./scripts/install.sh --all
```

Windows：

```powershell
.\scripts\install.ps1 -Cli
.\scripts\install.ps1 -App .\target\release\bundle\msi\iota-desktop_0.1.0_x64_en-US.msi
.\scripts\install.ps1 -All
```

Unix 安装脚本把 CLI 放到 `~/.local/bin`，macOS App 放到
`~/Applications`，Linux AppImage 放到 `~/.local/bin`；`.deb` 使用
`dpkg` 安装。Windows 脚本把 CLI 放到 `%USERPROFILE%\.local\bin`，
桌面 `.msi` 或 `.exe` 交给平台安装器处理。

卸载脚本只删除脚本管理的用户级 CLI 和桌面副本，保留配置、记忆、日志和
Kanban 数据：

```bash
./scripts/uninstall.sh
```

Windows 桌面包由 MSI/Windows 安装器管理；先在系统设置中卸载桌面 App，
再执行：

```powershell
.\scripts\uninstall.ps1
```

如果只需要查看脚本帮助：

```bash
./scripts/configure-model.sh --help
./scripts/run-desktop.sh --help
./scripts/install.sh --help
```

## 可复用 crates

Kanban 领域库已发布到 crates.io：

```toml
[dependencies]
iota-kanban = { package = "iota-sympantos-kanban", version = "0.1.0" }
```

核心 runtime 的包名是 `iota-sympantos-core`，Rust library target 仍为 `iota_core`。启用 `kanban` feature 时会使用已发布的 `iota-sympantos-kanban`（Rust import 仍为 `iota_kanban`）：

```toml
[dependencies]
iota-core = { package = "iota-sympantos-core", version = "0.1.0", features = ["kanban"] }
```

详见 [`crates/iota-core/README.md`](crates/iota-core/README.md) 和 [`crates/iota-kanban/README.md`](crates/iota-kanban/README.md)。

## 发布

`iota-sympantos-kanban` 必须先于启用其可选 feature 的 `iota-sympantos-core` 发布。发布前先运行：

```powershell
.\scripts\publish-crates.ps1 -DryRun
```

确认 crates.io token 已通过 `cargo login` 或 `CARGO_REGISTRY_TOKEN` 配置后，移除 `-DryRun` 执行正式发布。macOS/Linux 使用 `./scripts/publish-crates.sh --dry-run`。

### 配置文件

`~/.i6/nimia.yaml`，每个后端的关键字段：

```yaml
codex:
  enabled: true
  acp:
    command: npx
    args: ["-y", "@zed-industries/codex-acp@0.12.0"]
  model:
    provider: ninerouter
    name: gh/gpt-5.4
    base_url: http://localhost:20128/v1
    api_key: "<router-api-key>"
```

`iota check` 查看所有后端生效配置。

### Hermes 后端

```bash
pip install 'hermes-agent[acp]'
```

## 文档

| 文档 | 说明 |
| :------| :------|
| [`docs/iota book.md`](docs/iota%20book.md) | **《iota 技术指南》（iota book）** —— 面向程序员与 AI 从业者的系统化核心设计与实现指南 |
| [`docs/architecture.md`](docs/architecture.md) | 系统架构设计 |
| [`docs/code-call-chains.md`](docs/code-call-chains.md) | 代码调用链路 |
| [`docs/observability.md`](docs/observability.md) | logs / trace / metrics |
| [`docs/debugging.md`](docs/debugging.md) | 调试指南 |
| [`docs/docker.md`](docs/docker.md) | Docker 与外部观测栈 |
| [`docs/desktop-mvp-acceptance.md`](docs/desktop-mvp-acceptance.md) | Desktop MVP 验收标准 |
| [`crates/iota-core/README.md`](crates/iota-core/README.md) | `iota-sympantos-core` 依赖方式、features 与最小示例 |
| [`crates/iota-kanban/README.md`](crates/iota-kanban/README.md) | `iota-sympantos-kanban` crate API 与依赖方式 |

---

- `nimia`  词源：*μνημεία*
- `iota` 词源：*ιώτα*
- `sýmpantos` 词源：*σύμπαντος*
- `gefsi` 词源： `γεύση`

https://v2.tauri.app/release/
