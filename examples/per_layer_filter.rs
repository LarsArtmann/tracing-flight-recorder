//! Demonstrates per-layer filtering: the recorder captures DEBUG events
//! while the console output stays at INFO.

use tracing_flight_recorder::{FlightRecorder, FlightRecorderLayer};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;

fn main() {
    let recorder = FlightRecorder::new(1000);

    // CRITICAL: each layer gets its OWN filter.
    // The recorder sees DEBUG+, the console sees INFO+ only.
    // A global EnvFilter would block DEBUG before it reaches the recorder.
    let fr_filter = tracing_subscriber::EnvFilter::new("debug");
    let console_filter = tracing_subscriber::EnvFilter::new("info");

    tracing_subscriber::registry()
        .with(FlightRecorderLayer::new(recorder.clone()).with_filter(fr_filter))
        .with(tracing_subscriber::fmt::layer().with_filter(console_filter))
        .init();

    tracing::debug!("hidden from console, captured by recorder");
    tracing::info!("visible in both console and recorder");

    let snapshot = recorder.snapshot();
    println!("Recorder captured {} events:", snapshot.len());
    for event in &snapshot {
        println!("  [{}] {}", event.level, event.message);
    }
}
