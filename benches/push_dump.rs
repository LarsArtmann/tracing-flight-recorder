//! Hot-path benchmarks for `tracing-flight-recorder`.
//!
//! Run with `cargo bench` (add `-- --quick` for faster, less rigorous feedback).
//!
//! Covers three cost centres, all exercised through the **public API**:
//! - `on_event`     — the full capture path (subscriber → `FieldVisitor` → push),
//!                    i.e. the production hot path that runs on every event
//! - `snapshot/*`   — cloning the buffer into a `Vec`
//! - `dump_to_json` — serializing a full buffer to a JSON string
//!
//! `FlightRecorder::push` is deliberately `pub(crate)`, so buffers are seeded
//! by emitting real `tracing` events through the layer rather than calling
//! `push` directly.

// Benchmarks are non-shipped developer tooling. Relax the crate's strict
// (denied-by-default) clippy gate so idiomatic benchmark code compiles.
#![allow(
    clippy::pedantic,
    clippy::nursery,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::string_slice,
    clippy::panic,
    clippy::needless_pass_by_value,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::items_after_statements,
    clippy::doc_overindented_list_items
)]

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use tracing_flight_recorder::{FlightRecorder, FlightRecorderLayer};
use tracing_subscriber::layer::SubscriberExt;

/// Seed a recorder with `capacity` real events by driving them through the
/// public `FlightRecorderLayer` capture path.
fn seed_recorder(capacity: usize) -> FlightRecorder {
    let recorder = FlightRecorder::new(capacity);
    let subscriber =
        tracing_subscriber::registry().with(FlightRecorderLayer::new(recorder.clone()));
    let dispatch = tracing::Dispatch::new(subscriber);
    tracing::dispatcher::with_default(&dispatch, || {
        for i in 0..capacity {
            tracing::info!(count = i, target = "bench", "seed event");
        }
    });
    recorder
}

/// Full capture hot path: a real subscriber dispatches `tracing::info!`
/// events through `FlightRecorderLayer::on_event` (`FieldVisitor` + span
/// capture + push). This is the path that runs on every event in production.
fn bench_on_event(c: &mut Criterion) {
    let subscriber =
        tracing_subscriber::registry().with(FlightRecorderLayer::new(FlightRecorder::new(1_000)));
    let dispatch = tracing::Dispatch::new(subscriber);

    let mut group = c.benchmark_group("on_event");
    for &n in &[100_u64, 1_000, 10_000] {
        group.bench_function(format!("n={n}"), |b| {
            b.iter(|| {
                tracing::dispatcher::with_default(&dispatch, || {
                    for i in 0..n {
                        tracing::info!(count = i, target = "bench", "processing request");
                    }
                });
            });
        });
    }
    group.finish();
}

fn bench_snapshot(c: &mut Criterion) {
    let mut group = c.benchmark_group("snapshot");
    for &n in &[1_000_usize, 10_000] {
        let recorder = seed_recorder(n);
        group.bench_function(format!("n={n}"), |b| {
            b.iter(|| black_box(recorder.snapshot()));
        });
    }
    group.finish();
}

fn bench_dump_to_json(c: &mut Criterion) {
    let recorder = seed_recorder(1_000);
    c.bench_function("dump_to_json (n=1000)", |b| {
        b.iter(|| {
            let json = recorder.dump_to_json().expect("serialize");
            black_box(json);
        });
    });
}

criterion_group!(benches, bench_on_event, bench_snapshot, bench_dump_to_json);
criterion_main!(benches);
