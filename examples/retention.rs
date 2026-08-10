//! Demonstrates `dump_with_retention`: write timestamped snapshots and
//! automatically prune old ones.

use std::path::Path;

use tracing_flight_recorder::{FlightRecorder, FlightRecorderLayer};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let recorder = FlightRecorder::new(200);

    let fr_filter = tracing_subscriber::EnvFilter::new("debug");
    tracing_subscriber::registry()
        .with(FlightRecorderLayer::new(recorder.clone()).with_filter(fr_filter))
        .init();

    let dir = Path::new("./diagnostics-example");

    // Simulate three failure cycles — each dumps a snapshot.
    for cycle in 1..=3 {
        tracing::info!("starting cycle {cycle}");
        for item in 1..=20 {
            tracing::debug!(cycle, item, "processing");
        }
        tracing::warn!(cycle, "simulated failure!");

        let path = recorder.dump_with_retention(dir, "snapshot", 5)?;
        println!("Cycle {cycle}: wrote {}", path.display());
    }

    // Count remaining snapshots.
    let count = std::fs::read_dir(dir).map_or(0, |entries| entries.filter_map(Result::ok).count());
    println!("Remaining snapshot files: {count}");

    Ok(())
}
