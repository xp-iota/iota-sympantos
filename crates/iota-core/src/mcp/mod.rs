//! MCP protocol layer.
//!
//! - [`client`] — spawns and communicates with stdio MCP sidecar processes
//! - [`server`] — stdio JSON-RPC MCP server (`iota mcp context`)
//! - [`router`] — intercepts `iota_*` tool calls in the ACP stream
//! - [`tool_dispatch`] — shared tool execution logic used by both server and router

use std::io::{self, BufRead};

pub(crate) const MAX_MCP_MESSAGE_BYTES: usize = 1024 * 1024;

pub(crate) fn read_limited_line<R: BufRead>(reader: &mut R) -> io::Result<Option<String>> {
    let mut bytes = Vec::new();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return if bytes.is_empty() {
                Ok(None)
            } else {
                String::from_utf8(bytes)
                    .map(Some)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
            };
        }
        let end = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |index| index + 1);
        if bytes.len().saturating_add(end) > MAX_MCP_MESSAGE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("MCP message exceeds {MAX_MCP_MESSAGE_BYTES} byte limit"),
            ));
        }
        let complete = available[end - 1] == b'\n';
        bytes.extend_from_slice(&available[..end]);
        reader.consume(end);
        if complete {
            bytes.pop();
            if bytes.last() == Some(&b'\r') {
                bytes.pop();
            }
            return String::from_utf8(bytes)
                .map(Some)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error));
        }
    }
}
pub mod client;
pub mod router;
pub mod server;
pub(crate) mod tool_dispatch;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_line_reader_rejects_oversized_input() {
        let mut input = vec![b'x'; MAX_MCP_MESSAGE_BYTES + 1];
        input.push(b'\n');
        let mut reader = io::Cursor::new(input);
        assert!(read_limited_line(&mut reader).is_err());
    }
}
