# tracing-flight-recorder

[![crates.io](https://img.shields.io/crates/v/tracing-flight-recorder.svg)](https://crates.io/crates/tracing-flight-recorder)
[![docs.rs](https://docs.rs/tracing-flight-recorder/badge.svg)](https://docs.rs/tracing-flight-recorder)
[![CI](https://github.com/LarsArtmann/tracing-flight-recorder/actions/workflows/ci.yml/badge.svg)](https://github.com/LarsArtmann/tracing-flight-recorder/actions/workflows/ci.yml)
[![msrv 1.86](https://img.shields.io/static/v1?label=msrv&message=1.86&color=blue)](https://github.com/LarsArtmann/tracing-flight-recorder/blob/master/Cargo.toml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

In-memory ring-buffer flight recorder for [`tracing`] events.

Inspired by Go 1.25's [`trace.FlightRecorder`], this crate provides a
`tracing_subscriber::Layer` that continuously buffers tracing events in a
bounded ring buffer. When something goes wrong — a sync failure, a circuit
breaker opening, a panic — you snapshot the buffer and get the last N
seconds of verbose (DEBUG/TRACE) context that would otherwise be lost.

The recorder pays zero I/O cost until a snapshot is triggered.

## Features

- **Bounded ring buffer** — fixed capacity, evicts oldest events first
- **`tracing_subscriber::Layer`** — drops into any existing tracing setup
- **Per-layer filtering** — capture DEBUG/TRACE while console stays at INFO
- **Span context capture** — events record their full span hierarchy (names + fields, root-first)
- **Configurable span capture** — disable span storage for max throughput via `with_span_capture(recorder, false)`
- **Automatic snapshots on failure** — `Trigger` trait + `LevelTrigger`/`OnceTrigger` dump the buffer automatically when an error fires, no manual wiring
- **Secret redaction** — fields named `token`, `password`, `secret`, `authorization`, `cookie`, `session_id`, etc. are automatically redacted to `[REDACTED]`
- **JSON snapshots** — compact by default (`dump_to_json`), with `dump_to_json_pretty` for human-readable output; plus `dump_to_file`, `dump_to_writer`, `dump_with_retention`, and envelope writer variants
- **Dump metadata envelope** — `FlightRecorderDump` wraps events with schema version, timestamp, event count, crate version, and trigger reason
- **NDJSON output** — `dump_to_json_lines()` and `dump_to_writer_lines()` for streamable, line-delimited JSON ingestible by log pipelines
- **Gzip compression** — `dump_to_file_gz` / `dump_envelope_to_file_gz` behind the `gzip` feature for 5-10× smaller snapshots
- **Observability hooks** — `with_on_dump(callback)` fires after every file dump with the destination path, byte count, duration, source (manual vs trigger), and success/error status
- **Optional OpenAPI** — `utoipa::ToSchema` derive behind the `openapi` feature flag
- **Minimal dependencies** — `tracing` ecosystem + `serde`/`chrono` for serialization

## Quick Start

```toml
[dependencies]
tracing-flight-recorder = "0.3"
```

```rust,no_run
use tracing_flight_recorder::{FlightRecorder, FlightRecorderLayer};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;

let recorder = FlightRecorder::new(1000);

// CRITICAL: apply per-layer filtering so the recorder sees DEBUG+
// while the console stays at INFO. A global EnvFilter would block
// DEBUG/TRACE events from reaching the recorder layer entirely.
let fr_filter = tracing_subscriber::EnvFilter::new("my_app=debug,warn");
let console_filter = tracing_subscriber::EnvFilter::new("info");

tracing_subscriber::registry()
    .with(FlightRecorderLayer::new(recorder.clone()).with_filter(fr_filter))
    .with(tracing_subscriber::fmt::layer().with_filter(console_filter))
    .init();

// ... application code ...

// On error: dump the last 1000 events to disk
recorder.dump_to_file(std::path::Path::new("flight-recorder.json")).ok();
```

## Snapshot with Retention

```rust,no_run
use tracing_flight_recorder::FlightRecorder;

let recorder = FlightRecorder::new(1000);

// Write to a diagnostics directory, keeping at most 10 snapshot files
let path = recorder
    .dump_with_retention(
        std::path::Path::new("./diagnostics"),
        "snapshot",
        10,
    )
    .ok();
```

## Span Context Capture

Events fired inside spans automatically record their full span hierarchy, so
snapshots preserve request context: `request_id`, `user_id`, `method`, and any
other fields set on parent spans. Sensitive fields in spans are redacted just
like event fields.

```rust,no_run
use tracing_flight_recorder::{FlightRecorder, FlightRecorderLayer};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;

let recorder = FlightRecorder::new(1000);
let fr_filter = tracing_subscriber::EnvFilter::new("my_app=debug,warn");

tracing_subscriber::registry()
    .with(FlightRecorderLayer::new(recorder.clone()).with_filter(fr_filter))
    .init();

// Events inside spans capture the full hierarchy (root-first):
let request = tracing::info_span!("http_request", request_id = "req-123", method = "GET");
let _enter = request.enter();
tracing::debug!("authenticating");  // spans: [http_request]

let db = tracing::debug_span!("db_query", table = "users");
let _enter2 = db.enter();
tracing::warn!("slow query");  // spans: [http_request, db_query]

// Each captured event's `spans` field contains the full chain:
// [
//   SpanContext { name: "http_request", fields: [("request_id", "req-123"), ...] },
//   SpanContext { name: "db_query", fields: [("table", "users")] },
// ]
```

## Automatic Snapshots on Failure

The whole point of a flight recorder is to capture the buffer _when something
goes wrong_ — without wiring a `dump` call into every error path. Attach a
[`Trigger`](https://docs.rs/tracing-flight-recorder/latest/tracing_flight_recorder/trait.Trigger.html)
to the layer and it dumps automatically:

```rust,no_run
use tracing_flight_recorder::{
    FlightRecorder, FlightRecorderLayer, LevelTrigger, OnceTrigger,
};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;

let recorder = FlightRecorder::new(1000);
let fr_filter = tracing_subscriber::EnvFilter::new("my_app=debug,warn");

// Dump exactly once — the first time an ERROR fires — to ./diagnostics,
// keeping at most 10 snapshot envelopes.
tracing_subscriber::registry()
    .with(
        FlightRecorderLayer::new(recorder.clone())
            // with_dump_on before with_filter: with_filter wraps the layer
            .with_dump_on(
                OnceTrigger::new(LevelTrigger::error()),
                "./diagnostics",
                "incident",
                10,
            )
            .with_filter(fr_filter),
    )
    .init();

// When this fires, ./diagnostics/incident-<timestamp>.json is written
// automatically — a self-describing envelope with the trigger reason,
// event count, crate version, and the full buffered history.
tracing::error!("connection lost");
```

Built-in triggers:

- **`LevelTrigger`** — fires at or above a severity (`LevelTrigger::error()`, `LevelTrigger::new(Level::WARN)`)
- **`OnceTrigger`** — wraps any trigger; fires at most once until `reset()`, preventing disk-filling cascades

Implement the `Trigger` trait for custom conditions (e.g. "dump on a specific
error code field").

## Dump Metadata Envelope

For self-describing incident snapshots, use the `FlightRecorderDump` envelope
(events + metadata) instead of a bare event array:

```rust,no_run
use tracing_flight_recorder::FlightRecorder;

let recorder = FlightRecorder::new(1000);
// ... events accumulate ...

// Writes { schema_version, captured_at, crate_version, event_count,
//         trigger_reason, events: [...] }
recorder
    .dump_envelope_to_file(std::path::Path::new("incident.json"), Some("manual"))
    .ok();
```

## OpenAPI Support

Enable the `openapi` feature to derive `utoipa::ToSchema` on `CapturedEvent`, `SpanContext`, and `FlightRecorderDump`:

```toml
[dependencies]
tracing-flight-recorder = { version = "0.3", features = ["openapi"] }
```

## Compression & Observability

**Gzip compression** (behind the `gzip` feature) writes snapshots 5-10× smaller,
useful when shipping incident files over the network or archiving them. Enable
the feature, then call `dump_to_file_gz` / `dump_envelope_to_file_gz`:

```toml
[dependencies]
tracing-flight-recorder = { version = "0.3", features = ["gzip"] }
```

**Observability hooks** let the host react to every persisted dump without
polling — emit a metric, ship the file, enqueue an audit entry:

```rust,no_run
use tracing_flight_recorder::FlightRecorder;
let recorder = FlightRecorder::new(1000)
    .with_on_dump(|ev| eprintln!("dumped {} bytes to {:?}", ev.bytes_written, ev.path));
// Every file dump (manual and automatic trigger dumps) now invokes the callback.
```

## How It Works

The `FlightRecorderLayer` is a `tracing_subscriber::Layer` that receives every
event that passes its per-layer filter. Each event is captured into a
`CapturedEvent` struct (timestamp, level, target, message, fields, and the
full span hierarchy) and pushed into a bounded `VecDeque` ring buffer. When
the buffer is full, the oldest event is evicted.

When an event fires inside a span (or nested spans), the layer captures the
span stack — names and key-value fields — so that snapshots preserve the
request context: `request_id`, `user_id`, `method`, `path`, and any other
fields set on parent spans. Sensitive fields in both events and spans are
redacted automatically.

The key insight from Go 1.25's flight recorder: you always want verbose
(DEBUG/TRACE) context available, but you don't want to pay the I/O cost of
writing it all to disk. The ring buffer gives you the last N events for free —
you only pay the serialization cost when you actually need to dump them.

**Per-layer filtering is essential.** If you use a global `EnvFilter`, it
drops DEBUG/TRACE events before they reach the recorder layer. You must give
the `FlightRecorderLayer` its own broader filter (e.g., `EnvFilter::new("my_app=debug,warn")`)
and apply a narrower filter to your console `fmt` layer.

## License

Apache-2.0
