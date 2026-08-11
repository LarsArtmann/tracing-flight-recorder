//! Demonstrates gzip-compressed dumps: snapshots 5-10× smaller, ideal for
//! archiving or shipping over the network. Requires the `gzip` feature
//! (`cargo run --all-features --example compression`).

use std::path::PathBuf;

use tracing_flight_recorder::{FlightRecorder, FlightRecorderLayer};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let recorder = FlightRecorder::new(500);

    let fr_filter = tracing_subscriber::EnvFilter::new("debug");
    tracing_subscriber::registry()
        .with(FlightRecorderLayer::new(recorder.clone()).with_filter(fr_filter))
        .init();

    // Emit events that the layer will capture into the ring buffer.
    for i in 0..50 {
        tracing::debug!(index = i, "event {i}");
    }

    let dir: PathBuf = std::env::temp_dir().join("flight-recorder-compression");
    let _ = std::fs::create_dir_all(&dir);

    // Write a raw-array gzip dump.
    let raw_gz = dir.join("events.json.gz");
    recorder.dump_to_file_gz(&raw_gz)?;

    // Write an envelope gzip dump (self-describing: schema version, timestamp, etc.).
    let envelope_gz = dir.join("incident.json.gz");
    recorder.dump_envelope_to_file_gz(&envelope_gz, Some("demo"))?;

    // Compare sizes.
    let raw_json = recorder.dump_to_json()?;
    let raw_gz_bytes = std::fs::metadata(&raw_gz)?.len();
    let envelope_gz_bytes = std::fs::metadata(&envelope_gz)?.len();

    println!("Raw compact JSON:   {} bytes", raw_json.len());
    println!("Raw gzip:           {raw_gz_bytes} bytes");
    println!("Envelope gzip:      {envelope_gz_bytes} bytes");
    println!("\nFiles written to {}", dir.display());

    Ok(())
}
