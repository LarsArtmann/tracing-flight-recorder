//! Demonstrates the trigger system: the flight recorder dumps automatically
//! when an ERROR fires — no manual `dump` call in every error path. The dump
//! is a self-describing `FlightRecorderDump` envelope (schema version, trigger
//! reason, event count, crate version, events).

use std::path::PathBuf;

use tracing_flight_recorder::{FlightRecorder, FlightRecorderLayer, LevelTrigger, OnceTrigger};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let recorder = FlightRecorder::new(500);

    // Diagnostics directory for automatic incident snapshots.
    let incident_dir: PathBuf = std::env::temp_dir().join("flight-recorder-incidents");
    let _ = std::fs::create_dir_all(&incident_dir);

    let fr_filter = tracing_subscriber::EnvFilter::new("debug");
    let console_filter = tracing_subscriber::EnvFilter::new("info");

    tracing_subscriber::registry()
        .with(
            FlightRecorderLayer::new(recorder.clone())
                // with_dump_on MUST come before with_filter: with_filter wraps
                // the layer in Filtered<L, F, S>, which doesn't expose with_dump_on.
                .with_dump_on(
                    OnceTrigger::new(LevelTrigger::error()),
                    incident_dir.clone(),
                    "incident",
                    5,
                )
                .with_filter(fr_filter),
        )
        .with(tracing_subscriber::fmt::layer().with_filter(console_filter))
        .init();

    // Normal operation — DEBUG/TRACE context accumulates in the buffer.
    tracing::debug!("connecting to database");
    tracing::debug!("connection established");
    tracing::info!("serving request req-001");

    // When this fires, the layer writes the buffer to disk automatically.
    tracing::error!(target: "db", "connection lost — writeable region is now captured");

    // Subsequent errors do NOT trigger more dumps (OnceTrigger).
    tracing::error!("retry failed");
    tracing::error!("giving up");

    // Show what was written.
    let snapshots: Vec<_> = std::fs::read_dir(&incident_dir)?
        .filter_map(std::result::Result::ok)
        .filter(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|n| n.starts_with("incident"))
        })
        .collect();

    println!(
        "Trigger wrote {} snapshot(s) to {}:",
        snapshots.len(),
        incident_dir.display()
    );

    for entry in &snapshots {
        let path = entry.path();
        let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        let contents = std::fs::read_to_string(&path)?;
        let parsed: serde_json::Value = serde_json::from_str(&contents)?;
        println!(
            "  {fname} — trigger_reason={}, event_count={}, schema=v{}",
            parsed
                .get("trigger_reason")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("?"),
            parsed
                .get("event_count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0),
            parsed
                .get("schema_version")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0),
        );
    }

    // Even with OnceTrigger, the manual API is still available:
    let manual = recorder.dump_envelope_to_json(Some("manual_inspection"))?;
    let parsed: serde_json::Value = serde_json::from_str(&manual)?;
    println!(
        "\nManual envelope: {} events, crate_version={}",
        parsed
            .get("event_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        parsed
            .get("crate_version")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("?"),
    );

    Ok(())
}
