use super::*;
use crate::capture::CapturedEvent;

fn make_event(msg: &str) -> CapturedEvent {
    CapturedEvent {
        timestamp: chrono::Utc::now(),
        level: "DEBUG".to_string(),
        target: "test".to_string(),
        message: msg.to_string(),
        fields: vec![],
    }
}

#[test]
fn ring_buffer_evicts_oldest_at_capacity() {
    let recorder = FlightRecorder::new(3);
    recorder.push(make_event("first"));
    recorder.push(make_event("second"));
    recorder.push(make_event("third"));
    recorder.push(make_event("fourth"));

    let snap = recorder.snapshot();
    assert_eq!(snap.len(), 3);
    assert_eq!(snap[0].message, "second");
    assert_eq!(snap[1].message, "third");
    assert_eq!(snap[2].message, "fourth");
}

#[test]
fn snapshot_returns_events_in_insertion_order() {
    let recorder = FlightRecorder::new(100);
    recorder.push(make_event("a"));
    recorder.push(make_event("b"));
    recorder.push(make_event("c"));

    let snap = recorder.snapshot();
    assert_eq!(snap.len(), 3);
    assert_eq!(snap[0].message, "a");
    assert_eq!(snap[1].message, "b");
    assert_eq!(snap[2].message, "c");
}

#[test]
fn dump_to_json_produces_valid_json_array() {
    let recorder = FlightRecorder::new(100);
    recorder.push(make_event("hello"));
    recorder.push(make_event("world"));

    let json = recorder.dump_to_json().unwrap_or_default();
    let parsed: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap_or_default();
    assert_eq!(parsed.len(), 2);
}

#[test]
fn dump_to_file_writes_valid_json() {
    let recorder = FlightRecorder::new(100);
    recorder.push(make_event("file-test"));

    let dir = tempfile_dir();
    let path = dir.join("fr-test.json");
    // If dump fails the parse will get an empty file and the len assertion will fail.
    let _ = recorder.dump_to_file(&path);

    let contents = std::fs::read_to_string(&path).unwrap_or_default();
    let parsed: Vec<serde_json::Value> = serde_json::from_str(&contents).unwrap_or_default();
    assert_eq!(parsed.len(), 1);
}

#[test]
fn clear_empties_buffer() {
    let recorder = FlightRecorder::new(100);
    recorder.push(make_event("x"));
    assert!(!recorder.is_empty());

    recorder.clear();
    assert!(recorder.is_empty());
    assert_eq!(recorder.len(), 0);
}

#[test]
fn capacity_one_evicts_immediately() {
    let recorder = FlightRecorder::new(1);
    recorder.push(make_event("a"));
    recorder.push(make_event("b"));

    let snap = recorder.snapshot();
    assert_eq!(snap.len(), 1);
    assert_eq!(snap[0].message, "b");
}

#[test]
fn clone_shares_same_buffer() {
    let recorder = FlightRecorder::new(100);
    let clone = recorder.clone();
    clone.push(make_event("from-clone"));

    let snap = recorder.snapshot();
    assert_eq!(snap.len(), 1);
    assert_eq!(snap[0].message, "from-clone");
}

fn tempfile_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fr-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos()),
    ));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

#[test]
fn dump_with_retention_creates_file_and_dir() {
    let recorder = FlightRecorder::new(100);
    recorder.push(make_event("retention-test"));

    let dir = tempfile_dir();
    let path = recorder.dump_with_retention(&dir, "test", 5).unwrap();

    assert!(path.exists());
    assert!(path.starts_with(&dir));

    let contents = std::fs::read_to_string(&path).unwrap();
    let parsed: Vec<serde_json::Value> = serde_json::from_str(&contents).unwrap();
    assert_eq!(parsed.len(), 1);
}

#[test]
fn dump_with_retention_prunes_old_snapshots() {
    let dir = tempfile_dir();

    // Pre-create 5 old snapshot files.
    for i in 0..5 {
        let p = dir.join(format!("snap-2026010T00000{i}.json"));
        std::fs::write(&p, "[]").unwrap();
        // Stagger mtimes so sort is deterministic.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644));
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    let recorder = FlightRecorder::new(100);
    recorder.push(make_event("new"));
    let new_path = recorder.dump_with_retention(&dir, "snap", 3).unwrap();

    // Count remaining snap-*.json files.
    let remaining = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(std::result::Result::ok)
        .filter(|e| {
            e.file_name().to_str().is_some_and(|n| {
                n.starts_with("snap-")
                    && std::path::Path::new(n)
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
            })
        })
        .count();

    assert_eq!(remaining, 3, "retention should keep at most 3 files");
    assert!(new_path.exists());
}

// ── End-to-end pipeline tests ──────────────────────────────────────────
//
// These tests install a real tracing subscriber via `with_default`, emit
// actual `tracing::debug!`/`info!` events, and verify they land in the
// recorder — exercising the full Event → FieldVisitor → push pipeline.

#[test]
fn layer_captures_real_tracing_events() {
    use tracing_subscriber::layer::SubscriberExt;

    let recorder = FlightRecorder::new(100);
    let layer = FlightRecorderLayer::new(recorder.clone());

    let subscriber = tracing_subscriber::registry().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        tracing::debug!("a debug message");
        tracing::info!("an info message");
        tracing::warn!("a warn message");
    });

    let snap = recorder.snapshot();
    assert_eq!(snap.len(), 3);
    assert_eq!(snap[0].level, "DEBUG");
    assert_eq!(snap[0].message, "a debug message");
    assert_eq!(snap[1].level, "INFO");
    assert_eq!(snap[2].level, "WARN");
}

#[test]
fn layer_captures_structured_fields_from_real_events() {
    use tracing_subscriber::layer::SubscriberExt;

    let recorder = FlightRecorder::new(100);
    let layer = FlightRecorderLayer::new(recorder.clone());

    let subscriber = tracing_subscriber::registry().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        tracing::info!(
            device = "dev-1",
            count = 42i64,
            active = true,
            ratio = 0.95f64,
            "sync completed"
        );
    });

    let snap = recorder.snapshot();
    assert_eq!(snap.len(), 1);
    let event = &snap[0];
    assert_eq!(event.message, "sync completed");

    let fields: std::collections::HashMap<&str, &str> = event
        .fields
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    assert_eq!(fields.get("device"), Some(&"dev-1"));
    assert_eq!(fields.get("count"), Some(&"42"));
    assert_eq!(fields.get("active"), Some(&"true"));
    assert!(fields.contains_key("ratio"));
}

#[test]
fn flight_recorder_sees_events_blocked_by_other_layer_filter() {
    // This is the core regression test for the EnvFilter design flaw.
    // The fmt layer has a filter that blocks DEBUG, but the FR layer has
    // no filter — it must still capture the DEBUG event.
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::Layer;

    let recorder = FlightRecorder::new(100);
    let fr_layer = FlightRecorderLayer::new(recorder.clone());
    let fmt_filter = tracing_subscriber::EnvFilter::new("info");
    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::sink)
        .with_filter(fmt_filter);

    let subscriber = tracing_subscriber::registry()
        .with(fr_layer)
        .with(fmt_layer);

    tracing::subscriber::with_default(subscriber, || {
        tracing::debug!("hidden from console, captured by FR");
        tracing::info!("visible everywhere");
    });

    let snap = recorder.snapshot();
    assert_eq!(
        snap.len(),
        2,
        "FR must capture both events even though fmt filter blocks DEBUG"
    );
    assert_eq!(snap[0].level, "DEBUG");
    assert_eq!(snap[0].message, "hidden from console, captured by FR");
    assert_eq!(snap[1].level, "INFO");
}

#[test]
fn sensitive_fields_are_redacted() {
    use tracing_subscriber::layer::SubscriberExt;

    let recorder = FlightRecorder::new(100);
    let layer = FlightRecorderLayer::new(recorder.clone());

    let subscriber = tracing_subscriber::registry().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        tracing::info!(
            auth_token = "secret-token-value",
            api_key = "sk-1234567890",
            password = "hunter2",
            device = "dev-1",
            "login attempt"
        );
    });

    let snap = recorder.snapshot();
    assert_eq!(snap.len(), 1);
    let event = &snap[0];
    let fields: std::collections::HashMap<&str, &str> = event
        .fields
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();

    assert_eq!(fields.get("auth_token"), Some(&"[REDACTED]"));
    assert_eq!(fields.get("api_key"), Some(&"[REDACTED]"));
    assert_eq!(fields.get("password"), Some(&"[REDACTED]"));
    assert_eq!(fields.get("device"), Some(&"dev-1"));
}

#[test]
fn dump_with_retention_does_not_overwrite_same_second() {
    let recorder = FlightRecorder::new(100);
    recorder.push(make_event("collision-test"));

    let dir = tempfile_dir();

    // Pre-create a file that collides with the timestamp-based name
    // to deterministically exercise the collision guard.
    let ts = chrono::Utc::now().format("%Y%m%dT%H%M%S");
    let colliding = dir.join(format!("collide-{ts}.json"));
    std::fs::write(&colliding, "[]").unwrap();

    let path = recorder.dump_with_retention(&dir, "collide", 10).unwrap();

    // The pre-existing file must survive — no silent overwrite.
    assert!(
        colliding.exists(),
        "pre-existing file must not be overwritten"
    );
    // The dump must land at a distinct path.
    assert_ne!(
        path, colliding,
        "same-second dump must get a counter suffix"
    );
    assert!(path.exists(), "dump file should exist");

    // Verify the dumped content is valid.
    let contents = std::fs::read_to_string(&path).unwrap();
    let parsed: Vec<serde_json::Value> = serde_json::from_str(&contents).unwrap();
    assert_eq!(parsed.len(), 1);
}
