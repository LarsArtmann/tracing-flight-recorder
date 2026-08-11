//! Ring-buffer flight recorder and `tracing_subscriber::Layer` implementation.

use crate::capture::{
    CapturedEvent, DumpEvent, DumpSource, FieldVisitor, FlightRecorderDump, SpanContext,
    DUMP_SCHEMA_VERSION,
};
use crate::trigger::Trigger;
use crate::DEFAULT_CAPACITY;
use std::borrow::Cow;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;

/// Shared, thread-safe dump-observability callback.
type DumpHook = Arc<dyn Fn(&DumpEvent) + Send + Sync>;

/// A bounded in-memory ring buffer of tracing events.
///
/// Clone this handle freely — all clones share the same underlying buffer
/// (and the same [`on_dump`](Self::with_on_dump) callback, if set).
/// See [`FlightRecorderLayer`] for how to connect this to a `tracing` subscriber.
#[derive(Clone)]
pub struct FlightRecorder {
    buffer: Arc<Mutex<VecDeque<CapturedEvent>>>,
    capacity: usize,
    on_dump: Option<DumpHook>,
}

impl FlightRecorder {
    /// Create a new flight recorder with the given capacity (max events retained).
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: Arc::new(Mutex::new(VecDeque::with_capacity(capacity))),
            capacity,
            on_dump: None,
        }
    }

    /// Create a new flight recorder with the default capacity ([`DEFAULT_CAPACITY`]).
    #[must_use]
    pub fn with_default_capacity() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }

    /// Register a callback invoked after every dump that persists to a file
    /// (manual dumps, retention dumps, and automatic trigger dumps).
    ///
    /// All clones of this recorder share the same callback. The callback is
    /// invoked with a [`DumpEvent`] carrying the destination path, byte count,
    /// wall-clock duration, trigger reason, and source (manual vs trigger). It
    /// is best-effort: a panicking callback is contained and never propagates
    /// back into the recording or trigger path.
    ///
    /// ```no_run
    /// # use tracing_flight_recorder::FlightRecorder;
    /// let recorder = FlightRecorder::new(1000)
    ///     .with_on_dump(|ev| eprintln!("wrote {} bytes to {:?}", ev.bytes_written, ev.path));
    /// ```
    #[must_use]
    pub fn with_on_dump<F>(mut self, hook: F) -> Self
    where
        F: Fn(&DumpEvent) + Send + Sync + 'static,
    {
        self.on_dump = Some(Arc::new(hook));
        self
    }

    /// Push a captured event into the ring buffer, evicting the oldest if at capacity.
    pub(crate) fn push(&self, event: CapturedEvent) {
        if self.capacity == 0 {
            return;
        }
        let mut buf = self
            .buffer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if buf.len() >= self.capacity {
            buf.pop_front();
        }
        buf.push_back(event);
    }

    /// Return a snapshot of all buffered events in insertion order (oldest first).
    #[must_use]
    pub fn snapshot(&self) -> Vec<CapturedEvent> {
        self.buffer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .cloned()
            .collect()
    }

    /// Serialize the buffer to a **compact** JSON string.
    ///
    /// Compact is the default because flight-recorder snapshots are often
    /// persisted automatically (trigger dumps, retention pruning) where file
    /// size matters. For human-readable output use [`dump_to_json_pretty`](Self::dump_to_json_pretty).
    ///
    /// # Errors
    ///
    /// Returns `serde_json::Error` if serialization fails.
    pub fn dump_to_json(&self) -> serde_json::Result<String> {
        let events = self.snapshot();
        serde_json::to_string(&events)
    }

    /// Serialize the buffer to a pretty-printed (indented) JSON string.
    ///
    /// Like [`dump_to_json`](Self::dump_to_json) but with whitespace for
    /// human reading. Roughly 2-3× larger than the compact form.
    ///
    /// # Errors
    ///
    /// Returns `serde_json::Error` if serialization fails.
    pub fn dump_to_json_pretty(&self) -> serde_json::Result<String> {
        let events = self.snapshot();
        serde_json::to_string_pretty(&events)
    }

    /// Write the buffer as **compact** JSON to any writer.
    ///
    /// Streams directly to the writer without buffering the full JSON string,
    /// making it suitable for large buffers or network sinks. For indented
    /// output use [`dump_to_writer_pretty`](Self::dump_to_writer_pretty).
    ///
    /// # Errors
    ///
    /// Returns `io::Error` if serialization or writing fails.
    pub fn dump_to_writer(&self, writer: &mut dyn std::io::Write) -> std::io::Result<()> {
        let events = self.snapshot();
        serde_json::to_writer(writer, &events).map_err(std::io::Error::other)
    }

    /// Write the buffer as pretty-printed (indented) JSON to any writer.
    ///
    /// # Errors
    ///
    /// Returns `io::Error` if serialization or writing fails.
    pub fn dump_to_writer_pretty(&self, writer: &mut dyn std::io::Write) -> std::io::Result<()> {
        let events = self.snapshot();
        serde_json::to_writer_pretty(writer, &events).map_err(std::io::Error::other)
    }

    /// Serialize the buffer as JSON Lines (NDJSON) — one compact JSON object per line.
    ///
    /// Each line is a self-contained JSON object representing a single event.
    /// This format is streamable, appendable, and ingestible by log pipelines
    /// (e.g. `jq`, Elasticsearch, Datadog) without a full-array parse.
    ///
    /// # Errors
    ///
    /// Returns `serde_json::Error` if serialization fails.
    pub fn dump_to_json_lines(&self) -> serde_json::Result<String> {
        let events = self.snapshot();
        let mut output = String::new();
        for event in &events {
            let line = serde_json::to_string(event)?;
            output.push_str(&line);
            output.push('\n');
        }
        Ok(output)
    }

    /// Write the buffer as JSON Lines (NDJSON) to any writer.
    ///
    /// Streams one compact JSON object per line directly to the writer without
    /// buffering the full output in memory, making it suitable for large buffers
    /// or network sinks.
    ///
    /// # Errors
    ///
    /// Returns `io::Error` if serialization or writing fails.
    pub fn dump_to_writer_lines(&self, writer: &mut dyn std::io::Write) -> std::io::Result<()> {
        let events = self.snapshot();
        for event in &events {
            serde_json::to_writer(&mut *writer, event).map_err(std::io::Error::other)?;
            writer.write_all(b"\n")?;
        }
        Ok(())
    }

    /// Write the buffer to a file as **compact** JSON.
    ///
    /// Creates parent directories if they don't exist. For indented output
    /// use [`dump_to_file_pretty`](Self::dump_to_file_pretty).
    ///
    /// # Errors
    ///
    /// Returns `io::Error` if JSON serialization, directory creation, or file writing fails.
    pub fn dump_to_file(&self, path: &Path) -> std::io::Result<()> {
        let json = self.dump_to_json().map_err(std::io::Error::other)?;
        self.write_and_report(path, &json, None, DumpSource::Manual)
    }

    /// Write the buffer to a file as pretty-printed (indented) JSON.
    ///
    /// Creates parent directories if they don't exist.
    ///
    /// # Errors
    ///
    /// Returns `io::Error` if JSON serialization, directory creation, or file writing fails.
    pub fn dump_to_file_pretty(&self, path: &Path) -> std::io::Result<()> {
        let json = self.dump_to_json_pretty().map_err(std::io::Error::other)?;
        self.write_and_report(path, &json, None, DumpSource::Manual)
    }

    /// Build a [`FlightRecorderDump`] envelope around the current buffer.
    ///
    /// The envelope carries diagnostic metadata (capture timestamp, event
    /// count, crate version, optional trigger reason) alongside the events.
    /// Pass `reason` to record why the dump was taken (e.g. `"error_level"`);
    /// pass `None` for a manual snapshot.
    #[must_use]
    pub fn dump_envelope(&self, reason: Option<&str>) -> FlightRecorderDump {
        let events = self.snapshot();
        let event_count = events.len();
        FlightRecorderDump {
            schema_version: DUMP_SCHEMA_VERSION,
            captured_at: chrono::Utc::now(),
            crate_version: Cow::Borrowed(env!("CARGO_PKG_VERSION")),
            event_count,
            trigger_reason: reason.map(|r| Cow::Owned(r.to_string())),
            events,
        }
    }

    /// Serialize the buffer as a [`FlightRecorderDump`] envelope to a **compact** JSON string.
    ///
    /// # Errors
    ///
    /// Returns `serde_json::Error` if serialization fails.
    pub fn dump_envelope_to_json(&self, reason: Option<&str>) -> serde_json::Result<String> {
        serde_json::to_string(&self.dump_envelope(reason))
    }

    /// Serialize the buffer as a [`FlightRecorderDump`] envelope to a pretty-printed JSON string.
    ///
    /// # Errors
    ///
    /// Returns `serde_json::Error` if serialization fails.
    pub fn dump_envelope_to_json_pretty(&self, reason: Option<&str>) -> serde_json::Result<String> {
        serde_json::to_string_pretty(&self.dump_envelope(reason))
    }

    /// Write the buffer as a [`FlightRecorderDump`] envelope (**compact** JSON) to any writer.
    ///
    /// Streams directly to the writer without buffering the full JSON string.
    /// For indented output use [`dump_envelope_to_writer_pretty`](Self::dump_envelope_to_writer_pretty).
    ///
    /// # Errors
    ///
    /// Returns `io::Error` if serialization or writing fails.
    pub fn dump_envelope_to_writer(
        &self,
        writer: &mut dyn std::io::Write,
        reason: Option<&str>,
    ) -> std::io::Result<()> {
        let envelope = self.dump_envelope(reason);
        serde_json::to_writer(writer, &envelope).map_err(std::io::Error::other)
    }

    /// Write the buffer as a [`FlightRecorderDump`] envelope (pretty-printed JSON) to any writer.
    ///
    /// # Errors
    ///
    /// Returns `io::Error` if serialization or writing fails.
    pub fn dump_envelope_to_writer_pretty(
        &self,
        writer: &mut dyn std::io::Write,
        reason: Option<&str>,
    ) -> std::io::Result<()> {
        let envelope = self.dump_envelope(reason);
        serde_json::to_writer_pretty(writer, &envelope).map_err(std::io::Error::other)
    }

    /// Write the buffer as a [`FlightRecorderDump`] envelope (**compact** JSON) to a file.
    ///
    /// Creates parent directories if they don't exist. For indented output use
    /// [`dump_envelope_to_file_pretty`](Self::dump_envelope_to_file_pretty).
    ///
    /// # Errors
    ///
    /// Returns `io::Error` if serialization, directory creation, or writing fails.
    pub fn dump_envelope_to_file(&self, path: &Path, reason: Option<&str>) -> std::io::Result<()> {
        let json = self
            .dump_envelope_to_json(reason)
            .map_err(std::io::Error::other)?;
        self.write_and_report(path, &json, reason, DumpSource::Manual)
    }

    /// Write the buffer as a [`FlightRecorderDump`] envelope (pretty JSON) to a file.
    ///
    /// Creates parent directories if they don't exist.
    ///
    /// # Errors
    ///
    /// Returns `io::Error` if serialization, directory creation, or writing fails.
    pub fn dump_envelope_to_file_pretty(
        &self,
        path: &Path,
        reason: Option<&str>,
    ) -> std::io::Result<()> {
        let json = self
            .dump_envelope_to_json_pretty(reason)
            .map_err(std::io::Error::other)?;
        self.write_and_report(path, &json, reason, DumpSource::Manual)
    }

    /// Write the buffer to a file as **gzip-compressed** compact JSON.
    ///
    /// Requires the `gzip` feature. The compressed output is typically 5-10×
    /// smaller than the equivalent pretty JSON, which matters when snapshots
    /// are shipped over the network or archived. Fires the
    /// [`on_dump`](Self::with_on_dump) callback with the *compressed* byte
    /// count.
    ///
    /// # Errors
    ///
    /// Returns `io::Error` if serialization, compression, directory creation,
    /// or file writing fails.
    #[cfg(feature = "gzip")]
    pub fn dump_to_file_gz(&self, path: &Path) -> std::io::Result<()> {
        let json = self.dump_to_json().map_err(std::io::Error::other)?;
        self.write_gz_and_report(path, &json, None, DumpSource::Manual)
    }

    /// Write the buffer as a gzip-compressed [`FlightRecorderDump`] envelope.
    ///
    /// Requires the `gzip` feature. Like
    /// [`dump_to_file_gz`](Self::dump_to_file_gz) but wraps the events in the
    /// self-describing envelope. Fires the
    /// [`on_dump`](Self::with_on_dump) callback with the *compressed* byte count.
    ///
    /// # Errors
    ///
    /// Returns `io::Error` if serialization, compression, directory creation,
    /// or file writing fails.
    #[cfg(feature = "gzip")]
    pub fn dump_envelope_to_file_gz(
        &self,
        path: &Path,
        reason: Option<&str>,
    ) -> std::io::Result<()> {
        let json = self
            .dump_envelope_to_json(reason)
            .map_err(std::io::Error::other)?;
        self.write_gz_and_report(path, &json, reason, DumpSource::Manual)
    }

    /// Gzip-compress `json`, write it to `path`, and report the *compressed*
    /// byte count to the [`on_dump`](Self::with_on_dump) callback.
    #[cfg(feature = "gzip")]
    fn write_gz_and_report(
        &self,
        path: &Path,
        json: &str,
        reason: Option<&str>,
        source: DumpSource,
    ) -> std::io::Result<()> {
        let start = Instant::now();
        let compressed = gzip_encode(json)?;
        write_bytes_file(path, &compressed)?;
        let duration = start.elapsed();
        let event = DumpEvent {
            path: Some(path.to_path_buf()),
            bytes_written: compressed.len(),
            duration,
            trigger_reason: reason.map(str::to_string),
            source,
            success: true,
            error: None,
        };
        self.report(&event);
        Ok(())
    }

    /// Write `json` to `path`, then deliver a [`DumpEvent`] to the
    /// [`on_dump`](Self::with_on_dump) callback (if any).
    ///
    /// Centralizes hook firing so every file-writing dump reports exactly once
    /// with accurate byte count and duration.
    fn write_and_report(
        &self,
        path: &Path,
        json: &str,
        reason: Option<&str>,
        source: DumpSource,
    ) -> std::io::Result<()> {
        let start = Instant::now();
        write_json_file(path, json)?;
        let duration = start.elapsed();
        let event = DumpEvent {
            path: Some(path.to_path_buf()),
            bytes_written: json.len(),
            duration,
            trigger_reason: reason.map(str::to_string),
            source,
            success: true,
            error: None,
        };
        self.report(&event);
        Ok(())
    }

    /// Deliver `event` to the registered callback, swallowing any panic so a
    /// misbehaving observer can never destabilize the recorder or its trigger path.
    fn report(&self, event: &DumpEvent) {
        if let Some(hook) = &self.on_dump {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| hook(event)));
        }
    }

    /// Number of events currently in the buffer.
    #[must_use]
    pub fn len(&self) -> usize {
        self.buffer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    /// Whether the buffer is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buffer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_empty()
    }

    /// Clear all events from the buffer.
    pub fn clear(&self) {
        self.buffer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }

    /// The maximum number of events the buffer will retain.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// Write a timestamped snapshot to a diagnostics directory with retention.
    ///
    /// Creates `dir` if it doesn't exist. The filename is
    /// `{prefix}-{YYYYmmddT-HHMMSS}.json`. If a file with that name already
    /// exists (e.g. two dumps in the same second), a counter is appended:
    /// `{prefix}-{YYYYmmddT-HHMMSS}-1.json`, `-2.json`, etc. After writing,
    /// deletes the oldest snapshots matching the prefix beyond `max_files`.
    ///
    /// A `max_files` of 0 means "unlimited" — no retention cleanup is performed.
    /// This matches the convention used by the Go sibling project and avoids the
    /// footgun of writing a snapshot and then immediately deleting it.
    ///
    /// # Errors
    ///
    /// Returns `io::Error` if serialization, directory creation, or writing fails.
    /// Retention cleanup errors are logged and ignored (best-effort).
    pub fn dump_with_retention(
        &self,
        dir: &Path,
        prefix: &str,
        max_files: usize,
    ) -> std::io::Result<PathBuf> {
        let json = self.dump_to_json().map_err(std::io::Error::other)?;
        self.retention_write(dir, prefix, max_files, &json, None, DumpSource::Manual)
    }

    /// Like [`dump_with_retention`](Self::dump_with_retention) but writes a
    /// [`FlightRecorderDump`] envelope (events + metadata) instead of a bare
    /// event array. The `reason` is recorded as the dump's `trigger_reason`.
    ///
    /// # Errors
    ///
    /// Returns `io::Error` if serialization, directory creation, or writing fails.
    /// Retention cleanup errors are logged and ignored (best-effort).
    pub fn dump_with_retention_envelope(
        &self,
        dir: &Path,
        prefix: &str,
        max_files: usize,
        reason: Option<&str>,
    ) -> std::io::Result<PathBuf> {
        let json = self
            .dump_envelope_to_json(reason)
            .map_err(std::io::Error::other)?;
        self.retention_write(dir, prefix, max_files, &json, reason, DumpSource::Manual)
    }

    /// Shared timestamped-write + retention-prune + hook-report core for
    /// [`dump_with_retention`](Self::dump_with_retention) and
    /// [`dump_with_retention_envelope`](Self::dump_with_retention_envelope),
    /// also used by the trigger path with [`DumpSource::Trigger`].
    fn retention_write(
        &self,
        dir: &Path,
        prefix: &str,
        max_files: usize,
        json: &str,
        reason: Option<&str>,
        source: DumpSource,
    ) -> std::io::Result<PathBuf> {
        let path = prepare_retention_path(dir, prefix)?;
        self.write_and_report(&path, json, reason, source)?;
        cleanup_old_snapshots(dir, prefix, max_files);
        Ok(path)
    }
}

/// Upper bound on same-second collision counter suffixes.
const COLLISION_LIMIT: u32 = 9999;

/// Write a JSON string to `path`, creating parent directories first.
fn write_json_file(path: &Path, json: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(path, json)
}

/// Write a byte buffer to `path`, creating parent directories first.
#[cfg(feature = "gzip")]
fn write_bytes_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(path, bytes)
}

/// Gzip-compress `json` into an in-memory buffer. Requires the `gzip` feature.
#[cfg(feature = "gzip")]
fn gzip_encode(json: &str) -> std::io::Result<Vec<u8>> {
    use std::io::Write as _;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(json.as_bytes())?;
    encoder.finish()
}

/// Create `dir` and resolve a non-colliding timestamped path
/// `{prefix}-{YYYYmmddT-HHMMSS}.json` inside it.
fn prepare_retention_path(dir: &Path, prefix: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let ts = chrono::Utc::now().format("%Y%m%dT%H%M%S");
    let base = format!("{prefix}-{ts}");
    resolve_collision_path(dir, &base, COLLISION_LIMIT)
}

/// Resolve a unique file path for a timestamped snapshot.
///
/// Tries `{base}.json`, then `{base}-1.json`, `{base}-2.json`, ... up to `limit`
/// before returning an error to prevent unbounded looping when an absurd number
/// of same-second files already exist.
fn resolve_collision_path(dir: &Path, base: &str, limit: u32) -> std::io::Result<PathBuf> {
    let primary = dir.join(format!("{base}.json"));
    if !primary.exists() {
        return Ok(primary);
    }
    for counter in 1..=limit {
        let candidate = dir.join(format!("{base}-{counter}.json"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        format!("too many same-second snapshot files ({limit}+)"),
    ))
}

/// Delete oldest snapshot files in `dir` matching `prefix*.json` beyond `max_files`.
///
/// A `max_files` of 0 means "unlimited" — no cleanup is performed. This avoids the
/// footgun where writing a snapshot and then cleaning up with `max_files=0` would
/// immediately delete the just-written file (silent data loss).
fn cleanup_old_snapshots(dir: &Path, prefix: &str, max_files: usize) {
    if max_files == 0 {
        return;
    }

    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    let mut snapshots: Vec<_> = entries
        .filter_map(std::result::Result::ok)
        .filter(|e| {
            e.file_name().to_str().is_some_and(|name| {
                name.starts_with(prefix)
                    && std::path::Path::new(name)
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
            })
        })
        .collect();

    if snapshots.len() <= max_files {
        return;
    }

    snapshots.sort_by_key(|e| {
        e.metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
    });

    let excess = snapshots.len().saturating_sub(max_files);
    for entry in snapshots.iter().take(excess) {
        let _ = std::fs::remove_file(entry.path());
    }
}

impl std::fmt::Debug for FlightRecorder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let len = self
            .buffer
            .lock()
            .map_or_else(|_| "<poisoned>".to_string(), |b| b.len().to_string());
        f.debug_struct("FlightRecorder")
            .field("capacity", &self.capacity)
            .field("len", &len)
            .field("on_dump", &self.on_dump.is_some())
            .finish()
    }
}

/// A `tracing_subscriber::Layer` that feeds every event into a [`FlightRecorder`].
///
/// **Per-layer filtering is required for this to deliver value.** Apply
/// `EnvFilter` to the `fmt` (console) layer only, and give this layer its own
/// broader filter (e.g. `EnvFilter::new("my_app=debug,warn")`). If a global
/// `EnvFilter` is used instead, DEBUG/TRACE events are dropped before reaching
/// this layer's `on_event`, defeating the entire purpose.
pub struct FlightRecorderLayer {
    recorder: FlightRecorder,
    capture_span_context: bool,
    dump_config: Option<DumpConfig>,
}

/// Configuration for automatic snapshot-on-trigger dumps.
///
/// Built by [`FlightRecorderLayer::with_dump_on`]. When the trigger fires, the
/// layer writes a [`FlightRecorderDump`] envelope to `dir` with `prefix` and
/// keeps at most `max_files` snapshots (0 = unlimited).
#[derive(Debug)]
struct DumpConfig {
    trigger: Box<dyn Trigger>,
    dir: PathBuf,
    prefix: String,
    max_files: usize,
}

impl FlightRecorderLayer {
    /// Create a new layer that feeds events into the given recorder.
    ///
    /// Span context capture is **enabled by default**: events fired inside
    /// spans record their full span hierarchy. To disable it for maximum
    /// throughput (when request context is not needed in snapshots), use
    /// [`FlightRecorderLayer::with_span_capture`].
    #[must_use]
    pub const fn new(recorder: FlightRecorder) -> Self {
        Self {
            recorder,
            capture_span_context: true,
            dump_config: None,
        }
    }

    /// Create a new layer, explicitly choosing whether to capture the span
    /// hierarchy on each event.
    ///
    /// Set `capture` to `false` to skip span field storage and per-event scope
    /// walking entirely — useful for high-throughput pipelines where request
    /// context is not needed. Such events have an empty `spans` vec.
    #[must_use]
    pub const fn with_span_capture(recorder: FlightRecorder, capture: bool) -> Self {
        Self {
            recorder,
            capture_span_context: capture,
            dump_config: None,
        }
    }

    /// Attach an automatic snapshot-on-trigger policy.
    ///
    /// On every event, `trigger` is evaluated; when it returns `true` the
    /// buffer is written (as a [`FlightRecorderDump`] envelope, with the
    /// trigger's name as `trigger_reason`) to a timestamped file in `dir`
    /// named `{prefix}-{YYYYmmddT-HHMMSS}.json`, keeping at most `max_files`
    /// snapshots (`0` = unlimited).
    ///
    /// The dump happens synchronously in the thread that emitted the triggering
    /// event. For the common case — a [`OnceTrigger`](crate::OnceTrigger)
    /// around a [`LevelTrigger::error`](crate::LevelTrigger::error) — this is a
    /// single file write, once per process lifetime.
    #[must_use]
    pub fn with_dump_on(
        mut self,
        trigger: impl Trigger + 'static,
        dir: impl Into<PathBuf>,
        prefix: impl Into<String>,
        max_files: usize,
    ) -> Self {
        self.dump_config = Some(DumpConfig {
            trigger: Box::new(trigger),
            dir: dir.into(),
            prefix: prefix.into(),
            max_files,
        });
        self
    }

    /// Get a clone of the underlying recorder handle.
    #[must_use]
    pub fn recorder(&self) -> FlightRecorder {
        self.recorder.clone()
    }

    /// Write a snapshot envelope to the configured dump directory.
    ///
    /// No-op when no dump policy is attached. On success, the
    /// [`on_dump`](FlightRecorder::with_on_dump) callback fires (via
    /// `retention_write`) with [`DumpSource::Trigger`] and `success: true`.
    /// On failure, the callback fires with `success: false` and a
    /// human-readable error so the host can alert on the missed capture.
    fn fire_dump(&self, reason: &str) {
        let Some(cfg) = &self.dump_config else {
            return;
        };
        let start = Instant::now();
        let result = (|| -> std::io::Result<()> {
            let json = self
                .recorder
                .dump_envelope_to_json(Some(reason))
                .map_err(std::io::Error::other)?;
            self.recorder.retention_write(
                &cfg.dir,
                &cfg.prefix,
                cfg.max_files,
                &json,
                Some(reason),
                DumpSource::Trigger,
            )?;
            Ok(())
        })();
        if let Err(e) = result {
            self.recorder.report(&DumpEvent {
                path: None,
                bytes_written: 0,
                duration: start.elapsed(),
                trigger_reason: Some(reason.to_string()),
                source: DumpSource::Trigger,
                success: false,
                error: Some(e.to_string()),
            });
        }
    }
}

impl std::fmt::Debug for FlightRecorderLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlightRecorderLayer")
            .field("recorder", &self.recorder)
            .field("capture_span_context", &self.capture_span_context)
            .field("dump_config", &self.dump_config)
            .finish()
    }
}

impl<S> Layer<S> for FlightRecorderLayer
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &tracing::span::Id,
        ctx: Context<'_, S>,
    ) {
        if !self.capture_span_context {
            return;
        }
        let mut visitor = FieldVisitor::default();
        attrs.record(&mut visitor);
        let fields = visitor.into_fields();
        if let Some(span) = ctx.span(id) {
            span.extensions_mut()
                .insert(CapturedSpanFields(Arc::new(fields)));
        }
    }

    fn on_record(
        &self,
        id: &tracing::span::Id,
        values: &tracing::span::Record<'_>,
        ctx: Context<'_, S>,
    ) {
        if !self.capture_span_context {
            return;
        }
        let mut visitor = FieldVisitor::default();
        values.record(&mut visitor);
        let new_fields = visitor.into_fields();
        if let Some(span) = ctx.span(id) {
            let mut extensions = span.extensions_mut();
            match extensions.get_mut::<CapturedSpanFields>() {
                Some(existing) => Arc::make_mut(&mut existing.0).extend(new_fields),
                None => extensions.insert(CapturedSpanFields(Arc::new(new_fields))),
            }
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let mut captured = CapturedEvent::from_event(event);
        if self.capture_span_context {
            captured.spans = capture_span_context(event, &ctx);
        }
        // Decide whether to dump *before* moving `captured` into the buffer,
        // so the triggering event is included in the snapshot we write.
        let reason = self.dump_config.as_ref().and_then(|cfg| {
            cfg.trigger
                .should_dump(&captured)
                .then(|| cfg.trigger.name().to_string())
        });
        self.recorder.push(captured);
        if let Some(reason) = reason {
            self.fire_dump(&reason);
        }
    }
}

/// Internal wrapper for storing captured span fields as an extension on span data.
///
/// Stored via `LookupSpan`'s extension mechanism when a span is created or
/// updated, then read back in `on_event` to populate [`SpanContext`].
struct CapturedSpanFields(Arc<Vec<(String, String)>>);

/// Walk the span hierarchy around `event` and collect each span's name + fields.
///
/// Returns spans in root-first order (outermost span first, innermost last).
/// Sensitive span fields are already redacted because they were captured via
/// [`FieldVisitor`] in `on_new_span` / `on_record`.
fn capture_span_context<S>(event: &Event<'_>, ctx: &Context<'_, S>) -> Vec<SpanContext>
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    let Some(scope) = ctx.event_scope(event) else {
        return Vec::new();
    };
    scope
        .from_root()
        .map(|span_ref| SpanContext {
            name: span_ref.name().to_string(),
            fields: span_ref
                .extensions()
                .get::<CapturedSpanFields>()
                .map_or_else(|| Arc::new(Vec::new()), |f| Arc::clone(&f.0)),
        })
        .collect()
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic_in_result_fn,
    clippy::panic
)]
#[path = "layer_tests.rs"]
mod tests;
