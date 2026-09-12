use tracing_subscriber::Layer;
use tracing_subscriber::fmt;

/// The stderr log layer.
///
/// Writes through [`super::redact::RedactingStderr`] so a credential that
/// reached a log field — from a backend error quoting its request, a debug
/// format of a config struct, or a panic payload — does not reach the terminal
/// or a captured log file.
pub fn stderr_layer<S>() -> impl Layer<S>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    fmt::layer()
        .with_writer(super::redact::RedactingStderr)
        // ANSI escapes are disabled because the redacting writer inspects the
        // formatted text: colour codes would split a secret across styled
        // spans and defeat matching.
        .with_ansi(false)
        .with_target(true)
        .with_level(true)
}
