# Domain Language

Ubiquitous vocabulary for the `tracing-flight-recorder` crate. Every term below
appears in source code, tests, or documentation — there are no dead terms.

---

## Core Concepts

| Term | Definition | Where used |
|------|------------|------------|
| **Flight Recorder** | An in-memory, bounded ring buffer that continuously records `tracing` events, enabling on-demand retrieval of recent verbose (DEBUG/TRACE) context when something goes wrong. Inspired by Go 1.25's `trace.FlightRecorder`. | `FlightRecorder` struct — `src/layer.rs:17` |
| **Ring Buffer** | The fixed-capacity circular buffer (`VecDeque`) inside `FlightRecorder` that stores events and evicts the oldest when full. | `FlightRecorder.buffer` field — `src/layer.rs:18` |
| **Snapshot** | A point-in-time copy (cloned `Vec<CapturedEvent>`) of all events currently in the ring buffer, returned in insertion order (oldest first). | `FlightRecorder::snapshot()` — `src/layer.rs:52` |
| **Eviction** | The removal (`pop_front`) of the oldest event from the ring buffer when it is at capacity, to make room for a new event. | `FlightRecorder::push()` — `src/layer.rs:39` |
| **Capacity** | The maximum number of events the ring buffer retains before evicting old ones. Defaults to 1000 (`DEFAULT_CAPACITY`). | `FlightRecorder.capacity` — `src/layer.rs:19`; `DEFAULT_CAPACITY` — `src/lib.rs:77` |

## Data Structures

| Term | Definition | Where used |
|------|------------|------------|
| **Captured Event** | A serializable struct representing a single `tracing::Event` — timestamp, severity level, target, message, and structured fields. The unit of data stored in the ring buffer. | `CapturedEvent` struct — `src/capture.rs:14` |
| **Field Visitor** | A `tracing::field::Visit` implementation that traverses all fields on an event, collecting key-value pairs, with special-case extraction of the `"message"` field and automatic redaction of sensitive values. | `FieldVisitor` struct — `src/capture.rs:54` |
| **Timestamp** | The UTC date-time at which the event was observed, set via `chrono::Utc::now()` at capture time. | `CapturedEvent.timestamp` — `src/capture.rs:16` |
| **Level / Severity** | The `ERROR`/`WARN`/`INFO`/`DEBUG`/`TRACE` classification of an event. | `CapturedEvent.level` — `src/capture.rs:18` |
| **Target** | The module path of a `tracing` event, derived from `event.metadata().target()`. | `CapturedEvent.target` — `src/capture.rs:20` |
| **Message Field** | The conventional `tracing` field named `"message"` holding the human-readable event text; extracted separately from structured fields. | `FieldVisitor::take_message()` — `src/capture.rs:61` |
| **Fields (Structured Fields)** | All non-message key-value pairs attached to a `tracing` event, stored as `Vec<(String, String)>`. | `CapturedEvent.fields` — `src/capture.rs:24` |

## Operations

| Term | Definition | Where used |
|------|------------|------------|
| **Dump** | Serializing the ring-buffer contents to an external format — a JSON string (`dump_to_json`) or a file (`dump_to_file`). | `FlightRecorder::dump_to_json()` — `src/layer.rs:66`; `dump_to_file()` — `src/layer.rs:78` |
| **Retention Dump** | A dump strategy that writes a timestamped snapshot file into a diagnostics directory, then deletes the oldest matching files beyond `max_files`. | `FlightRecorder::dump_with_retention()` — `src/layer.rs:134` |
| **Diagnostics Directory** | A filesystem directory where timestamped snapshot files are written and pruned according to the retention policy. | `dir` parameter — `src/layer.rs:136` |
| **Redaction** | Automatic replacement of sensitive field values with `[REDACTED]`. Field names matched: `token`, `password`, `secret`, `api_key`, `credential`, `passphrase`, `private_key`. Over-redaction is intentional. | `is_sensitive_field()` — `src/capture.rs:91` |

## Tracing Integration

| Term | Definition | Where used |
|------|------------|------------|
| **Layer** (`FlightRecorderLayer`) | A `tracing_subscriber::Layer` that receives every event passing its per-layer filter and feeds each into a `FlightRecorder`. | `FlightRecorderLayer` — `src/layer.rs:234`; `impl Layer` — `src/layer.rs:252` |
| **Per-Layer Filter** | A `tracing_subscriber` filter applied to an individual `Layer` so the recorder captures DEBUG/TRACE while the console `fmt` layer stays at INFO. A global filter would block verbose events before they reach the recorder. | Doc comment — `src/layer.rs:229`; Quick Start — `src/lib.rs:23` |

## Optional Features

| Term | Definition | Where used |
|------|------------|------------|
| **OpenAPI Schema** | Optional cargo feature (`openapi`) that derives `utoipa::ToSchema` on `CapturedEvent`, enabling automatic OpenAPI/JSON-schema generation. | `#[cfg_attr(feature = "openapi", ...)]` — `src/capture.rs:13` |
