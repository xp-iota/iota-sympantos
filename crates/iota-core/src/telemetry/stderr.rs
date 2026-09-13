use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::PathBuf;

use tracing_subscriber::Layer;
use tracing_subscriber::fmt;

/// The stderr log layer.
///
/// Writes through [`super::redact::RedactingStderr`] so a credential that
/// reached a log field — from a backend error quoting its request, a debug
/// format of a config struct, or a panic payload — does not reach the terminal
/// or a captured log file.
pub enum LogWriter {
    Stderr,
    File(PathBuf),
}

pub fn log_layer<S>(writer: LogWriter) -> impl Layer<S>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    fmt::layer()
        .with_writer(writer)
        // ANSI escapes are disabled because the redacting writer inspects the
        // formatted text: colour codes would split a secret across styled
        // spans and defeat matching.
        .with_ansi(false)
        .with_target(true)
        .with_level(true)
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogWriter {
    type Writer = LogWriterHandle;

    fn make_writer(&'a self) -> Self::Writer {
        LogWriterHandle {
            writer: match self {
                Self::Stderr => LogWriterHandleKind::Stderr,
                Self::File(path) => LogWriterHandleKind::File(path.clone()),
            },
        }
    }
}

pub struct LogWriterHandle {
    writer: LogWriterHandleKind,
}

enum LogWriterHandleKind {
    Stderr,
    File(PathBuf),
}

impl Write for LogWriterHandle {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let text = String::from_utf8_lossy(buf);
        let redacted = super::redact::redact(&text);
        match &self.writer {
            LogWriterHandleKind::Stderr => {
                io::stderr().write_all(redacted.as_bytes())?;
            }
            LogWriterHandleKind::File(path) => {
                let mut file = OpenOptions::new().create(true).append(true).open(path)?;
                file.write_all(redacted.as_bytes())?;
            }
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        match &self.writer {
            LogWriterHandleKind::Stderr => io::stderr().flush(),
            LogWriterHandleKind::File(path) => {
                let mut file = OpenOptions::new().create(true).append(true).open(path)?;
                file.flush()
            }
        }
    }
}
