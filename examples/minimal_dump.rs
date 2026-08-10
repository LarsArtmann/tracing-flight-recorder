//! Minimal example: record tracing events and dump them to a JSON file on "failure".

use tracing_flight_recorder::{FlightRecorder, FlightRecorderLayer};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let recorder = FlightRecorder::new(500);

    // Each layer gets its own filter so the recorder captures DEBUG+
    // while the console stays at INFO.
    let fr_filter = tracing_subscriber::EnvFilter::new("debug");
    let console_filter = tracing_subscriber::EnvFilter::new("info");

    tracing_subscriber::registry()
        .with(FlightRecorderLayer::new(recorder.clone()).with_filter(fr_filter))
        .with(tracing_subscriber::fmt::layer().with_filter(console_filter))
        .init();

    // Simulate application work.
    tracing::debug!(item_id = 42, "processing item");
    tracing::info!("service started");
    tracing::debug!(item_id = 43, "processing item");
    tracing::warn!(latency_ms = 850, "slow request detected");

    // Simulate a failure: dump the flight recorder to the OS temp directory.
    let path = std::env::temp_dir().join("minimal-dump.json");
    recorder.dump_to_file(&path)?;
    println!("Dumped flight recorder to {}", path.display());
    println!("Inspect with: jq . {}", path.display());

    Ok(())
}
