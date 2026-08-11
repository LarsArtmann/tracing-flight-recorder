//! Demonstrates span context capture: events fired inside spans record their
//! full span hierarchy (names + fields), so snapshots preserve request context.

use tracing_flight_recorder::{FlightRecorder, FlightRecorderLayer};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let recorder = FlightRecorder::new(500);

    let fr_filter = tracing_subscriber::EnvFilter::new("debug");
    let console_filter = tracing_subscriber::EnvFilter::new("info");

    tracing_subscriber::registry()
        .with(FlightRecorderLayer::new(recorder.clone()).with_filter(fr_filter))
        .with(tracing_subscriber::fmt::layer().with_filter(console_filter))
        .init();

    // Simulate an incoming HTTP request with a span.
    let request_id = "req-abc-123";
    let user_id = "user-42";

    {
        let request_span = tracing::info_span!(
            "http_request",
            method = "GET",
            path = "/api/users",
            request_id,
            user_id,
        );
        let _enter = request_span.enter();

        tracing::debug!("authenticating user");

        // Nested span — a database query inside the request.
        {
            let db_span = tracing::debug_span!(
                "db_query",
                table = "users",
                query = "SELECT * FROM users WHERE id = $1",
            );
            let _enter = db_span.enter();

            tracing::debug!(rows = 1, "query executed");
            tracing::warn!(latency_ms = 250, "slow query");
        }

        tracing::info!("request completed");
    }

    // Dump and show the span context in each captured event.
    let json = recorder.dump_to_json()?;
    let events: Vec<serde_json::Value> = serde_json::from_str(&json)?;

    println!("Captured {} events. Span context for each:\n", events.len());
    for event in &events {
        let level = event["level"].as_str().unwrap_or("?");
        let msg = event["message"].as_str().unwrap_or("");
        let spans = event["spans"].as_array();

        if let Some(spans) = spans {
            let names: Vec<&str> = spans
                .iter()
                .map(|s| s["name"].as_str().unwrap_or("?"))
                .collect();
            println!("  [{level}] {msg}");
            if !names.is_empty() {
                println!("       spans: {names:?}");
                for span in spans {
                    let name = span["name"].as_str().unwrap_or("?");
                    let fields = span["fields"]
                        .as_array()
                        .map(|f| {
                            f.iter()
                                .map(|kv| {
                                    format!(
                                        "{}={}",
                                        kv[0].as_str().unwrap_or("?"),
                                        kv[1].as_str().unwrap_or("?")
                                    )
                                })
                                .collect::<Vec<_>>()
                                .join(", ")
                        })
                        .unwrap_or_default();
                    println!("         {name}: {fields}");
                }
            }
        } else {
            println!("  [{level}] {msg} (no spans)");
        }
    }

    // Write to disk for inspection.
    let path = std::env::temp_dir().join("span-context-example.json");
    recorder.dump_to_file(&path)?;
    println!("\nFull dump written to {}", path.display());

    Ok(())
}
