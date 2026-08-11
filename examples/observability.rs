//! Demonstrates the `on_dump` observability hook: receive structured metadata
//! about every persisted dump (path, bytes, duration, trigger reason, source,
//! success/error) without polling. Wire it into metrics, audit logs, or
//! object-storage shipping.

use tracing_flight_recorder::FlightRecorder;
use tracing_flight_recorder::FlightRecorderLayer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::env::temp_dir().join("flight-recorder-observability");
    let _ = std::fs::create_dir_all(&dir);

    // Register a callback — fires after every file-writing dump.
    let recorder = FlightRecorder::new(500).with_on_dump(|ev| {
        let status = if ev.success { "OK" } else { "FAILED" };
        println!(
            "[on_dump] {status}  source={:?}  bytes={}  duration={:?}  reason={}  path={}",
            ev.source,
            ev.bytes_written,
            ev.duration,
            ev.trigger_reason.as_deref().unwrap_or("none"),
            ev.path
                .as_ref()
                .map_or_else(|| "<none>".into(), |p| p.display().to_string()),
        );
        if let Some(err) = &ev.error {
            println!("           error: {err}");
        }
    });

    let fr_filter = tracing_subscriber::EnvFilter::new("debug");
    tracing_subscriber::registry()
        .with(FlightRecorderLayer::new(recorder.clone()).with_filter(fr_filter))
        .init();

    // Emit events so the buffer has content.
    for i in 0..10 {
        tracing::debug!("event {i}");
    }

    // Manual file dump — callback fires with DumpSource::Manual.
    let path1 = dir.join("manual.json");
    recorder.dump_to_file(&path1)?;

    // Envelope file dump — callback fires with the supplied reason.
    let path2 = dir.join("envelope.json");
    recorder.dump_envelope_to_file(&path2, Some("manual_inspection"))?;

    // Retention dump — callback fires with the resolved path.
    let _path3 = recorder.dump_with_retention(&dir, "snap", 10)?;

    // In-memory dumps do NOT fire the callback.
    let _ = recorder.dump_to_json()?;
    let _ = recorder.dump_to_json_pretty()?;

    println!("\nAll file dumps complete. The callback fired 3 times (Manual source).");

    Ok(())
}
