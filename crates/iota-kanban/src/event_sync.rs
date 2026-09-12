//! Cross-node event synchronization for the Kanban store.
//!
//! # Event identity
//!
//! An event's identity is its immutable [`KanbanEvent::event_uuid`], not its
//! local `id`. Each node numbers its own event log independently, so the same
//! numeric id means different events on different nodes. Import deduplicates on
//! the UUID, which makes re-importing an overlapping bundle a no-op rather than
//! a duplicate.
//!
//! # Cursors
//!
//! Progress is tracked per `(source_id, source_sequence)`: a producer numbers
//! *its own* events, and a consumer remembers the highest sequence it has seen
//! from that producer. There is deliberately no global event counter — an
//! earlier design required event ids to be globally contiguous, which cannot
//! hold once more than one node produces events.
//!
//! # Trust boundary
//!
//! `serve-sync` accepts only loopback connections and requires the daemon owner
//! token on **every** request, including reads. A sync peer can import events
//! into the local store, so an unauthenticated caller on the same machine would
//! otherwise be able to rewrite Kanban state.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::{EventId, KanbanEvent, KanbanStore, SqliteKanbanStore};

/// Read/write timeout for each event-sync TCP connection.
const EVENT_SYNC_IO_TIMEOUT_SECS: u64 = 30;
const MAX_EVENT_SYNC_MESSAGE_BYTES: usize = 10 * 1024 * 1024;
const MAX_EVENT_SYNC_SOURCE_BYTES: usize = 256;

/// Current bundle format version.
///
/// v1 carried events with no uuid/hash/auth and assumed globally contiguous
/// ids. v2 is the only version written; v1 is readable solely through
/// [`migrate_v1_bundle`] so an operator can import an old export explicitly.
pub const FORMAT_VERSION: u32 = 2;

/// Bundle format version this build can still *read* for migration purposes.
pub const LEGACY_FORMAT_VERSION: u32 = 1;

/// Environment variable overriding the sync token path, mirroring
/// `IOTA_DAEMON_TOKEN_PATH` in `iota-core`.
pub const SYNC_TOKEN_PATH_ENV: &str = "IOTA_SYNC_TOKEN_PATH";

/// Minimum accepted token length in characters (hex-encoded 32 bytes).
const MIN_TOKEN_CHARS: usize = 64;

/// Producer metadata recorded in a bundle, so an importer can tell where a
/// bundle came from without trusting the `source_id` string alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleProducer {
    pub hostname: String,
    pub tool_version: String,
    pub os: String,
}

impl BundleProducer {
    pub fn local() -> Self {
        Self {
            hostname: hostname::get()
                .ok()
                .and_then(|value| value.into_string().ok())
                .unwrap_or_else(|| "unknown".to_string()),
            tool_version: env!("CARGO_PKG_VERSION").to_string(),
            os: std::env::consts::OS.to_string(),
        }
    }
}

/// A versioned, integrity-checked event bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KanbanEventBundle {
    /// Bundle format version. See [`FORMAT_VERSION`].
    pub format_version: u32,
    /// Identity of the producing node.
    pub source_id: String,
    /// Highest event sequence this bundle accounts for, in the *producer's*
    /// own numbering. A consumer stores this as its cursor for `source_id`.
    pub source_sequence: EventId,
    /// Where this bundle was produced (informational, unauthenticated).
    pub producer: BundleProducer,
    pub events: Vec<KanbanEvent>,
    /// SHA-256 over the canonical encoding of the fields above.
    ///
    /// Detects accidental corruption in transit or in a file. It is **not** a
    /// signature: anyone who can rewrite the bundle can recompute it. The
    /// authentication that actually matters is the token check at the transport
    /// layer; this only catches silent damage.
    pub bundle_hash: String,
}

/// The producer-supplied fields covered by [`KanbanEventBundle::bundle_hash`].
///
/// Kept separate so the hash and the bundle cannot drift: both are derived from
/// this one value.
#[derive(Serialize)]
struct HashedBundlePayload<'a> {
    format_version: u32,
    source_id: &'a str,
    source_sequence: EventId,
    producer: &'a BundleProducer,
    events: &'a [KanbanEvent],
}

impl KanbanEventBundle {
    /// Computes the integrity hash for the given contents.
    fn compute_hash(
        format_version: u32,
        source_id: &str,
        source_sequence: EventId,
        producer: &BundleProducer,
        events: &[KanbanEvent],
    ) -> Result<String> {
        let payload = HashedBundlePayload {
            format_version,
            source_id,
            source_sequence,
            producer,
            events,
        };
        let bytes = serde_json::to_vec(&payload).context("encoding bundle for hashing")?;
        use sha2::{Digest, Sha256};
        Ok(hex::encode(Sha256::digest(&bytes)))
    }

    /// Recomputes the hash and compares it with the carried one.
    pub fn verify_hash(&self) -> Result<()> {
        let expected = Self::compute_hash(
            self.format_version,
            &self.source_id,
            self.source_sequence,
            &self.producer,
            &self.events,
        )?;
        anyhow::ensure!(
            constant_time_eq(expected.as_bytes(), self.bundle_hash.as_bytes()),
            "kanban event bundle hash mismatch: bundle is corrupt or was modified after hashing"
        );
        Ok(())
    }

    /// Builds a correctly-hashed bundle from raw parts.
    ///
    /// Lets tests construct bundles — including deliberately unsupported
    /// versions — without duplicating the hashing rule.
    #[cfg(test)]
    pub(crate) fn for_tests_with_version(
        format_version: u32,
        source_id: &str,
        source_sequence: EventId,
        events: Vec<KanbanEvent>,
    ) -> Self {
        let producer = BundleProducer::local();
        let bundle_hash = Self::compute_hash(
            format_version,
            source_id,
            source_sequence,
            &producer,
            &events,
        )
        .expect("hashing test bundle");
        Self {
            format_version,
            source_id: source_id.to_string(),
            source_sequence,
            producer,
            events,
            bundle_hash,
        }
    }

    /// [`Self::for_tests_with_version`] at the current [`FORMAT_VERSION`].
    #[cfg(test)]
    pub(crate) fn for_tests(
        format_version: u32,
        source_id: &str,
        source_sequence: EventId,
        events: Vec<KanbanEvent>,
    ) -> Self {
        Self::for_tests_with_version(format_version, source_id, source_sequence, events)
    }
}

/// Builds a bundle covering every event after `after_sequence`.
///
/// `after_sequence` is the consumer's last-seen sequence from this producer.
/// Because a node's own log is contiguous, the producer's current sequence is
/// simply its highest local event id.
pub fn export_event_bundle(
    store: &dyn KanbanStore,
    after_sequence: EventId,
    source_id: impl Into<String>,
) -> Result<KanbanEventBundle> {
    let source_id = source_id.into();
    anyhow::ensure!(
        !source_id.trim().is_empty() && source_id.len() <= MAX_EVENT_SYNC_SOURCE_BYTES,
        "kanban event bundle source must be 1..={MAX_EVENT_SYNC_SOURCE_BYTES} bytes"
    );

    // Event ids are this store's local log positions, so reading past the
    // consumer's sequence yields exactly the events it has not seen.
    let events = store.events_since(after_sequence)?;
    let source_sequence = events
        .last()
        .map(|event| event.id)
        .unwrap_or(after_sequence);
    let producer = BundleProducer::local();
    let bundle_hash = KanbanEventBundle::compute_hash(
        FORMAT_VERSION,
        &source_id,
        source_sequence,
        &producer,
        &events,
    )?;
    Ok(KanbanEventBundle {
        format_version: FORMAT_VERSION,
        source_id,
        source_sequence,
        producer,
        events,
        bundle_hash,
    })
}

pub fn write_event_bundle(path: &Path, bundle: &KanbanEventBundle) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating kanban event bundle dir {}", parent.display()))?;
    }
    let json = serde_json::to_vec_pretty(bundle)?;
    anyhow::ensure!(
        json.len() <= MAX_EVENT_SYNC_MESSAGE_BYTES,
        "kanban event bundle exceeds {MAX_EVENT_SYNC_MESSAGE_BYTES} byte limit"
    );
    fs::write(path, json).with_context(|| format!("writing kanban event bundle {}", path.display()))
}

/// Reads and validates a v2 bundle from disk.
///
/// A v1 file fails schema deserialization, so the version is sniffed first and
/// reported as an unsupported version with the migration command — otherwise an
/// operator would get an opaque "missing field `source_id`" error.
pub fn read_event_bundle(path: &Path) -> Result<KanbanEventBundle> {
    let bytes = read_bundle_bytes(path)?;
    ensure_supported_bundle_version(&bytes, path)?;
    let bundle: KanbanEventBundle = serde_json::from_slice(&bytes)
        .with_context(|| format!("parsing kanban event bundle {}", path.display()))?;
    bundle.verify_hash()?;
    validate_event_bundle(&bundle)?;
    Ok(bundle)
}

/// Rejects a bundle whose declared `format_version` predates [`FORMAT_VERSION`].
///
/// Sniffs only the version field, so it works on the older schema too.
fn ensure_supported_bundle_version(bytes: &[u8], path: &Path) -> Result<()> {
    #[derive(Deserialize)]
    struct VersionProbe {
        format_version: u32,
    }
    let Ok(probe) = serde_json::from_slice::<VersionProbe>(bytes) else {
        // Not even a version field: let the real parser produce its error.
        return Ok(());
    };
    anyhow::ensure!(
        probe.format_version == FORMAT_VERSION,
        "unsupported kanban event bundle version {} (expected {FORMAT_VERSION}) at {}; \
         bundles written before v2 must be imported with `iota kanban migrate-events`",
        probe.format_version,
        path.display()
    );
    Ok(())
}

fn read_bundle_bytes(path: &Path) -> Result<Vec<u8>> {
    let file = fs::File::open(path)
        .with_context(|| format!("opening kanban event bundle {}", path.display()))?;
    let size = file
        .metadata()
        .with_context(|| format!("reading kanban event bundle metadata {}", path.display()))?
        .len();
    anyhow::ensure!(
        size <= MAX_EVENT_SYNC_MESSAGE_BYTES as u64,
        "kanban event bundle exceeds {MAX_EVENT_SYNC_MESSAGE_BYTES} byte limit"
    );
    let mut bytes = Vec::with_capacity(size as usize);
    file.take(MAX_EVENT_SYNC_MESSAGE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("reading kanban event bundle {}", path.display()))?;
    anyhow::ensure!(
        bytes.len() <= MAX_EVENT_SYNC_MESSAGE_BYTES,
        "kanban event bundle exceeds {MAX_EVENT_SYNC_MESSAGE_BYTES} byte limit"
    );
    Ok(bytes)
}

/// A v1 bundle, retained only so old exports can be migrated.
#[derive(Debug, Clone, Deserialize)]
struct LegacyEventBundle {
    #[allow(dead_code)]
    format_version: u32,
    source: String,
    cursor: EventId,
    events: Vec<LegacyEvent>,
}

#[derive(Debug, Clone, Deserialize)]
struct LegacyEvent {
    id: EventId,
    event_type: String,
    payload: String,
    created_at: i64,
}

/// Reads a v1 bundle and converts it to a v2 bundle.
///
/// v1 events carry no UUID, so each is assigned a fresh one at migration time.
/// That is safe for a one-shot migration (there is no prior UUID to conflict
/// with) but means the converted events cannot be deduplicated against another
/// migration of the same file — migrating the same export twice would import
/// its events twice. The caller must therefore migrate once.
pub fn migrate_v1_bundle(path: &Path) -> Result<KanbanEventBundle> {
    let legacy: LegacyEventBundle = serde_json::from_slice(&read_bundle_bytes(path)?)
        .with_context(|| format!("parsing legacy kanban event bundle {}", path.display()))?;
    anyhow::ensure!(
        legacy.format_version == LEGACY_FORMAT_VERSION,
        "bundle at {} is version {}, not the legacy version {}",
        path.display(),
        legacy.format_version,
        LEGACY_FORMAT_VERSION
    );

    let events: Vec<KanbanEvent> = legacy
        .events
        .into_iter()
        .map(|event| KanbanEvent {
            id: event.id,
            event_uuid: uuid::Uuid::new_v4(),
            event_type: event.event_type,
            payload: event.payload,
            created_at: event.created_at,
        })
        .collect();

    let producer = BundleProducer::local();
    let source_sequence = events.last().map(|event| event.id).unwrap_or(legacy.cursor);
    let bundle_hash = KanbanEventBundle::compute_hash(
        FORMAT_VERSION,
        &legacy.source,
        source_sequence,
        &producer,
        &events,
    )?;
    let bundle = KanbanEventBundle {
        format_version: FORMAT_VERSION,
        source_id: legacy.source,
        source_sequence,
        producer,
        events,
        bundle_hash,
    };
    validate_event_bundle(&bundle)?;
    Ok(bundle)
}

/// Structural validation of a bundle's contents.
///
/// Unlike the v1 validator this does **not** require globally contiguous event
/// ids: ids are per-store log positions, so gaps are expected once a node has
/// synced from elsewhere. What it does require is that a producer's events are
/// strictly increasing within one bundle (they are read from one log in order).
fn validate_event_bundle(bundle: &KanbanEventBundle) -> Result<()> {
    anyhow::ensure!(
        !bundle.source_id.trim().is_empty()
            && bundle.source_id.len() <= MAX_EVENT_SYNC_SOURCE_BYTES,
        "kanban event bundle source must be 1..={MAX_EVENT_SYNC_SOURCE_BYTES} bytes"
    );
    anyhow::ensure!(
        bundle.source_sequence <= i64::MAX as u64,
        "kanban event bundle sequence exceeds SQLite integer range"
    );
    anyhow::ensure!(
        bundle.format_version == FORMAT_VERSION,
        "unsupported kanban event bundle version {}",
        bundle.format_version
    );

    let mut previous = None;
    for event in &bundle.events {
        anyhow::ensure!(
            event.id > 0 && event.id <= i64::MAX as u64,
            "kanban event id {} is outside SQLite integer range",
            event.id
        );
        anyhow::ensure!(
            !event.event_uuid.is_nil(),
            "kanban event {} has no uuid",
            event.id
        );
        if let Some(previous) = previous {
            anyhow::ensure!(
                event.id > previous,
                "kanban event ids must be strictly increasing within a bundle"
            );
        }
        previous = Some(event.id);
    }

    if let Some(last) = bundle.events.last() {
        anyhow::ensure!(
            bundle.source_sequence == last.id,
            "kanban event bundle sequence {} does not match last event {}",
            bundle.source_sequence,
            last.id
        );
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventImportReport {
    pub source: String,
    pub events_seen: usize,
    pub events_applied: usize,
    pub events_skipped: usize,
    pub cursor: EventId,
}

/// A request over the sync transport.
///
/// Every variant carries `auth_token`: a sync peer can import events into the
/// local store, so reads are authenticated too — a read leaks the full event
/// log, which is the same data a write could reconstruct.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
enum EventSyncRequest {
    EventsSince {
        /// Consumer's last-seen sequence from `source`, in the *producer's*
        /// numbering.
        cursor: EventId,
        source: String,
        #[serde(default)]
        auth_token: Option<String>,
    },
    ImportBundle {
        bundle: KanbanEventBundle,
        #[serde(default)]
        auth_token: Option<String>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct EventSyncResponse {
    ok: bool,
    bundle: Option<KanbanEventBundle>,
    report: Option<EventImportReport>,
    error: Option<String>,
}

impl EventSyncResponse {
    fn failure(error: impl Into<String>) -> Self {
        Self {
            ok: false,
            bundle: None,
            report: None,
            error: Some(error.into()),
        }
    }
}

/// Imports a validated bundle into `store`.
pub fn import_event_bundle(
    store: &SqliteKanbanStore,
    bundle: &KanbanEventBundle,
) -> Result<EventImportReport> {
    anyhow::ensure!(
        bundle.format_version == FORMAT_VERSION,
        "unsupported kanban event bundle version: {}",
        bundle.format_version
    );
    bundle.verify_hash()?;
    validate_event_bundle(bundle)?;

    let events_seen = bundle.events.len();
    // Deduplication happens inside the atomic import, on each event's uuid;
    // the cursor is per-source so it never has to be comparable across nodes.
    let events_applied = store.import_event_bundle_atomic(
        &bundle.events,
        &bundle.source_id,
        bundle.source_sequence,
    )?;
    let events_skipped = events_seen.saturating_sub(events_applied);
    Ok(EventImportReport {
        source: bundle.source_id.clone(),
        events_seen,
        events_applied,
        events_skipped,
        cursor: bundle.source_sequence,
    })
}

pub fn default_pull_source(addr: &str) -> String {
    let trimmed = addr.trim();
    let source = if trimmed.is_empty() {
        "unknown"
    } else {
        trimmed
    };
    format!("peer:{source}")
}

/// Resolves the daemon/sync owner token path.
///
/// Honours [`SYNC_TOKEN_PATH_ENV`] for tests and explicit overrides, otherwise
/// uses `<home>/.i6/daemon.token` — the same file the daemon uses, so a machine
/// has one local trust credential rather than a second one to manage.
pub fn sync_token_path() -> Result<PathBuf> {
    if let Ok(path) = std::env::var(SYNC_TOKEN_PATH_ENV) {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }
    let home = dirs::home_dir().context("failed to resolve home directory")?;
    Ok(home.join(".i6").join("daemon.token"))
}

/// Loads the owner token. Returns `None` when no token file exists.
pub fn load_sync_token() -> Result<Option<String>> {
    let path = sync_token_path()?;
    match fs::read_to_string(&path) {
        Ok(contents) => {
            let token = contents.trim().to_string();
            if token.is_empty() {
                Ok(None)
            } else {
                Ok(Some(token))
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("reading sync token {}", path.display())),
    }
}

/// Compares two tokens without leaking their contents through timing.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Verifies a presented token against `expected`.
///
/// A token shorter than [`MIN_TOKEN_CHARS`] is rejected regardless of match, so
/// a truncated or placeholder value cannot be accepted by accident.
fn token_is_valid(presented: Option<&str>, expected: &str) -> bool {
    let Some(presented) = presented else {
        return false;
    };
    if presented.len() < MIN_TOKEN_CHARS {
        return false;
    }
    constant_time_eq(presented.as_bytes(), expected.as_bytes())
}

/// Why a sync request was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncRejectionReason {
    Unauthenticated,
    UnsupportedVersion,
    InvalidRequest,
}

impl SyncRejectionReason {
    /// Stable wire/diagnostic string.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unauthenticated => "unauthenticated",
            Self::UnsupportedVersion => "unsupported_version",
            Self::InvalidRequest => "invalid_request",
        }
    }
}

/// Serves sync requests until the listener is dropped.
///
/// `expected_token` must be the owner token: every request, read or write, is
/// rejected without it.
pub fn serve_event_sync_with_token<A: ToSocketAddrs>(
    store: Arc<SqliteKanbanStore>,
    addr: A,
    expected_token: String,
) -> Result<()> {
    let listener = bind_sync_listener(addr)?;
    for stream in listener.incoming() {
        let stream = stream.context("accepting kanban event sync connection")?;
        if let Err(error) = handle_event_sync_stream(store.as_ref(), stream, &expected_token) {
            eprintln!("kanban sync connection failed: {error:#}");
        }
    }
    Ok(())
}

/// Binds the sync listener, refusing any non-loopback address.
///
/// Exposed so tests can bound a server without going through the accept loop.
pub fn bind_sync_listener<A: ToSocketAddrs>(addr: A) -> Result<TcpListener> {
    let bind_addr = addr
        .to_socket_addrs()
        .context("resolving kanban event sync address")?
        .next()
        .context("kanban event sync address did not resolve")?;
    anyhow::ensure!(
        bind_addr.ip().is_loopback(),
        "refusing to expose kanban sync outside loopback"
    );
    TcpListener::bind(bind_addr).context("binding kanban event sync listener")
}

/// Serves sync requests, loading the owner token from disk.
///
/// Fails closed: with no token available there is nothing to authenticate
/// against, so the server refuses to start rather than accept unauthenticated
/// writes.
pub fn serve_event_sync<A: ToSocketAddrs>(store: Arc<SqliteKanbanStore>, addr: A) -> Result<()> {
    let token = load_sync_token()?.context(
        "no kanban sync token found; run the daemon once to generate ~/.i6/daemon.token, \
         or set IOTA_SYNC_TOKEN_PATH",
    )?;
    serve_event_sync_with_token(store, addr, token)
}

pub fn pull_event_bundle<A: ToSocketAddrs>(
    addr: A,
    cursor: EventId,
    source: impl Into<String>,
) -> Result<KanbanEventBundle> {
    let request = EventSyncRequest::EventsSince {
        cursor,
        source: source.into(),
        auth_token: load_sync_token()?,
    };
    let response = send_event_sync_request(addr, &request)?;
    if response.ok {
        let bundle = response
            .bundle
            .context("kanban sync peer did not return an event bundle")?;
        bundle.verify_hash()?;
        Ok(bundle)
    } else {
        anyhow::bail!(
            "kanban sync pull failed: {}",
            response
                .error
                .unwrap_or_else(|| "unknown error".to_string())
        )
    }
}

pub fn push_event_bundle<A: ToSocketAddrs>(
    addr: A,
    bundle: KanbanEventBundle,
) -> Result<EventImportReport> {
    let request = EventSyncRequest::ImportBundle {
        bundle,
        auth_token: load_sync_token()?,
    };
    let response = send_event_sync_request(addr, &request)?;
    if response.ok {
        response
            .report
            .context("kanban sync peer did not return an import report")
    } else {
        anyhow::bail!(
            "kanban sync push failed: {}",
            response
                .error
                .unwrap_or_else(|| "unknown error".to_string())
        )
    }
}

fn send_event_sync_request<A: ToSocketAddrs>(
    addr: A,
    request: &EventSyncRequest,
) -> Result<EventSyncResponse> {
    let timeout = std::time::Duration::from_secs(EVENT_SYNC_IO_TIMEOUT_SECS);
    let mut last_error = None;
    let mut connected = None;
    for peer in addr
        .to_socket_addrs()
        .context("resolving kanban sync peer")?
    {
        match TcpStream::connect_timeout(&peer, timeout) {
            Ok(stream) => {
                connected = Some(stream);
                break;
            }
            Err(error) => last_error = Some(error),
        }
    }
    let mut stream = connected.with_context(|| match last_error {
        Some(error) => format!("connecting to kanban sync peer: {error}"),
        None => "kanban sync peer address did not resolve".to_string(),
    })?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;

    let request_json = serde_json::to_vec(request)?;
    anyhow::ensure!(
        request_json.len() < MAX_EVENT_SYNC_MESSAGE_BYTES,
        "kanban sync request exceeded {MAX_EVENT_SYNC_MESSAGE_BYTES} byte limit"
    );
    stream.write_all(&request_json)?;
    stream.write_all(b"\n")?;
    stream.flush()?;

    let line = read_limited_line(BufReader::new(stream))?;
    serde_json::from_str(&line).context("parsing kanban sync peer response")
}

/// Handles one connection: authenticate, then process the request.
///
/// Authentication happens before the request is interpreted, so an
/// unauthenticated peer learns nothing about the store — not even whether the
/// request was well-formed.
pub fn handle_event_sync_stream(
    store: &SqliteKanbanStore,
    mut stream: TcpStream,
    expected_token: &str,
) -> Result<()> {
    let timeout = Some(std::time::Duration::from_secs(EVENT_SYNC_IO_TIMEOUT_SECS));
    let _ = stream.set_read_timeout(timeout);
    let _ = stream.set_write_timeout(timeout);

    let line = read_limited_line(BufReader::new(stream.try_clone()?))?;
    let response = match serde_json::from_str::<EventSyncRequest>(&line) {
        Ok(request) => {
            let presented = match &request {
                EventSyncRequest::EventsSince { auth_token, .. }
                | EventSyncRequest::ImportBundle { auth_token, .. } => auth_token.as_deref(),
            };
            if !token_is_valid(presented, expected_token) {
                EventSyncResponse::failure(format!(
                    "kanban sync rejected: {}",
                    SyncRejectionReason::Unauthenticated.as_str()
                ))
            } else {
                handle_event_sync_request(store, request)
            }
        }
        Err(err) => EventSyncResponse::failure(format!(
            "kanban sync rejected: {}: {err}",
            SyncRejectionReason::InvalidRequest.as_str()
        )),
    };
    let mut response_json = serde_json::to_vec(&response)?;
    if response_json.len() >= MAX_EVENT_SYNC_MESSAGE_BYTES {
        response_json = serde_json::to_vec(&EventSyncResponse::failure(
            "kanban sync response exceeded message limit",
        ))?;
    }
    stream.write_all(&response_json)?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    // Graceful half-close: signal EOF to the client so it can finish reading
    // before the OS drops the connection.
    let _ = stream.shutdown(Shutdown::Write);
    Ok(())
}

fn read_limited_line<R: BufRead>(mut reader: R) -> Result<String> {
    let mut bytes = Vec::new();
    reader
        .by_ref()
        .take(MAX_EVENT_SYNC_MESSAGE_BYTES as u64 + 1)
        .read_until(b'\n', &mut bytes)?;
    anyhow::ensure!(
        bytes.len() <= MAX_EVENT_SYNC_MESSAGE_BYTES,
        "kanban sync message exceeded {MAX_EVENT_SYNC_MESSAGE_BYTES} byte limit"
    );
    String::from_utf8(bytes).context("kanban sync message was not valid UTF-8")
}

fn handle_event_sync_request(
    store: &SqliteKanbanStore,
    request: EventSyncRequest,
) -> EventSyncResponse {
    match request {
        EventSyncRequest::EventsSince { cursor, source, .. } => {
            match export_event_bundle(store, cursor, source) {
                Ok(bundle) => EventSyncResponse {
                    ok: true,
                    bundle: Some(bundle),
                    report: None,
                    error: None,
                },
                Err(err) => EventSyncResponse::failure(err.to_string()),
            }
        }
        EventSyncRequest::ImportBundle { bundle, .. } => {
            match import_event_bundle(store, &bundle) {
                Ok(report) => EventSyncResponse {
                    ok: true,
                    bundle: None,
                    report: Some(report),
                    error: None,
                },
                Err(err) => EventSyncResponse::failure(err.to_string()),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "event_sync_tests.rs"]
mod tests;
