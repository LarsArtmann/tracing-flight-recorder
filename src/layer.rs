//! Ring-buffer flight recorder and `tracing_subscriber::Layer` implementation.

use crate::capture::CapturedEvent;
use crate::DEFAULT_CAPACITY;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::Context;
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
    pub fn push(&self, event: CapturedEvent) {
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
        let path = dir.join(format!("{base}.json"));

        let path = if path.exists() {
            let mut counter: u32 = 1;
            let mut candidate = dir.join(format!("{base}-{counter}.json"));
            while candidate.exists() {
                if counter >= 9999 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::AlreadyExists,
                        "too many same-second snapshot files (9999+)",
                    ));
                }
                counter = counter.saturating_add(1);
                candidate = dir.join(format!("{base}-{counter}.json"));
            }
            candidate
        } else {
            path
        };

        self.dump_to_file(&path)?;

        cleanup_old_snapshots(dir, prefix, max_files);

        Ok(path)
    }
}

/// Delete oldest snapshot files in `dir` matching `prefix*.json` beyond `max_files`.
fn cleanup_old_snapshots(dir: &Path, prefix: &str, max_files: usize) {
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
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        self.recorder.push(CapturedEvent::from_event(event));
    }
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
