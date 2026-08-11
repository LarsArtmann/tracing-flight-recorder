//! In-memory flight recorder for `tracing` events.
//!
//! Inspired by Go 1.25's [`trace.FlightRecorder`], this crate provides a
//! `tracing_subscriber::Layer` that continuously buffers tracing events in a
//! bounded ring buffer. When something goes wrong — a sync failure, a circuit
//! breaker opening, a panic — you snapshot the buffer and get the last N
//! seconds of verbose (DEBUG/TRACE) context that would otherwise be lost.
//!
//! Events capture their full span hierarchy (names + fields), so snapshots
//! preserve request context like `request_id`, `user_id`, and `method`.
//! Sensitive fields are automatically redacted in both events and spans.
//!
//! The recorder pays zero I/O cost until a snapshot is triggered.
//!
//! # Quick Start
//!
//! ```no_run
//! use tracing_flight_recorder::{FlightRecorder, FlightRecorderLayer};
//! use tracing_subscriber::layer::SubscriberExt;
//! use tracing_subscriber::util::SubscriberInitExt;
//! use tracing_subscriber::Layer;
//!
//! let recorder = FlightRecorder::new(1000);
//! // CRITICAL: apply per-layer filtering so the recorder sees DEBUG+
//! // while the console stays at INFO. A global EnvFilter would block
//! // DEBUG/TRACE events from reaching the recorder layer entirely.
//! let fr_filter = tracing_subscriber::EnvFilter::new("my_app=debug,warn");
//! let console_filter = tracing_subscriber::EnvFilter::new("info");
//! tracing_subscriber::registry()
//!     .with(FlightRecorderLayer::new(recorder.clone()).with_filter(fr_filter))
//!     .with(tracing_subscriber::fmt::layer().with_filter(console_filter))
//!     .init();
//!
//! // ... application code ...
//!
//! // On error: dump the last 1000 events to disk
//! recorder.dump_to_file(std::path::Path::new("flight-recorder.json")).ok();
//! ```
//!
//! [`trace.FlightRecorder`]: https://go.dev/blog/flight-recorder
//!
//! # Minimal Example
//!
//! ```
//! use tracing_flight_recorder::FlightRecorder;
//!
//! let recorder = FlightRecorder::new(50);
//! assert!(recorder.is_empty());
//! assert_eq!(recorder.capacity(), 50);
//! ```

#![cfg_attr(
    test,
    allow(
        clippy::pedantic,
        clippy::nursery,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::exit,
        clippy::unreachable,
        clippy::unimplemented,
        clippy::todo,
        clippy::panic_in_result_fn,
        clippy::unchecked_time_subtraction,
        clippy::as_conversions,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::string_slice,
        clippy::useless_vec,
        clippy::unused_async,
    )
)]

mod capture;
mod layer;
mod trigger;

#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct _ReadmeDoctests;

pub use capture::{
    CapturedEvent, DumpEvent, DumpSource, FlightRecorderDump, SpanContext, DUMP_SCHEMA_VERSION,
};
pub use layer::{FlightRecorder, FlightRecorderLayer};
pub use trigger::{LevelTrigger, OnceTrigger, Trigger};

/// Default ring-buffer capacity (number of events).
///
/// At ~200-500 bytes per event, this uses ~200 KB - 500 KB of memory and
/// captures the most recent 1000 events. The time span covered depends on
/// your application's event rate — at 10-50 events/sec this is roughly
/// 20-100 seconds of context.
pub const DEFAULT_CAPACITY: usize = 1000;
