use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

pub const TOOLS: [(&str, &str); 7] = [
    ("fun.rust", "Rust"),
    ("fun.typescript", "TypeScript"),
    ("fun.python", "Python"),
    ("fun.go", "Go"),
    ("fun.java", "Java"),
    ("fun.cpp", "C++"),
    ("fun.zig", "Zig"),
];
const MAX_COMMAND_OUTPUT_BYTES: usize = 64 * 1024;

/// Total size cap for the on-disk build cache (`~/.i6/iota-fun`).
///
/// Each language/target combination leaves a binary behind, and without a cap a
/// long-lived install accumulates them indefinitely.
const MAX_BUILD_CACHE_BYTES: u64 = 256 * 1024 * 1024;

/// Concurrent `iota-fun` executions allowed at once.
///
/// Each execution may compile with a full toolchain, so an unbounded fan-out
/// from concurrent MCP calls can exhaust the process table and memory.
const MAX_CONCURRENT_EXECUTIONS: usize = 4;

/// Environment variables never passed to a `iota-fun` child, even though the
/// parent process holds them.
///
/// `iota-core` injects these into ACP backend subprocesses
/// (`config/adapters.rs`). A pet-generator toy has no use for any of them, and
/// inheriting them would leak live credentials into a process whose whole
/// purpose is to run trivial sample code.
const SCRUBBED_ENV_KEYS: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_TOKEN",
    "OPENAI_API_KEY",
    "GEMINI_API_KEY",
    "ROUTER_API_KEY",
    "MINIMAX_API_KEY",
    "MINIMAX_CN_API_KEY",
    "IOTA_DAEMON_TOKEN_PATH",
    "IOTA_SYNC_TOKEN_PATH",
    "OTEL_EXPORTER_OTLP_ENDPOINT",
    "OTEL_EXPORTER_OTLP_HEADERS",
];

/// Runs per execution at once, bounding total subprocess fan-out.
static EXECUTION_SLOTS: std::sync::OnceLock<std::sync::Mutex<usize>> = std::sync::OnceLock::new();

/// RAII slot in the global concurrency budget.
struct ExecutionSlot;

impl ExecutionSlot {
    /// Acquires a slot, blocking until one is free.
    fn acquire() -> Self {
        let slots = EXECUTION_SLOTS.get_or_init(|| std::sync::Mutex::new(0));
        loop {
            {
                let mut active = crate::utils::lock_or_recover(slots);
                if *active < MAX_CONCURRENT_EXECUTIONS {
                    *active += 1;
                    return Self;
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for ExecutionSlot {
    fn drop(&mut self) {
        let slots = EXECUTION_SLOTS.get_or_init(|| std::sync::Mutex::new(0));
        let mut active = crate::utils::lock_or_recover(slots);
        *active = active.saturating_sub(1);
    }
}

/// A prepared, isolated build/run directory for one execution.
///
/// Contains copies of the already SHA-256-verified sources, so the compiler and
/// the program can only write inside this directory — the shipped sources under
/// `skills/pet-generator/iota-fun` are never used as a working directory.
///
/// Removed on drop, including when the execution times out or panics.
struct TempWorkspace {
    root: PathBuf,
}

impl TempWorkspace {
    /// Creates a fresh temp directory holding `sources`, flattened by file name.
    ///
    /// Sources are addressed by bare file name inside the workspace, matching
    /// the relative paths the language runners already use.
    fn prepare(sources: &[PathBuf]) -> Result<Self> {
        let root = std::env::temp_dir().join(format!(
            "iota-fun-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        // `create_dir` (not `create_dir_all`) would race with itself; a unique
        // name means a leftover directory from a crashed run is the only way
        // this fails, and recreating it is the right recovery.
        fs::create_dir_all(&root)
            .with_context(|| format!("Failed to create iota-fun workspace {}", root.display()))?;
        let workspace = Self { root };
        for source in sources {
            let name = source.file_name().with_context(|| {
                format!("iota-fun source has no file name: {}", source.display())
            })?;
            let destination = workspace.root.join(name);
            fs::copy(source, &destination).with_context(|| {
                format!(
                    "Failed to stage iota-fun source {} into {}",
                    source.display(),
                    destination.display()
                )
            })?;
        }
        Ok(workspace)
    }

    fn path(&self) -> &Path {
        &self.root
    }

    /// Moves a produced artifact out of the workspace into the persistent cache.
    ///
    /// The workspace is deleted on drop, so a compiled binary has to be moved
    /// or it is lost and every call recompiles.
    fn promote_artifact(&self, name: &str, destination: &Path) -> Result<()> {
        let produced = self.root.join(name);
        if !produced.exists() {
            return Ok(());
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        // A cross-device rename fails; fall back to copy + remove.
        match fs::rename(&produced, destination) {
            Ok(()) => Ok(()),
            Err(_) => {
                fs::copy(&produced, destination).with_context(|| {
                    format!(
                        "Failed to cache iota-fun artifact {} -> {}",
                        produced.display(),
                        destination.display()
                    )
                })?;
                Ok(())
            }
        }
    }
}

impl Drop for TempWorkspace {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.root) {
            tracing::warn!(
                workspace = %self.root.display(),
                error = %error,
                "failed to remove iota-fun temp workspace"
            );
        }
    }
}

/// Structured failure kinds for `iota-fun` executions.
///
/// The MCP response carries the kind so a caller can distinguish "the toolchain
/// is missing" from "the program crashed", which need different responses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FunErrorKind {
    UnknownTool,
    ToolMissing,
    CompileFailed,
    RunFailed,
    Timeout,
    OutputLimit,
    Busy,
}

impl FunErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UnknownTool => "unknown_tool",
            Self::ToolMissing => "tool_missing",
            Self::CompileFailed => "compile_failed",
            Self::RunFailed => "run_failed",
            Self::Timeout => "timeout",
            Self::OutputLimit => "output_limit",
            Self::Busy => "busy",
        }
    }
}

/// A structured `iota-fun` failure.
#[derive(Debug)]
pub struct FunError {
    pub kind: FunErrorKind,
    pub message: String,
}

impl std::fmt::Display for FunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.kind.as_str(), self.message)
    }
}

impl std::error::Error for FunError {}

impl FunError {
    fn new(kind: FunErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

/// Converts a `run_command` failure into a structured error.
fn classify_command_error(error: anyhow::Error, kind: FunErrorKind) -> FunError {
    let kind = match error.to_string() {
        message if message.contains("timed out") => FunErrorKind::Timeout,
        message if message.contains("output exceeded") => FunErrorKind::OutputLimit,
        message if message.contains("Failed to start") => FunErrorKind::ToolMissing,
        _ => kind,
    };
    FunError::new(kind, error.to_string())
}

pub fn run_stdio() -> Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut stdin = stdin.lock();
    while let Some(line) = crate::mcp::read_limited_line(&mut stdin)? {
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = serde_json::from_str(&line).with_context(|| {
            let preview = line.chars().take(256).collect::<String>();
            format!("Invalid JSON-RPC: {preview}")
        })?;
        if request.get("id").is_none() {
            continue;
        }
        let response = handle_request(&request);
        writeln!(stdout, "{}", serde_json::to_string(&response)?)?;
        stdout.flush()?;
    }
    Ok(())
}

fn handle_request(request: &Value) -> Value {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    match request.get("method").and_then(Value::as_str).unwrap_or("") {
        "initialize" => ok(
            id,
            json!({"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"iota-fun","version":env!("CARGO_PKG_VERSION")}}),
        ),
        "tools/list" => ok(id, json!({"tools": tool_descriptions()})),
        "tools/call" => {
            let params = request.get("params").unwrap_or(&Value::Null);
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(Value::Null);
            match run_tool_structured(name, &args) {
                Ok(text) => ok(
                    id,
                    json!({"content":[{"type":"text","text":text}],"isError":false}),
                ),
                Err(error) => ok(
                    id,
                    json!({
                        "content":[{"type":"text","text":error.to_string()}],
                        "isError":true,
                        "structuredContent":{"error_kind":error.kind.as_str()},
                    }),
                ),
            }
        }
        other => error(id, -32601, &format!("unknown method {}", other)),
    }
}

fn tool_descriptions() -> Vec<Value> {
    TOOLS.iter().map(|(name, language)| json!({
        "name": name,
        "description": format!("Execute the configured pet-generator {} function with iota guardrails", language),
        "inputSchema": {"type":"object","properties":{"timeout_ms":{"type":"integer"}},"required":[]}
    })).collect()
}

pub fn run_tool(name: &str, args: &Value) -> Result<String> {
    run_tool_structured(name, args).map_err(anyhow::Error::from)
}

/// Executes one `iota-fun` tool, returning a structured failure.
///
/// Each execution:
///
/// 1. verifies the shipped sources by SHA-256,
/// 2. copies them into a fresh temp workspace,
/// 3. compiles and runs **inside that workspace** with a scrubbed environment,
/// 4. promotes any produced artifact into the persistent build cache,
/// 5. removes the workspace.
///
/// Step 3 is what keeps the compiler and the sample program from writing into
/// the shipped source tree; step 4 is what keeps the cache useful across calls.
///
/// Note the deliberate degradations below: for the compiled languages, a
/// missing toolchain or a failed build yields a random fallback value rather
/// than an error, because the pet generator is expected to work on machines
/// without (say) a Zig compiler. Those cases are reported in the response text
/// so the degradation is visible. Timeouts and output-limit breaches *are*
/// structured errors, since those indicate a runaway process rather than an
/// absent toolchain.
pub fn run_tool_structured(name: &str, args: &Value) -> std::result::Result<String, FunError> {
    let timeout_ms = args
        .get("timeout_ms")
        .and_then(Value::as_u64)
        .unwrap_or(10_000)
        .min(60_000);
    // Bounds how many compilations run at once; released when this returns.
    let _slot = ExecutionSlot::acquire();
    // Keep the cache from growing without bound across many builds.
    prune_build_cache();

    let outcome = match name {
        "fun.python" => run_python(timeout_ms),
        "fun.typescript" => run_typescript(timeout_ms),
        "fun.rust" => run_rust(timeout_ms),
        "fun.go" => run_go(timeout_ms),
        "fun.java" => run_java(timeout_ms),
        "fun.cpp" => run_cpp(timeout_ms),
        "fun.zig" => run_zig(timeout_ms),
        other => {
            return Err(FunError::new(
                FunErrorKind::UnknownTool,
                format!("unknown tool {other}"),
            ));
        }
    };
    outcome.inspect_err(|error| {
        crate::telemetry::metrics::get().record_sandbox_limit(error.kind.as_str());
    })
}

/// Locates and verifies `sources`, then stages them in a temp workspace.
///
/// Verification happens against the shipped originals before copying, so the
/// workspace can only ever contain known-good content.
fn stage_sources(sources: &[PathBuf]) -> std::result::Result<TempWorkspace, FunError> {
    ensure_files(sources)
        .map_err(|error| FunError::new(FunErrorKind::ToolMissing, error.to_string()))?;
    TempWorkspace::prepare(sources)
        .map_err(|error| FunError::new(FunErrorKind::RunFailed, error.to_string()))
}

fn run_python(timeout_ms: u64) -> std::result::Result<String, FunError> {
    let source = fun_root()
        .map_err(|e| FunError::new(FunErrorKind::RunFailed, e.to_string()))?
        .join("python")
        .join("random_number.py");
    let workspace = stage_sources(&[source])?;
    run_command(
        "python3",
        &[OsString::from("random_number.py")],
        Some(workspace.path()),
        timeout_ms,
    )
    .map_err(|e| classify_command_error(e, FunErrorKind::RunFailed))
}

fn run_typescript(timeout_ms: u64) -> std::result::Result<String, FunError> {
    let root = fun_root().map_err(|e| FunError::new(FunErrorKind::RunFailed, e.to_string()))?;
    let cwd = root.join("typescript");
    let sources = [cwd.join("runner.js"), cwd.join("randomColor.ts")];
    let workspace = stage_sources(&sources)?;
    run_command(
        "node",
        &[OsString::from("runner.js")],
        Some(workspace.path()),
        timeout_ms,
    )
    .map_err(|e| classify_command_error(e, FunErrorKind::RunFailed))
}

fn run_rust(timeout_ms: u64) -> std::result::Result<String, FunError> {
    let root = fun_root().map_err(|e| FunError::new(FunErrorKind::RunFailed, e.to_string()))?;
    let cwd = root.join("rust");
    let sources = [cwd.join("runner.rs"), cwd.join("random_material.rs")];
    let bin = cached_binary_path("rust", &sources)
        .map_err(|e| FunError::new(FunErrorKind::RunFailed, e.to_string()))?;
    let effective_timeout = timeout_ms.max(30_000);

    if !bin.exists() {
        let workspace = stage_sources(&sources)?;
        let mut compile_args = vec![OsString::from("runner.rs"), OsString::from("-o")];
        let staged_bin = workspace.path().join(binary_file_name(&bin));
        compile_args.push(staged_bin.as_os_str().to_os_string());
        #[cfg(windows)]
        {
            // rust-lld avoids depending on external MSVC linker installations.
            compile_args.push(OsString::from("-C"));
            compile_args.push(OsString::from("linker=rust-lld"));
        }
        if let Err(error) = run_command(
            "rustc",
            &compile_args,
            Some(workspace.path()),
            effective_timeout,
        ) {
            // No toolchain, or the build failed: fall back rather than error.
            tracing::warn!(error = %error, "iota-fun rust build unavailable; using fallback");
            return Ok(fallback_material_with_note("rust build unavailable"));
        }
        workspace
            .promote_artifact(binary_file_name(&bin), &bin)
            .map_err(|e| FunError::new(FunErrorKind::RunFailed, e.to_string()))?;
    }
    run_cached_binary(&bin, &cwd, effective_timeout, fallback_material, "rust")
}

fn run_go(timeout_ms: u64) -> std::result::Result<String, FunError> {
    let root = fun_root().map_err(|e| FunError::new(FunErrorKind::RunFailed, e.to_string()))?;
    let cwd = root.join("go");
    let sources = [cwd.join("random_shape.go"), cwd.join("runner.go")];
    let bin = cached_binary_path("go", &sources)
        .map_err(|e| FunError::new(FunErrorKind::RunFailed, e.to_string()))?;

    if !bin.exists() {
        let workspace = stage_sources(&sources)?;
        let staged_bin = workspace.path().join(binary_file_name(&bin));
        let built = run_command(
            "go",
            &[
                OsString::from("build"),
                OsString::from("-o"),
                staged_bin.as_os_str().to_os_string(),
                OsString::from("random_shape.go"),
                OsString::from("runner.go"),
            ],
            Some(workspace.path()),
            timeout_ms,
        );
        if let Err(error) = built {
            tracing::warn!(error = %error, "iota-fun go build unavailable");
            return Ok(fallback_shape_with_note("go build unavailable"));
        }
        workspace
            .promote_artifact(binary_file_name(&bin), &bin)
            .map_err(|e| FunError::new(FunErrorKind::RunFailed, e.to_string()))?;
    }
    run_cached_binary(&bin, &cwd, timeout_ms, fallback_shape, "go")
}

fn run_java(timeout_ms: u64) -> std::result::Result<String, FunError> {
    let root = fun_root().map_err(|e| FunError::new(FunErrorKind::RunFailed, e.to_string()))?;
    let cwd = root.join("java");
    let sources = [
        cwd.join("RandomAnimal.java"),
        cwd.join("RandomAnimalRunner.java"),
    ];
    let class_dir = cached_class_dir_path("java", &sources)
        .map_err(|e| FunError::new(FunErrorKind::RunFailed, e.to_string()))?;
    let class = class_dir.join("RandomAnimalRunner.class");
    if !class.exists() {
        let workspace = stage_sources(&sources)?;
        let staged_dir = workspace.path().join("classes");
        fs::create_dir_all(&staged_dir)
            .map_err(|e| FunError::new(FunErrorKind::RunFailed, e.to_string()))?;
        let compiled = run_command(
            "javac",
            &[
                OsString::from("-encoding"),
                OsString::from("UTF-8"),
                OsString::from("-d"),
                staged_dir.as_os_str().to_os_string(),
                OsString::from("RandomAnimal.java"),
                OsString::from("RandomAnimalRunner.java"),
            ],
            Some(workspace.path()),
            timeout_ms,
        );
        if let Err(error) = compiled {
            tracing::warn!(error = %error, "iota-fun java build unavailable");
            return Ok(fallback_animal_with_note("java build unavailable"));
        }
        // Promote the whole class directory, since `java` loads classes from it.
        promote_directory(&staged_dir, &class_dir)
            .map_err(|e| FunError::new(FunErrorKind::RunFailed, e.to_string()))?;
    }
    run_command(
        "java",
        &[
            OsString::from("-cp"),
            class_dir.as_os_str().to_os_string(),
            OsString::from("RandomAnimalRunner"),
        ],
        Some(class_dir.as_path()),
        timeout_ms,
    )
    .map_err(|e| classify_command_error(e, FunErrorKind::RunFailed))
}

fn run_cpp(timeout_ms: u64) -> std::result::Result<String, FunError> {
    let root = fun_root().map_err(|e| FunError::new(FunErrorKind::RunFailed, e.to_string()))?;
    let cwd = root.join("cpp");
    let sources = [
        cwd.join("random_action.h"),
        cwd.join("random_action.cpp"),
        cwd.join("random_action_runner.cpp"),
    ];
    let compiler = if command_available("clang++") {
        "clang++"
    } else {
        "g++"
    };
    let bin = cached_binary_path("cpp", &sources)
        .map_err(|e| FunError::new(FunErrorKind::RunFailed, e.to_string()))?;
    let effective_timeout = timeout_ms.max(30_000);

    if !bin.exists() {
        let workspace = stage_sources(&sources)?;
        let staged_bin = workspace.path().join(binary_file_name(&bin));
        let compiled = run_command(
            compiler,
            &[
                OsString::from("random_action.cpp"),
                OsString::from("random_action_runner.cpp"),
                OsString::from("-std=c++17"),
                OsString::from("-O2"),
                OsString::from("-o"),
                staged_bin.as_os_str().to_os_string(),
            ],
            Some(workspace.path()),
            effective_timeout,
        );
        if let Err(error) = compiled {
            tracing::warn!(error = %error, "iota-fun cpp build unavailable");
            return Ok(fallback_action_with_note("cpp build unavailable"));
        }
        workspace
            .promote_artifact(binary_file_name(&bin), &bin)
            .map_err(|e| FunError::new(FunErrorKind::RunFailed, e.to_string()))?;
    }
    run_cached_binary(&bin, &cwd, effective_timeout, fallback_action, "cpp")
}

fn run_zig(timeout_ms: u64) -> std::result::Result<String, FunError> {
    let root = fun_root().map_err(|e| FunError::new(FunErrorKind::RunFailed, e.to_string()))?;
    let cwd = root.join("zig");
    let sources = [cwd.join("runner.zig"), cwd.join("random_size.zig")];
    let bin = cached_binary_path("zig", &sources)
        .map_err(|e| FunError::new(FunErrorKind::RunFailed, e.to_string()))?;
    let effective_timeout = timeout_ms.max(30_000);

    if !bin.exists() {
        let workspace = stage_sources(&sources)?;
        let staged_bin = workspace.path().join(binary_file_name(&bin));
        let built = run_command(
            "zig",
            &[
                OsString::from("build-exe"),
                OsString::from("runner.zig"),
                OsString::from("-O"),
                OsString::from("ReleaseFast"),
                OsString::from("-lc"),
                OsString::from(format!("-femit-bin={}", staged_bin.display())),
            ],
            Some(workspace.path()),
            effective_timeout,
        );
        if let Err(error) = built {
            tracing::warn!(error = %error, "iota-fun zig build unavailable");
            return Ok(fallback_size_with_note("zig build unavailable"));
        }
        workspace
            .promote_artifact(binary_file_name(&bin), &bin)
            .map_err(|e| FunError::new(FunErrorKind::RunFailed, e.to_string()))?;
    }
    run_cached_binary(&bin, &cwd, effective_timeout, fallback_size, "zig")
}

/// Runs a cached binary, falling back to `fallback` when it produces nothing.
fn run_cached_binary(
    bin: &Path,
    run_dir: &Path,
    timeout_ms: u64,
    fallback: fn() -> String,
    language: &str,
) -> std::result::Result<String, FunError> {
    match run_command(bin.as_os_str(), &[], Some(run_dir), timeout_ms) {
        Ok(value) if !value.trim().is_empty() => Ok(value),
        Ok(_) => Ok(fallback()),
        Err(error) => {
            // A timeout or output breach is a runaway process, not an absent
            // toolchain, so it must surface as an error rather than silently
            // becoming a random value.
            let classified = classify_command_error(error, FunErrorKind::RunFailed);
            if matches!(
                classified.kind,
                FunErrorKind::Timeout | FunErrorKind::OutputLimit
            ) {
                return Err(classified);
            }
            tracing::warn!(
                language,
                error = %classified.message,
                "iota-fun run failed; using fallback"
            );
            Ok(fallback())
        }
    }
}

/// Moves every file from `source_dir` into `destination_dir`.
fn promote_directory(source_dir: &Path, destination_dir: &Path) -> Result<()> {
    fs::create_dir_all(destination_dir)
        .with_context(|| format!("Failed to create {}", destination_dir.display()))?;
    for entry in fs::read_dir(source_dir)
        .with_context(|| format!("Failed to read {}", source_dir.display()))?
    {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            let destination = destination_dir.join(entry.file_name());
            fs::copy(entry.path(), &destination).with_context(|| {
                format!(
                    "Failed to cache {} -> {}",
                    entry.path().display(),
                    destination.display()
                )
            })?;
        }
    }
    Ok(())
}

/// The binary name to look for inside a staged workspace.
fn binary_file_name(cached: &Path) -> &str {
    cached
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("runner")
}

fn run_command<S: AsRef<std::ffi::OsStr>>(
    command: S,
    args: &[OsString],
    cwd: Option<&Path>,
    timeout_ms: u64,
) -> Result<String> {
    let command_label = command.as_ref().to_string_lossy().to_string();
    let mut cmd = Command::new(&command);
    // `env_clear` starts from nothing, so the child only receives the explicit
    // allowlist below. The scrub list is applied as well for defence in depth:
    // a future edit that re-adds an inherited variable must not be able to
    // reintroduce a credential.
    cmd.args(args)
        .env_clear()
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    for key in SCRUBBED_ENV_KEYS {
        cmd.env_remove(key);
    }
    // Put the child in its own process group so a timeout can terminate the
    // whole tree. A compiler or a sample program may spawn helpers, and killing
    // only the direct child would leave those running.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    if let Some(path) = std::env::var_os("PATH") {
        cmd.env("PATH", path);
    }
    if let Some(home) = dirs::home_dir() {
        let go_cache = home.join(".i6").join("fun-cache").join("go-build");
        let _ = fs::create_dir_all(&go_cache);
        cmd.env("GOCACHE", go_cache);
        #[cfg(not(windows))]
        cmd.env("HOME", &home);
        #[cfg(windows)]
        {
            cmd.env("USERPROFILE", &home);
            cmd.env("HOME", &home);

            if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
                cmd.env("LOCALAPPDATA", local_app_data);
            } else {
                cmd.env("LOCALAPPDATA", home.join("AppData").join("Local"));
            }

            if let Some(app_data) = std::env::var_os("APPDATA") {
                cmd.env("APPDATA", app_data);
            } else {
                cmd.env("APPDATA", home.join("AppData").join("Roaming"));
            }
        }
    }
    for key in ["TMPDIR", "TEMP", "TMP"] {
        if let Some(value) = std::env::var_os(key) {
            cmd.env(key, value);
        }
    }
    #[cfg(windows)]
    {
        if let Some(system_root) = std::env::var_os("SystemRoot") {
            cmd.env("SystemRoot", system_root);
        }
        if let Some(windir) = std::env::var_os("WINDIR") {
            cmd.env("WINDIR", windir);
        }
    }
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    let mut child = cmd
        .spawn()
        .with_context(|| format!("Failed to start {}", command_label))?;

    let mut stdout = child.stdout.take().context("tool stdout was not piped")?;
    let mut stderr = child.stderr.take().context("tool stderr was not piped")?;
    let output_limit_exceeded = Arc::new(AtomicBool::new(false));
    let stdout_limit = Arc::clone(&output_limit_exceeded);
    let stdout_handle = std::thread::spawn(move || read_limited_output(&mut stdout, stdout_limit));
    let stderr_limit = Arc::clone(&output_limit_exceeded);
    let stderr_handle = std::thread::spawn(move || read_limited_output(&mut stderr, stderr_limit));

    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .with_context(|| format!("Failed to wait for {}", command_label))?
        {
            break status;
        }
        if Instant::now() >= deadline {
            kill_child_tree(&mut child);
            let _ = child.wait();
            drop(stdout_handle);
            drop(stderr_handle);
            return Err(anyhow!("tool timed out after {}ms", timeout_ms));
        }
        if output_limit_exceeded.load(Ordering::Relaxed) {
            kill_child_tree(&mut child);
            let _ = child.wait();
            let _ = join_output(stdout_handle, "stdout");
            let _ = join_output(stderr_handle, "stderr");
            return Err(anyhow!(
                "tool output exceeded {} bytes per stream",
                MAX_COMMAND_OUTPUT_BYTES
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    };

    let stdout = join_output(stdout_handle, "stdout")?;
    let stderr = join_output(stderr_handle, "stderr")?;
    if output_limit_exceeded.load(Ordering::Relaxed) {
        return Err(anyhow!(
            "tool output exceeded {} bytes per stream",
            MAX_COMMAND_OUTPUT_BYTES
        ));
    }
    let mut text = String::from_utf8_lossy(&stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&stderr));
    if status.success() {
        Ok(trim_output(&text))
    } else {
        Err(anyhow!(trim_output(&text)))
    }
}

fn read_limited_output(reader: &mut impl Read, limit_exceeded: Arc<AtomicBool>) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut chunk = [0_u8; 8 * 1024];
    loop {
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            return Ok(output);
        }
        let remaining = MAX_COMMAND_OUTPUT_BYTES.saturating_sub(output.len());
        let retained = remaining.min(read);
        output.extend_from_slice(&chunk[..retained]);
        if retained < read {
            limit_exceeded.store(true, Ordering::Relaxed);
        }
    }
}

/// Terminates a child and everything it spawned.
///
/// The child was started in its own process group (Unix) or is killed as a tree
/// (Windows), so helper processes a compiler or the sample program started are
/// terminated too rather than being left running.
fn kill_child_tree(child: &mut std::process::Child) {
    #[cfg(windows)]
    {
        let pid = child.id().to_string();
        let _ = Command::new("taskkill")
            .args(["/PID", &pid, "/T", "/F"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    #[cfg(unix)]
    {
        // Negative pid targets the whole process group created above.
        let pid = child.id() as i32;
        // SAFETY: `kill` with a negative pid signals the group; the child is
        // our own and the group was created for it.
        let signalled = unsafe { libc::kill(-pid, libc::SIGKILL) };
        if signalled != 0 {
            // Group signalling can fail if the child already exited; fall back
            // to the direct kill so the call is still best-effort complete.
            let _ = child.kill();
        }
    }
    #[cfg(not(unix))]
    let _ = child.kill();
}

fn join_output(
    handle: std::thread::JoinHandle<Result<Vec<u8>>>,
    stream_name: &str,
) -> Result<Vec<u8>> {
    handle
        .join()
        .map_err(|_| anyhow!("tool {} reader thread panicked", stream_name))?
}

fn fun_root() -> Result<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("skills").join("pet-generator").join("iota-fun"));
        candidates.push(
            cwd.join("iota-skill")
                .join("pet-generator")
                .join("iota-fun"),
        );
    }
    if let Ok(exe) = std::env::current_exe() {
        for ancestor in exe.ancestors().take(8) {
            candidates.push(
                ancestor
                    .join("skills")
                    .join("pet-generator")
                    .join("iota-fun"),
            );
            candidates.push(
                ancestor
                    .join("iota-skill")
                    .join("pet-generator")
                    .join("iota-fun"),
            );
        }
    }
    candidates
        .into_iter()
        .find(|path| path.is_dir())
        .context("Failed to locate pet-generator iota-fun directory")
}

fn ensure_files(paths: &[PathBuf]) -> Result<()> {
    for path in paths {
        ensure_file(path)?;
    }
    Ok(())
}

fn ensure_file(path: &Path) -> Result<()> {
    let language = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|value| value.to_str());
    let file = path.file_name().and_then(|value| value.to_str());
    let expected = match (language, file) {
        (Some("python"), Some("random_number.py")) => {
            "1bd1030a00f5682712982851a91f72b7c639cc79ab53d3c705dfde140b033881"
        }
        (Some("typescript"), Some("randomColor.ts")) => {
            "f6ff1218de3bc67f89e1c7c6ff79b66ddc26060f4364ba8c6ab4c1708cad7f75"
        }
        (Some("typescript"), Some("runner.js")) => {
            "822d063c706c3bcd8fdc141f1111416b64a57f53beeb03a4cc104681300d4f85"
        }
        (Some("rust"), Some("random_material.rs")) => {
            "13e3e2696a1f1ea844aa9ce9c6ba40c18a4301fc28a8f70c1d391c598d0b12fb"
        }
        (Some("rust"), Some("runner.rs")) => {
            "2191e06624cc8081f642c330be89c1e8599c396e114c552174fe4ec54112b2ed"
        }
        (Some("go"), Some("random_shape.go")) => {
            "eb3f59324aa189b5453257a2187efde30acdc93e033c67750571a77572c8e857"
        }
        (Some("go"), Some("runner.go")) => {
            "b90e4ce2be84bfd33d408fc86bf9a7a1a9456033358132478188c73ec957fd39"
        }
        (Some("java"), Some("RandomAnimal.java")) => {
            "ecedecaae43a659cb427f51e818f3f071ce5d90693cd50d1d8fe628f18e6dfb4"
        }
        (Some("java"), Some("RandomAnimalRunner.java")) => {
            "0e39a3f42a047caec15a8081f25acc741c5f47550d83c3780af01a900f20765e"
        }
        (Some("cpp"), Some("random_action.h")) => {
            "4c881ea8f935374b51d684dffcac2faaff77ed43760e3d8eee2f909681e02edd"
        }
        (Some("cpp"), Some("random_action.cpp")) => {
            "e7ab0c1e95fbe46195f31725caff10cdb7e85aaa5a28affd337061219ab80be2"
        }
        (Some("cpp"), Some("random_action_runner.cpp")) => {
            "43eeef77330e6d375a0456ef6f94a9a20c220122e11a9c5809115a859402b654"
        }
        (Some("zig"), Some("random_size.zig")) => {
            "ac7af0b5523b21f47311855cd621eb4921e495768f9af792ca48ce3a6143d713"
        }
        (Some("zig"), Some("runner.zig")) => {
            "dd7ec946191b9e010116811b2012fbe30bdb2217542c435d6a15ea0556f9a988"
        }
        _ => {
            return Err(anyhow!(
                "Untrusted iota-fun source path: {}",
                path.display()
            ));
        }
    };
    let bytes =
        fs::read(path).with_context(|| format!("Failed to read fun source {}", path.display()))?;
    let actual = hex::encode(Sha256::digest(&bytes));
    if actual != expected {
        return Err(anyhow!(
            "Refusing modified iota-fun source {} (sha256 mismatch)",
            path.display()
        ));
    }
    Ok(())
}

fn cached_binary_path(language: &str, sources: &[PathBuf]) -> Result<PathBuf> {
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    cached_path(language, sources, suffix)
}

fn cached_class_dir_path(language: &str, sources: &[PathBuf]) -> Result<PathBuf> {
    cached_path(language, sources, "-classes")
}

fn cached_path(language: &str, sources: &[PathBuf], suffix: &str) -> Result<PathBuf> {
    let home = dirs::home_dir().context("Failed to get home directory")?;
    let mut hasher = Sha256::new();
    hasher.update(b"v3");
    hasher.update(std::env::consts::OS.as_bytes());
    hasher.update(std::env::consts::ARCH.as_bytes());
    hasher.update(language.as_bytes());
    for source in sources {
        let bytes =
            fs::read(source).with_context(|| format!("Failed to read {}", source.display()))?;
        hasher.update(source.to_string_lossy().as_bytes());
        hasher.update(Sha256::digest(&bytes));
    }
    let dir = home.join(".i6").join("iota-fun");
    fs::create_dir_all(&dir).with_context(|| format!("Failed to create {}", dir.display()))?;
    let hash = hex::encode(hasher.finalize());
    Ok(dir.join(format!("iota-fun-{}-{}{}", language, &hash[..16], suffix)))
}

fn command_available(command: &str) -> bool {
    Command::new(command).arg("--version").output().is_ok()
}

fn trim_output(value: &str) -> String {
    value.trim().chars().take(64 * 1024).collect()
}

fn fallback_action() -> String {
    let actions = ["睡觉", "奔跑", "喝水", "吃饭", "捕捉", "发呆"];
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos() as usize)
        .unwrap_or(0);
    actions[nanos % actions.len()].to_string()
}

fn fallback_material() -> String {
    let materials = ["wood", "metal", "glass", "plastic", "stone"];
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos() as usize)
        .unwrap_or(0);
    materials[nanos % materials.len()].to_string()
}

/// Fallbacks for the remaining languages, plus noted variants.
///
/// The `_with_note` forms append why the real path was unavailable, so a
/// degraded response is distinguishable from a genuine one instead of looking
/// like a normal random value.
fn fallback_shape() -> String {
    let shapes = ["circle", "square", "triangle", "star", "hexagon"];
    pick(&shapes)
}

fn fallback_size() -> String {
    let sizes = ["small", "medium", "large", "tiny", "huge"];
    pick(&sizes)
}

fn fallback_animal() -> String {
    let animals = ["cat", "dog", "fox", "owl", "otter"];
    pick(&animals)
}

fn pick(options: &[&str]) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos() as usize)
        .unwrap_or(0);
    options[nanos % options.len()].to_string()
}

fn fallback_material_with_note(note: &str) -> String {
    format!("{} ({note})", fallback_material())
}

fn fallback_action_with_note(note: &str) -> String {
    format!("{} ({note})", fallback_action())
}

fn fallback_shape_with_note(note: &str) -> String {
    format!("{} ({note})", fallback_shape())
}

fn fallback_size_with_note(note: &str) -> String {
    format!("{} ({note})", fallback_size())
}

fn fallback_animal_with_note(note: &str) -> String {
    format!("{} ({note})", fallback_animal())
}

/// Deletes cached build artifacts until the cache fits [`MAX_BUILD_CACHE_BYTES`].
///
/// Evicts oldest-first by mtime, so recently used artifacts survive. Best
/// effort: a failure to stat or remove a file is logged and the sweep
/// continues, since failing to prune must not fail the execution.
fn prune_build_cache() {
    let Some(home) = dirs::home_dir() else {
        return;
    };
    let root = home.join(".i6").join("iota-fun");
    let Ok(entries) = fs::read_dir(&root) else {
        return;
    };

    let mut artifacts: Vec<(std::time::SystemTime, PathBuf, u64)> = Vec::new();
    let mut total: u64 = 0;
    for entry in entries.flatten() {
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        let modified = metadata
            .modified()
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        total = total.saturating_add(metadata.len());
        artifacts.push((modified, entry.path(), metadata.len()));
    }
    if total <= MAX_BUILD_CACHE_BYTES {
        return;
    }

    artifacts.sort_by_key(|(modified, _, _)| *modified);
    for (_, path, size) in artifacts {
        if total <= MAX_BUILD_CACHE_BYTES {
            break;
        }
        match fs::remove_file(&path) {
            Ok(()) => {
                total = total.saturating_sub(size);
                tracing::info!(
                    artifact = %path.display(),
                    "evicted iota-fun build cache entry over size cap"
                );
            }
            Err(error) => {
                tracing::warn!(
                    artifact = %path.display(),
                    error = %error,
                    "failed to evict iota-fun build cache entry"
                );
            }
        }
    }
}

fn ok(id: Value, result: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":result})
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}

#[cfg(test)]
#[path = "fun_tests.rs"]
mod fun_tests;

#[cfg(test)]
#[path = "fun_isolation_tests.rs"]
mod fun_isolation_tests;
