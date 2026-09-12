pub(super) use crate::utils::elapsed_ms;

pub(super) fn should_forward_backend_stderr(line: &str) -> bool {
    line.contains("context MCP memory")
        || line.contains("iota::context::server")
        || line.contains("[iota log]")
        || line.contains("[mcp stderr:")
}
