//! Capture types: converting a `tracing::Event` into a serializable struct.

use chrono::Utc;
use serde::{Deserialize, Serialize};
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
    pub level: String,
    /// Module path / target of the event.
    pub target: String,
    /// Human-readable message (the `message` field if present, otherwise empty).
    pub message: String,
    /// All structured key-value fields on the event.
    pub fields: Vec<(String, String)>,
}

impl CapturedEvent {
    /// Capture a `tracing::Event` into a `CapturedEvent`.
    ///
    /// Uses [`FieldVisitor`] to extract all fields (including the conventional
    /// `"message"` field) from the event.
    #[must_use]
    pub fn from_event(event: &Event<'_>) -> Self {
        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);

        let message = visitor.take_message().unwrap_or_default();

        Self {
            timestamp: Utc::now(),
            level: level_to_string(*event.metadata().level()),
            target: event.metadata().target().to_string(),
            message,
            fields: visitor.into_fields(),
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

    fn record_common(&mut self, field: &Field, value: String) {
        if field.name() == "message" {
            self.message = Some(value);
        } else {
            let stored_value = if is_sensitive_field(field.name()) {
                "[REDACTED]".to_string()
            } else {
                value
            };
            self.fields.push((field.name().to_string(), stored_value));
        }
    }
}

/// Check if a field name likely contains a secret value that should be redacted.
///
/// Matches common secret-bearing field names (`token`, `password`, `secret`, `api_key`,
/// `credential`, `passphrase`, `private_key`). Over-redaction is intentional — a false
/// positive (redacting a harmless field) is far less costly than a false negative
/// (leaking a secret into the ring buffer).
fn is_sensitive_field(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.contains("token")
        || lower.contains("password")
        || lower.contains("secret")
        || lower.contains("api_key")
        || lower.contains("apikey")
        || lower.contains("credential")
        || lower.contains("passphrase")
        || lower.contains("private_key")
}

impl Visit for FieldVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let mut buf = String::new();
        // write_fmt returns Result but for a String it never fails
        let _ = write!(buf, "{value:?}");
        self.record_common(field, buf);
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.record_common(field, value.to_string());
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.record_common(field, value.to_string());
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.record_common(field, value.to_string());
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.record_common(field, value.to_string());
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.record_common(field, value.to_string());
    }

    fn record_i128(&mut self, field: &Field, value: i128) {
        self.record_common(field, value.to_string());
    }

    fn record_u128(&mut self, field: &Field, value: u128) {
        self.record_common(field, value.to_string());
    }

    fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
        self.record_common(field, value.to_string());
    }
}

fn level_to_string(level: Level) -> String {
    match level {
        Level::ERROR => "ERROR",
        Level::WARN => "WARN",
        Level::INFO => "INFO",
        Level::DEBUG => "DEBUG",
        Level::TRACE => "TRACE",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captured_event_serializes_to_json() {
        let event = CapturedEvent {
            timestamp: Utc::now(),
            level: "ERROR".to_string(),
            target: "test::module".to_string(),
            message: "something broke".to_string(),
            fields: vec![("device".to_string(), "dev-1".to_string())],
        };
        let json = serde_json::to_string(&event).unwrap_or_default();
        assert!(json.contains("\"message\":\"something broke\""));
        assert!(json.contains("\"level\":\"ERROR\""));
    }

    #[test]
    fn captured_event_round_trips_through_json() {
        let original = CapturedEvent {
            timestamp: Utc::now(),
            level: "DEBUG".to_string(),
            target: "my::crate".to_string(),
            message: "detailed trace".to_string(),
            fields: vec![
                ("count".to_string(), "42".to_string()),
                ("active".to_string(), "true".to_string()),
            ],
        };
        let json = serde_json::to_string(&original).unwrap_or_default();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap_or_default();
        assert_eq!(parsed["level"], "DEBUG");
        assert_eq!(parsed["target"], "my::crate");
        assert_eq!(parsed["message"], "detailed trace");
        assert_eq!(parsed["fields"].as_array().map_or(0, Vec::len), 2);
    }
}
