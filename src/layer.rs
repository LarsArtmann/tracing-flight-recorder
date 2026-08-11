//! Ring-buffer flight recorder and `tracing_subscriber::Layer` implementation.

use crate::capture::{CapturedEvent, FieldVisitor, SpanContext};
use crate::DEFAULT_CAPACITY;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;

/// A bounded in-memory ring buffer of tracing events.
///
/// Clone this handle freely — all clones share the same underlying buffer.
/// See [`FlightRecorderLayer`] for how to connect this to a `tracing` subscriber.
#[derive(Clone)]
pub struct FlightRecorder {
    buffer: Arc<Mutex<VecDeque<CapturedEvent>>>,
    capacity: usize,
}

impl FlightRecorder {
    /// Create a new flight recorder with the given capacity (max events retained).
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: Arc::new(Mutex::new(VecDeque::with_capacity(capacity))),
            capacity,
        }
    }

    /// Create a new flight recorder with the default capacity ([`DEFAULT_CAPACITY`]).
    #[must_use]
    pub fn with_default_capacity() -> Self {
        Self::new(DEFAULT_CAPACITY)
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

    /// Serialize the buffer to a JSON string.
    ///
    /// # Errors
    ///
    /// Returns `serde_json::Error` if serialization fails.
    pub fn dump_to_json(&self) -> serde_json::Result<String> {
        let events = self.snapshot();
        serde_json::to_string_pretty(&events)
    }

    /// Write the buffer as pretty-printed JSON to any writer.
    ///
    /// Streams directly to the writer without buffering the full JSON string,
    /// making it suitable for large buffers or network sinks.
    ///
    /// # Errors
    ///
    /// Returns `io::Error` if serialization or writing fails.
    pub fn dump_to_writer(&self, writer: &mut dyn std::io::Write) -> std::io::Result<()> {
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

    /// Write the buffer to a file as pretty-printed JSON.
    ///
    /// Creates parent directories if they don't exist.
    ///
    /// # Errors
    ///
    /// Returns `io::Error` if JSON serialization, directory creation, or file writing fails.
    pub fn dump_to_file(&self, path: &Path) -> std::io::Result<()> {
        let json = self.dump_to_json().map_err(std::io::Error::other)?;

        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }

        std::fs::write(path, json)
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
        std::fs::create_dir_all(dir)?;

        let ts = chrono::Utc::now().format("%Y%m%dT%H%M%S");
        let base = format!("{prefix}-{ts}");

        let path = resolve_collision_path(dir, &base, COLLISION_LIMIT)?;

        self.dump_to_file(&path)?;

        cleanup_old_snapshots(dir, prefix, max_files);

        Ok(path)
    }
}

/// Upper bound on same-second collision counter suffixes.
const COLLISION_LIMIT: u32 = 9999;

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
}

impl FlightRecorderLayer {
    /// Create a new layer that feeds events into the given recorder.
    #[must_use]
    pub const fn new(recorder: FlightRecorder) -> Self {
        Self { recorder }
    }

    /// Get a clone of the underlying recorder handle.
    #[must_use]
    pub fn recorder(&self) -> FlightRecorder {
        self.recorder.clone()
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
        let mut visitor = FieldVisitor::default();
        attrs.record(&mut visitor);
        let fields = visitor.into_fields();
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(CapturedSpanFields(fields));
        }
    }

    fn on_record(
        &self,
        id: &tracing::span::Id,
        values: &tracing::span::Record<'_>,
        ctx: Context<'_, S>,
    ) {
        let mut visitor = FieldVisitor::default();
        values.record(&mut visitor);
        let new_fields = visitor.into_fields();
        if let Some(span) = ctx.span(id) {
            let mut extensions = span.extensions_mut();
            match extensions.get_mut::<CapturedSpanFields>() {
                Some(existing) => existing.0.extend(new_fields),
                None => extensions.insert(CapturedSpanFields(new_fields)),
            }
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let mut captured = CapturedEvent::from_event(event);
        captured.spans = capture_span_context(event, &ctx);
        self.recorder.push(captured);
    }
}

/// Internal wrapper for storing captured span fields as an extension on span data.
///
/// Stored via `LookupSpan`'s extension mechanism when a span is created or
/// updated, then read back in `on_event` to populate [`SpanContext`].
struct CapturedSpanFields(Vec<(String, String)>);

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
                .map_or_else(Vec::new, |f| f.0.clone()),
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
