# tracing-flight-recorder

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
- **Secret redaction** — fields named `token`, `password`, `secret`, `api_key`, etc. are automatically redacted to `[REDACTED]`
- **JSON snapshots** — `dump_to_json()`, `dump_to_file()`, `dump_with_retention()`
- **Optional OpenAPI** — `utoipa::ToSchema` derive behind the `openapi` feature flag
- **Zero non-tracing dependencies** — pure `tracing` ecosystem crate

## Quick Start

```toml
[dependencies]
tracing-flight-recorder = "0.1"
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

## OpenAPI Support

Enable the `openapi` feature to derive `utoipa::ToSchema` on `CapturedEvent`:

```toml
[dependencies]
tracing-flight-recorder = { version = "0.1", features = ["openapi"] }
```

## How It Works

The `FlightRecorderLayer` is a `tracing_subscriber::Layer` that receives every
event that passes its per-layer filter. Each event is captured into a
`CapturedEvent` struct (timestamp, level, target, message, fields) and pushed
into a bounded `VecDeque` ring buffer. When the buffer is full, the oldest event
is evicted.

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
