//! Capture types: converting a `tracing::Event` into a serializable struct.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tracing::field::{Field, Visit};
use tracing::{Event, Level};

/// A single tracing event captured into a serializable form.
///
/// This is the unit of data stored in the [`FlightRecorder`](crate::FlightRecorder) ring buffer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CapturedEvent {
    /// When the event was observed (UTC).
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Severity level (ERROR, WARN, INFO, DEBUG, TRACE).
    ///
    /// Stored as `Cow<'static, str>` so the five known `tracing::Level` variants
    /// require zero heap allocation — only deserialized events allocate an owned
    /// `String`.
    pub level: Cow<'static, str>,
    /// Module path / target of the event.
    pub target: String,
    /// Human-readable message (the `message` field if present, otherwise empty).
    pub message: String,
    /// All structured key-value fields on the event.
    pub fields: Vec<(String, String)>,
    /// The span hierarchy the event occurred inside (root-first, innermost-last).
    /// Empty when the event was not inside any span.
    pub spans: Vec<SpanContext>,
}

/// The context of a single span in the hierarchy when an event fires.
///
/// Each entry captures the span's name and its key-value fields as they were
/// at the time the event was emitted. The collection is ordered root-first
/// (outermost span first, innermost span last).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SpanContext {
    /// The span name (the first argument to `info_span!`, `debug_span!`, etc.).
    pub name: String,
    /// Key-value fields recorded on the span (sensitive fields redacted).
    ///
    /// Wrapped in `Arc` so that all events fired inside the same span share one
    /// allocation — cloning a span context into an event is an O(1) reference
    /// bump instead of an O(fields) deep copy. Serializes as a plain JSON array
    /// via serde's `rc` feature.
    pub fields: Arc<Vec<(String, String)>>,
}

/// Current [`FlightRecorderDump`] envelope schema version.
///
/// Bumped only on a breaking structural change to the envelope so consumers can
/// branch on a stable integer instead of guessing from field presence.
pub const DUMP_SCHEMA_VERSION: u32 = 1;

/// A complete flight-recorder dump: diagnostic metadata + the captured events.
///
/// Wraps the raw event array with context about *when* and *why* the snapshot
/// was taken, plus the crate version that produced it. This is the recommended
/// output for incident snapshots — it is self-describing and survives being
/// detached from the running process.
///
/// Produced by [`FlightRecorder::dump_envelope`](crate::FlightRecorder::dump_envelope)
/// and its file/retention convenience variants. Existing array-only dump methods
/// (`dump_to_json`, `dump_to_file`, …) remain unchanged for backward
/// compatibility.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct FlightRecorderDump {
    /// Envelope schema version (currently [`DUMP_SCHEMA_VERSION`]).
    pub schema_version: u32,
    /// When the dump was captured (UTC).
    pub captured_at: chrono::DateTime<chrono::Utc>,
    /// Version of `tracing-flight-recorder` that produced this dump.
    pub crate_version: Cow<'static, str>,
    /// Number of events in the `events` field (convenience duplicate of
    /// `events.len()`, present so it is visible without expanding the array).
    pub event_count: usize,
    /// Human-readable reason the dump was triggered (`None` for manual dumps).
    pub trigger_reason: Option<Cow<'static, str>>,
    /// The captured events, oldest-first.
    pub events: Vec<CapturedEvent>,
}

/// What kind of code path triggered a dump.
///
/// Delivered as part of [`DumpEvent`] so a callback can distinguish an
/// automatic snapshot (fired by a [`Trigger`](crate::Trigger)) from one
/// requested by application code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DumpSource {
    /// A dump requested by application code (e.g. [`dump_to_file`](crate::FlightRecorder::dump_to_file)).
    Manual,
    /// A dump fired automatically because an attached
    /// [`Trigger`](crate::Trigger) matched the event.
    Trigger,
}

/// Payload delivered to an [`on_dump`](crate::FlightRecorder::with_on_dump)
/// callback after a snapshot is attempted.
///
/// Lets the host wire the flight recorder into broader observability — emit a
/// metric, ship the file to object storage, enqueue an audit entry — without
/// polling. Delivered for dumps that write to a destination (file writes
/// and retention dumps, including automatic trigger dumps); in-memory
/// serialization methods (`dump_to_json`, `dump_to_writer`, …) do not fire the
/// callback.
///
/// The `success` field distinguishes completed dumps from failed ones: when a
/// trigger dump cannot be written (disk full, permission denied), the callback
/// still fires with `success: false` and a human-readable `error` so the host
/// can alert on the missed capture.
#[derive(Debug, Clone)]
pub struct DumpEvent {
    /// Where the snapshot was written, when the dump had a destination file.
    /// `None` when the dump failed before a path could be resolved.
    pub path: Option<PathBuf>,
    /// Number of bytes serialized and written. `0` on failure.
    pub bytes_written: usize,
    /// Wall-clock time spent attempting the dump.
    pub duration: Duration,
    /// Why the dump was taken. For a [`Trigger`](DumpSource::Trigger) dump this
    /// is the trigger's [`name`](crate::Trigger::name); for a
    /// [`Manual`](DumpSource::Manual) dump it is whatever reason the caller
    /// supplied (or `None`).
    pub trigger_reason: Option<String>,
    /// Whether the dump was automatic (trigger) or caller-requested.
    pub source: DumpSource,
    /// Whether the dump completed successfully (`true`) or failed (`false`).
    ///
    /// Trigger dumps that fail (disk full, permission denied) fire the callback
    /// with `success: false` so the host can alert on the missed capture —
    /// a diagnostic tool that silently loses data is worse than no tool at all.
    pub success: bool,
    /// Human-readable error message when `success` is `false`.
    pub error: Option<String>,
}

impl CapturedEvent {
    /// Capture a `tracing::Event` into a `CapturedEvent`.
    ///
    /// Uses an internal field visitor to extract all fields (including the
    /// conventional `"message"` field) from the event.
    #[must_use]
    pub fn from_event(event: &Event<'_>) -> Self {
        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);

        let message = visitor.take_message().unwrap_or_default();

        Self {
            timestamp: Utc::now(),
            level: Cow::Borrowed(level_to_string(*event.metadata().level())),
            target: event.metadata().target().to_string(),
            message,
            fields: visitor.into_fields(),
            spans: Vec::new(),
        }
    }
}

/// `tracing::field::Visit` implementation that collects all fields on an event.
///
/// The `"message"` field is special-cased and extracted separately via
/// [`take_message`](FieldVisitor::take_message).
#[derive(Default)]
pub struct FieldVisitor {
    message: Option<String>,
    fields: Vec<(String, String)>,
}

impl FieldVisitor {
    /// Take the `"message"` field value, if present.
    pub const fn take_message(&mut self) -> Option<String> {
        self.message.take()
    }

    /// Consume the visitor and return all non-message fields.
    #[must_use]
    pub fn into_fields(self) -> Vec<(String, String)> {
        self.fields
    }

    fn record_common(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
            return;
        }
        let stored_value = if is_sensitive_field(field.name()) {
            REDACTED.to_string()
        } else {
            value.to_string()
        };
        self.fields.push((field.name().to_string(), stored_value));
    }
}

/// Check if a field name likely contains a secret value that should be redacted.
///
/// Matches common secret-bearing field names (`token`, `password`, `secret`,
/// `api_key`/`apikey`, `credential`, `passphrase`, `private_key`,
/// `authorization`, `auth`, `bearer`, `cookie`, `session_id`, `access_code`).
/// Over-redaction is intentional — a false positive (redacting a harmless field)
/// is far less costly than a false negative (leaking a secret into the ring buffer).
///
/// Uses zero-allocation ASCII case-insensitive substring matching so no `String`
/// is allocated on the hot path.
fn is_sensitive_field(name: &str) -> bool {
    SENSITIVE_PATTERNS
        .iter()
        .any(|&pattern| contains_ascii_case_insensitive(name, pattern))
}

/// Placeholder value stored in place of a sensitive field's actual value.
const REDACTED: &str = "[REDACTED]";

/// Field-name substrings that trigger secret redaction.
const SENSITIVE_PATTERNS: &[&str] = &[
    "token",
    "password",
    "secret",
    "api_key",
    "apikey",
    "credential",
    "passphrase",
    "private_key",
    "authorization",
    "auth",
    "bearer",
    "cookie",
    "session_id",
    "access_code",
];

/// Check if `haystack` contains `needle` using ASCII case-insensitive comparison.
///
/// All sensitive-field patterns are ASCII, so byte-level `eq_ignore_ascii_case` is
/// sufficient and avoids the heap allocation that `str::to_lowercase` would incur.
/// Unicode multi-byte chars in the haystack simply never match an ASCII needle byte.
fn contains_ascii_case_insensitive(haystack: &str, needle: &str) -> bool {
    let needle_bytes = needle.as_bytes();
    if needle_bytes.is_empty() {
        return true;
    }
    haystack
        .as_bytes()
        .windows(needle_bytes.len())
        .any(|window| {
            window
                .iter()
                .zip(needle_bytes)
                .all(|(h, n)| h.eq_ignore_ascii_case(n))
        })
}

impl Visit for FieldVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let mut buf = String::new();
        // write_fmt returns Result but for a String it never fails
        let _ = write!(buf, "{value:?}");
        self.record_common(field, &buf);
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.record_common(field, value);
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.record_common(field, &value.to_string());
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.record_common(field, &value.to_string());
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.record_common(field, &value.to_string());
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.record_common(field, &value.to_string());
    }

    fn record_i128(&mut self, field: &Field, value: i128) {
        self.record_common(field, &value.to_string());
    }

    fn record_u128(&mut self, field: &Field, value: u128) {
        self.record_common(field, &value.to_string());
    }

    fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
        self.record_common(field, &value.to_string());
    }
}

const fn level_to_string(level: Level) -> &'static str {
    match level {
        Level::ERROR => "ERROR",
        Level::WARN => "WARN",
        Level::INFO => "INFO",
        Level::DEBUG => "DEBUG",
        Level::TRACE => "TRACE",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn captured_event_serializes_to_json() {
        let event = CapturedEvent {
            timestamp: Utc::now(),
            level: "ERROR".into(),
            target: "test::module".to_string(),
            message: "something broke".to_string(),
            fields: vec![("device".to_string(), "dev-1".to_string())],
            spans: vec![],
        };
        let json = serde_json::to_string(&event).unwrap_or_default();
        assert!(json.contains("\"message\":\"something broke\""));
        assert!(json.contains("\"level\":\"ERROR\""));
    }

    #[test]
    fn captured_event_round_trips_through_json() {
        let original = CapturedEvent {
            timestamp: Utc::now(),
            level: "DEBUG".into(),
            target: "my::crate".to_string(),
            message: "detailed trace".to_string(),
            fields: vec![
                ("count".to_string(), "42".to_string()),
                ("active".to_string(), "true".to_string()),
            ],
            spans: vec![],
        };
        let json = serde_json::to_string(&original).unwrap_or_default();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap_or_default();
        assert_eq!(parsed["level"], "DEBUG");
        assert_eq!(parsed["target"], "my::crate");
        assert_eq!(parsed["message"], "detailed trace");
        assert_eq!(parsed["fields"].as_array().map_or(0, Vec::len), 2);
    }

    #[cfg(feature = "openapi")]
    #[test]
    fn captured_event_openapi_schema_contains_all_fields() {
        use utoipa::OpenApi;

        #[derive(OpenApi)]
        #[openapi(components(schemas(CapturedEvent)))]
        struct ApiDoc;

        let json = ApiDoc::openapi().to_pretty_json().unwrap();
        assert!(json.contains("CapturedEvent"), "schema name must appear");
        assert!(json.contains("timestamp"));
        assert!(json.contains("level"));
        assert!(json.contains("target"));
        assert!(json.contains("message"));
        assert!(json.contains("fields"));
        assert!(json.contains("spans"), "spans field must appear in schema");
    }

    #[cfg(feature = "openapi")]
    #[test]
    fn span_context_openapi_schema_contains_all_fields() {
        use utoipa::OpenApi;

        #[derive(OpenApi)]
        #[openapi(components(schemas(SpanContext)))]
        struct ApiDoc;

        let json = ApiDoc::openapi().to_pretty_json().unwrap();
        assert!(json.contains("SpanContext"), "schema name must appear");
        assert!(json.contains("name"), "name field must appear");
        assert!(json.contains("fields"), "fields must appear in SpanContext");
    }

    #[cfg(feature = "openapi")]
    #[test]
    fn flight_recorder_dump_openapi_schema_contains_all_fields() {
        use utoipa::OpenApi;

        #[derive(OpenApi)]
        #[openapi(components(schemas(FlightRecorderDump)))]
        struct ApiDoc;

        let json = ApiDoc::openapi().to_pretty_json().unwrap();
        assert!(
            json.contains("FlightRecorderDump"),
            "schema name must appear"
        );
        assert!(json.contains("schema_version"));
        assert!(json.contains("captured_at"));
        assert!(json.contains("crate_version"));
        assert!(json.contains("event_count"));
        assert!(json.contains("trigger_reason"));
        assert!(json.contains("events"));
    }

    proptest::prelude::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig {
            cases: 512,
            ..proptest::prelude::ProptestConfig::default()
        })]

        /// Cross-validate the zero-allocation byte-window matcher against an
        /// independent reference implementation (`to_lowercase().contains`).
        /// The two algorithms are structurally different, so agreement across
        /// random field names is strong evidence of correctness.
        #[test]
        fn redaction_matches_reference_implementation(
            name in "[a-zA-Z0-9_]{0,24}"
        ) {
            let actual = is_sensitive_field(&name);
            let lowered = name.to_lowercase();
            let expected = SENSITIVE_PATTERNS
                .iter()
                .any(|pattern| lowered.contains(pattern));
            prop_assert_eq!(
                actual,
                expected,
                "redaction mismatch for field name {:?}",
                name
            );
        }
    }
}
