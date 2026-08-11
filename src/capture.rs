//! Capture types: converting a `tracing::Event` into a serializable struct.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt::Write as _;
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
    pub fields: Vec<(String, String)>,
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
}
