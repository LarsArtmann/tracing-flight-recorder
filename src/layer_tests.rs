use super::*;
use crate::capture::{CapturedEvent, DumpEvent, DumpSource};
use proptest::prelude::*;

fn make_event(msg: &str) -> CapturedEvent {
    CapturedEvent {
        timestamp: chrono::Utc::now(),
        level: "DEBUG".into(),
        target: "test".to_string(),
        message: msg.to_string(),
        fields: vec![],
        spans: vec![],
    }
}

/// Approximate total heap+stack size of a `CapturedEvent`, accounting for the
/// actual allocated **capacity** of every `String`/`Vec` rather than just
/// `.len()`.
///
/// This is far more accurate than summing `size_of` + `len()`, which ignores
/// allocator capacity rounding (strings often reserve ~2× their length). The
/// only thing it cannot see is the global allocator's per-allocation alignment
/// padding, so the true figure may be a few percent higher still.
fn deep_size_of_captured_event(e: &CapturedEvent) -> usize {
    use std::mem::size_of;

    // Stack footprint of the struct itself (Cow discriminant, Vec bookkeeping,
    // DateTime, etc.).
    let stack = size_of::<CapturedEvent>();

    // `level` heap: only `Owned` variants allocate; `Borrowed` is zero-copy.
    let level_heap = match &e.level {
        std::borrow::Cow::Borrowed(_) => 0,
        std::borrow::Cow::Owned(s) => s.capacity(),
    };
    let target_heap = e.target.capacity();
    let message_heap = e.message.capacity();

    // fields: Vec<(String, String)> — element buffer + each inner String heap.
    let fields_buf = e.fields.capacity() * size_of::<(String, String)>();
    let fields_strings: usize = e
        .fields
        .iter()
        .map(|(k, v)| k.capacity() + v.capacity())
        .sum();

    // spans: Vec<SpanContext> — element buffer + each span's name + its fields.
    let spans_buf = e.spans.capacity() * size_of::<SpanContext>();
    let spans_inner: usize = e
        .spans
        .iter()
        .map(|s| {
            let name = s.name.capacity();
            let buf = s.fields.capacity() * size_of::<(String, String)>();
            let strings: usize = s
                .fields
                .iter()
                .map(|(k, v)| k.capacity() + v.capacity())
                .sum();
            name + buf + strings
        })
        .sum();

    stack
        + level_heap
        + target_heap
        + message_heap
        + fields_buf
        + fields_strings
        + spans_buf
        + spans_inner
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
fn dump_to_writer_produces_valid_json() {
    let recorder = FlightRecorder::new(100);
    recorder.push(make_event("writer-test-1"));
    recorder.push(make_event("writer-test-2"));

    let mut buf = Vec::new();
    recorder.dump_to_writer(&mut buf).unwrap();

    let json = String::from_utf8(buf).unwrap();
    let parsed: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.len(), 2);
}

#[test]
fn dump_to_json_lines_produces_valid_ndjson() {
    let recorder = FlightRecorder::new(100);
    recorder.push(make_event("line-1"));
    recorder.push(make_event("line-2"));
    recorder.push(make_event("line-3"));

    let ndjson = recorder.dump_to_json_lines().unwrap();
    let lines: Vec<&str> = ndjson.lines().collect();
    assert_eq!(lines.len(), 3, "one JSON object per line");

    // Each line must be a valid standalone JSON object.
    for (i, line) in lines.iter().enumerate() {
        let parsed: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("line {i} is not valid JSON: {e}\n  line: {line}"));
        assert!(parsed.is_object(), "each line must be a JSON object");
    }

    // Verify message field round-trips.
    let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(first["message"], "line-1");
}

#[test]
fn dump_to_json_lines_empty_buffer_produces_empty_string() {
    let recorder = FlightRecorder::new(100);
    let ndjson = recorder.dump_to_json_lines().unwrap();
    assert!(
        ndjson.is_empty(),
        "empty buffer should produce empty NDJSON"
    );
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
fn capacity_zero_retains_nothing() {
    let recorder = FlightRecorder::new(0);
    recorder.push(make_event("a"));
    recorder.push(make_event("b"));
    assert_eq!(recorder.len(), 0, "capacity 0 must retain zero events");
    assert!(recorder.is_empty());
    assert!(recorder.snapshot().is_empty());
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

// ── M8: Test hardening ────────────────────────────────────────────────

#[test]
fn recorder_recovers_from_poisoned_mutex() {
    let recorder = FlightRecorder::new(100);
    recorder.push(make_event("before-poison"));

    // Poison the mutex by panicking while holding the lock.
    let recorder_clone = recorder.clone();
    let handle = std::thread::spawn(move || {
        let _guard = recorder_clone
            .buffer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        panic!("intentional panic while holding lock");
    });
    // Wait for the panicking thread to finish (ignore the panic).
    let _ = handle.join();

    // The recorder must still be usable — poison-safe recovery.
    recorder.push(make_event("after-poison"));
    let snap = recorder.snapshot();
    assert!(
        snap.iter().any(|e| e.message == "after-poison"),
        "recorder must be usable after mutex poison"
    );
    assert!(
        snap.iter().any(|e| e.message == "before-poison"),
        "pre-poison events must survive"
    );
}

#[test]
fn unicode_field_names_with_ascii_sensitive_substring_are_redacted() {
    use tracing_subscriber::layer::SubscriberExt;

    let recorder = FlightRecorder::new(100);
    let layer = FlightRecorderLayer::new(recorder.clone());

    let subscriber = tracing_subscriber::registry().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        tracing::info!(
            café_token = "unicode-prefixed-token",
            näme = "harmless-unicode-name",
            "unicode test"
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

    // "café_token" contains the ASCII substring "token" → redacted.
    assert_eq!(
        fields.get("café_token"),
        Some(&"[REDACTED]"),
        "ASCII 'token' substring inside Unicode field name must be caught"
    );
    // "näme" is harmless → preserved.
    assert_eq!(fields.get("näme"), Some(&"harmless-unicode-name"));
}

#[test]
fn dump_to_file_creates_nested_directories() {
    let recorder = FlightRecorder::new(100);
    recorder.push(make_event("nested-test"));

    let base = tempfile_dir();
    let nested = base.join("a").join("b").join("c");
    let path = nested.join("deep-dump.json");

    recorder.dump_to_file(&path).unwrap();

    assert!(path.exists());
    let contents = std::fs::read_to_string(&path).unwrap();
    let parsed: Vec<serde_json::Value> = serde_json::from_str(&contents).unwrap();
    assert_eq!(parsed.len(), 1);
}

#[test]
fn retention_pruning_leaves_non_json_files_alone() {
    let dir = tempfile_dir();

    // Pre-create some old JSON snapshots + some non-JSON files.
    for i in 0..3 {
        std::fs::write(dir.join(format!("snap-2026010T00000{i}.json")), "[]").unwrap();
    }
    std::fs::write(dir.join("snap-readme.txt"), "notes").unwrap();
    std::fs::write(dir.join("snap-config.yaml"), "key: value").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));

    let recorder = FlightRecorder::new(100);
    recorder.push(make_event("new"));
    let _ = recorder.dump_with_retention(&dir, "snap", 2).unwrap();

    // Non-JSON files must survive.
    assert!(
        dir.join("snap-readme.txt").exists(),
        "txt file must survive pruning"
    );
    assert!(
        dir.join("snap-config.yaml").exists(),
        "yaml file must survive pruning"
    );

    // JSON files should be pruned to max_files (2).
    let json_count = std::fs::read_dir(&dir)
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
    assert!(
        json_count <= 2,
        "at most 2 JSON snapshots should remain, got {json_count}"
    );
}

// ── M9: Concurrency + property tests ─────────────────────────────────

proptest::proptest! {
    #[test]
    fn eviction_invariant_len_never_exceeds_capacity(
        capacity in 0usize..=500,
        num_events in 0usize..=2000,
    ) {
        let recorder = FlightRecorder::new(capacity);
        for i in 0..num_events {
            recorder.push(make_event(&format!("event-{i}")));
        }

        let snap = recorder.snapshot();
        prop_assert!(
            snap.len() <= capacity,
            "snapshot len {} exceeded capacity {}",
            snap.len(),
            capacity
        );

        let expected_len = num_events.min(capacity);
        prop_assert_eq!(snap.len(), expected_len);

        // If we evicted, the oldest events should be gone and newest present.
        if capacity > 0 && num_events > capacity {
            let first_expected = num_events - capacity;
            prop_assert_eq!(&snap[0].message, &format!("event-{first_expected}"));
        }
        if capacity > 0 && num_events > 0 {
            prop_assert_eq!(&snap[snap.len() - 1].message, &format!("event-{}", num_events - 1));
        }
    }
}

#[test]
fn multi_thread_stress_push_and_snapshot() {
    use std::sync::Arc;
    use std::thread;

    let capacity = 200;
    let recorder = Arc::new(FlightRecorder::new(capacity));
    let num_threads = 8;
    let events_per_thread = 100;

    let mut handles = vec![];
    for t in 0..num_threads {
        let rc = Arc::clone(&recorder);
        handles.push(thread::spawn(move || {
            for i in 0..events_per_thread {
                rc.push(CapturedEvent {
                    timestamp: chrono::Utc::now(),
                    level: "DEBUG".into(),
                    target: format!("thread-{t}"),
                    message: format!("msg-{t}-{i}"),
                    fields: vec![],
                    spans: vec![],
                });
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    let snap = recorder.snapshot();
    assert!(
        snap.len() <= capacity,
        "snapshot must not exceed capacity: {} > {}",
        snap.len(),
        capacity
    );
    let total_pushed = num_threads * events_per_thread;
    let expected_len = total_pushed.min(capacity);
    assert_eq!(
        snap.len(),
        expected_len,
        "snapshot should contain exactly min(pushed, capacity) events"
    );

    // No event should be corrupted — all messages follow the expected pattern.
    for event in &snap {
        assert!(
            event.message.starts_with("msg-"),
            "corrupted event message: {}",
            event.message
        );
    }
}

// ── M13: Memory footprint measurement ────────────────────────────────

#[test]
fn memory_footprint_of_default_capacity_buffer() {
    let recorder = FlightRecorder::new(DEFAULT_CAPACITY);

    // Fill with representative events (realistic field sizes).
    for i in 0..DEFAULT_CAPACITY {
        recorder.push(CapturedEvent {
            timestamp: chrono::Utc::now(),
            level: "DEBUG".into(),
            target: format!("my_app::service::handler::{i}"),
            message: format!("Processing request batch #{i} with timeout 30s"),
            fields: vec![
                ("request_id".to_string(), format!("req-{i:04x}")),
                ("duration_ms".to_string(), "42".to_string()),
                ("status".to_string(), "ok".to_string()),
            ],
            spans: vec![],
        });
    }

    let snap = recorder.snapshot();
    let total_bytes: usize = snap.iter().map(deep_size_of_captured_event).sum();

    println!(
        "1000-event buffer (deep size, incl. capacity rounding): ~{} bytes ({:.1} KB), ~{:.0} bytes/event — README claims ~200-500 KB",
        total_bytes,
        total_bytes as f64 / 1024.0,
        total_bytes as f64 / DEFAULT_CAPACITY as f64
    );

    // With realistic events (~100-200 bytes each), 1000 events = ~200-500 KB.
    assert!(
        total_bytes < 1_000_000,
        "memory footprint {total_bytes} exceeds expected ~500KB range"
    );
}

// ── Span context capture tests ───────────────────────────────────────
//
// These verify that events fired inside spans capture the full span hierarchy
// (names + fields), addressing the central design flaw where `_ctx` was discarded.

#[test]
fn event_inside_single_span_captures_span_context() {
    use tracing_subscriber::layer::SubscriberExt;

    let recorder = FlightRecorder::new(100);
    let layer = FlightRecorderLayer::new(recorder.clone());

    let subscriber = tracing_subscriber::registry().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        let span = tracing::info_span!("http_request", method = "GET", path = "/api/users");
        let _enter = span.enter();
        tracing::error!("database query failed");
    });

    let snap = recorder.snapshot();
    assert_eq!(snap.len(), 1);
    let event = &snap[0];
    assert_eq!(event.message, "database query failed");
    assert_eq!(event.spans.len(), 1, "event should have one parent span");
    assert_eq!(event.spans[0].name, "http_request");

    let span_fields: std::collections::HashMap<&str, &str> = event.spans[0]
        .fields
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    assert_eq!(span_fields.get("method"), Some(&"GET"));
    assert_eq!(span_fields.get("path"), Some(&"/api/users"));
}

#[test]
fn event_inside_nested_spans_captures_full_hierarchy() {
    use tracing_subscriber::layer::SubscriberExt;

    let recorder = FlightRecorder::new(100);
    let layer = FlightRecorderLayer::new(recorder.clone());

    let subscriber = tracing_subscriber::registry().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        let outer = tracing::info_span!("http_request", request_id = "req-abc");
        let _outer_guard = outer.enter();
        let inner = tracing::debug_span!("db_query", table = "users");
        let _inner_guard = inner.enter();
        tracing::warn!("connection timeout");
    });

    let snap = recorder.snapshot();
    assert_eq!(snap.len(), 1);
    let event = &snap[0];
    assert_eq!(
        event.spans.len(),
        2,
        "event should capture both parent spans"
    );
    // Root-first ordering: http_request is outermost, db_query is innermost.
    assert_eq!(event.spans[0].name, "http_request");
    assert_eq!(event.spans[1].name, "db_query");

    let outer_fields: std::collections::HashMap<&str, &str> = event.spans[0]
        .fields
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    assert_eq!(outer_fields.get("request_id"), Some(&"req-abc"));

    let inner_fields: std::collections::HashMap<&str, &str> = event.spans[1]
        .fields
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    assert_eq!(inner_fields.get("table"), Some(&"users"));
}

#[test]
fn event_outside_any_span_has_empty_span_context() {
    use tracing_subscriber::layer::SubscriberExt;

    let recorder = FlightRecorder::new(100);
    let layer = FlightRecorderLayer::new(recorder.clone());

    let subscriber = tracing_subscriber::registry().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        tracing::info!("standalone event");
    });

    let snap = recorder.snapshot();
    assert_eq!(snap.len(), 1);
    assert!(
        snap[0].spans.is_empty(),
        "event outside any span should have empty spans vec"
    );
}

#[test]
fn sensitive_span_fields_are_redacted() {
    use tracing_subscriber::layer::SubscriberExt;

    let recorder = FlightRecorder::new(100);
    let layer = FlightRecorderLayer::new(recorder.clone());

    let subscriber = tracing_subscriber::registry().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        let span = tracing::info_span!(
            "auth",
            authorization = "Bearer secret-token",
            password = "hunter2",
            user_id = "user-42"
        );
        let _enter = span.enter();
        tracing::error!("auth failed");
    });

    let snap = recorder.snapshot();
    assert_eq!(snap.len(), 1);
    let span_fields: std::collections::HashMap<&str, &str> = snap[0].spans[0]
        .fields
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();

    assert_eq!(
        span_fields.get("authorization"),
        Some(&"[REDACTED]"),
        "span authorization field must be redacted"
    );
    assert_eq!(
        span_fields.get("password"),
        Some(&"[REDACTED]"),
        "span password field must be redacted"
    );
    assert_eq!(
        span_fields.get("user_id"),
        Some(&"user-42"),
        "non-sensitive span field must be preserved"
    );
}

#[test]
fn span_fields_updated_via_record_are_captured() {
    use tracing_subscriber::layer::SubscriberExt;

    let recorder = FlightRecorder::new(100);
    let layer = FlightRecorderLayer::new(recorder.clone());

    let subscriber = tracing_subscriber::registry().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        let span = tracing::info_span!(
            "request",
            method = "POST",
            status_code = tracing::field::Empty
        );
        let _enter = span.enter();
        span.record("status_code", 500i64);
        tracing::error!("handler failed");
    });

    let snap = recorder.snapshot();
    assert_eq!(snap.len(), 1);
    let span_fields: std::collections::HashMap<&str, &str> = snap[0].spans[0]
        .fields
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();

    assert_eq!(span_fields.get("method"), Some(&"POST"));
    assert_eq!(
        span_fields.get("status_code"),
        Some(&"500"),
        "field added via span.record() must be captured"
    );
}

// ── Redaction pattern tests ──────────────────────────────────────────

#[test]
fn expanded_redaction_patterns_cover_http_credentials() {
    use tracing_subscriber::layer::SubscriberExt;

    let recorder = FlightRecorder::new(100);
    let layer = FlightRecorderLayer::new(recorder.clone());

    let subscriber = tracing_subscriber::registry().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        tracing::info!(
            authorization = "Bearer abc123",
            cookie = "session=xyz",
            session_id = "sess-456",
            access_code = "code-789",
            bearer = "token-value",
            device = "dev-1",
            "http request"
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

    assert_eq!(fields.get("authorization"), Some(&"[REDACTED]"));
    assert_eq!(fields.get("cookie"), Some(&"[REDACTED]"));
    assert_eq!(fields.get("session_id"), Some(&"[REDACTED]"));
    assert_eq!(fields.get("access_code"), Some(&"[REDACTED]"));
    assert_eq!(fields.get("bearer"), Some(&"[REDACTED]"));
    assert_eq!(fields.get("device"), Some(&"dev-1"));
}

#[test]
fn dump_with_retention_zero_max_files_means_unlimited() {
    let recorder = FlightRecorder::new(100);
    recorder.push(make_event("zero-retention"));

    let dir = tempfile_dir();
    // max_files=0 must mean "no retention cleanup" (unlimited), NOT "delete everything."
    // The just-written snapshot must survive.
    let path = recorder.dump_with_retention(&dir, "snap", 0).unwrap();

    assert!(
        path.exists(),
        "snapshot must not be deleted when max_files=0"
    );

    let contents = std::fs::read_to_string(&path).unwrap();
    let parsed: Vec<serde_json::Value> = serde_json::from_str(&contents).unwrap();
    assert_eq!(parsed.len(), 1);
}

#[test]
fn resolve_collision_path_returns_error_at_limit() {
    let dir = tempfile_dir();
    let base = "snap-2026010T120000";

    // Saturate all slots up to a small limit of 3:
    //   {base}.json, {base}-1.json, {base}-2.json, {base}-3.json
    std::fs::write(dir.join(format!("{base}.json")), "[]").unwrap();
    for i in 1..=3 {
        std::fs::write(dir.join(format!("{base}-{i}.json")), "[]").unwrap();
    }

    let err = resolve_collision_path(&dir, base, 3).unwrap_err();
    assert_eq!(
        err.kind(),
        std::io::ErrorKind::AlreadyExists,
        "exceeding the collision limit must return AlreadyExists"
    );
}

#[test]
fn resolve_collision_path_finds_first_free_slot() {
    let dir = tempfile_dir();
    let base = "snap-2026010T120000";

    // Only the primary + slot 1 exist → slot 2 is free.
    std::fs::write(dir.join(format!("{base}.json")), "[]").unwrap();
    std::fs::write(dir.join(format!("{base}-1.json")), "[]").unwrap();

    let path = resolve_collision_path(&dir, base, 3).unwrap();
    assert_eq!(
        path,
        dir.join(format!("{base}-2.json")),
        "must return the first available counter suffix"
    );
    assert!(!path.exists(), "returned path should not yet exist");
}

#[test]
fn resolve_collision_path_returns_primary_when_free() {
    let dir = tempfile_dir();
    let base = "snap-2026010T120000";

    // No files exist → primary path returned immediately.
    let path = resolve_collision_path(&dir, base, 3).unwrap();
    assert_eq!(path, dir.join(format!("{base}.json")));
}

// ── dump_to_writer / dump_to_writer_lines tests ──────────────────────

#[test]
fn dump_to_writer_writes_valid_json_to_sink() {
    let recorder = FlightRecorder::new(100);
    recorder.push(make_event("writer-test-1"));
    recorder.push(make_event("writer-test-2"));

    let mut sink = std::io::sink();
    let result = recorder.dump_to_writer(&mut sink);
    assert!(result.is_ok(), "dump_to_writer to sink should succeed");
}

#[test]
fn dump_to_writer_lines_produces_valid_ndjson() {
    let recorder = FlightRecorder::new(100);
    recorder.push(make_event("line-1"));
    recorder.push(make_event("line-2"));
    recorder.push(make_event("line-3"));

    let mut buf = Vec::new();
    recorder.dump_to_writer_lines(&mut buf).unwrap();

    let output = String::from_utf8(buf).unwrap();
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines.len(), 3, "should have one line per event");

    for line in lines {
        let parsed: serde_json::Value =
            serde_json::from_str(line).expect("each line must be valid JSON");
        assert!(
            parsed.is_object(),
            "each NDJSON line must be a JSON object, not an array"
        );
    }
}

#[test]
fn dump_to_writer_lines_empty_buffer_writes_nothing() {
    let recorder = FlightRecorder::new(100);
    let mut buf = Vec::new();
    recorder.dump_to_writer_lines(&mut buf).unwrap();
    assert!(buf.is_empty(), "empty buffer should produce no output");
}

// ── Span context + per-layer filtering ───────────────────────────────

#[test]
fn span_context_captured_with_per_layer_filter() {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::Layer;
    let recorder = FlightRecorder::new(100);
    let fr_filter = tracing_subscriber::EnvFilter::new("my_app=debug");

    let subscriber = tracing_subscriber::registry()
        .with(FlightRecorderLayer::new(recorder.clone()).with_filter(fr_filter));

    tracing::subscriber::with_default(subscriber, || {
        let span = tracing::info_span!(target: "my_app", "request", request_id = "req-123");
        let _enter = span.enter();
        tracing::debug!(target: "my_app", "processing inside span");
    });

    let snap = recorder.snapshot();
    assert_eq!(snap.len(), 1);
    assert_eq!(snap[0].spans.len(), 1);
    assert_eq!(snap[0].spans[0].name, "request");

    let fields: std::collections::HashMap<&str, &str> = snap[0].spans[0]
        .fields
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    assert_eq!(fields.get("request_id"), Some(&"req-123"));
}

// ── Edge case tests ──────────────────────────────────────────────────

#[test]
fn snapshot_on_empty_recorder_returns_empty_vec() {
    let recorder = FlightRecorder::new(100);
    assert!(recorder.snapshot().is_empty());
    assert!(recorder.is_empty());
    assert_eq!(recorder.len(), 0);
}

#[test]
fn span_with_no_fields_produces_empty_fields_vec() {
    use tracing_subscriber::layer::SubscriberExt;

    let recorder = FlightRecorder::new(100);
    let layer = FlightRecorderLayer::new(recorder.clone());

    let subscriber = tracing_subscriber::registry().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        let span = tracing::info_span!("bare_span");
        let _enter = span.enter();
        tracing::info!("event inside a span with no fields");
    });

    let snap = recorder.snapshot();
    assert_eq!(snap.len(), 1);
    assert_eq!(snap[0].spans.len(), 1);
    assert_eq!(snap[0].spans[0].name, "bare_span");
    assert!(
        snap[0].spans[0].fields.is_empty(),
        "span with no fields should have empty fields vec"
    );
}

// ── Configurable span context capture ────────────────────────────────

#[test]
fn span_capture_disabled_produces_empty_spans() {
    use tracing_subscriber::layer::SubscriberExt;

    let recorder = FlightRecorder::new(100);
    let layer = FlightRecorderLayer::with_span_capture(recorder.clone(), false);

    let subscriber = tracing_subscriber::registry().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        let span = tracing::info_span!("http_request", method = "GET", path = "/api");
        let _enter = span.enter();
        tracing::error!("something failed");
    });

    let snap = recorder.snapshot();
    assert_eq!(snap.len(), 1, "event must still be captured");
    assert_eq!(snap[0].message, "something failed");
    assert!(
        snap[0].spans.is_empty(),
        "span capture disabled: spans vec must be empty"
    );
}

#[test]
fn span_capture_enabled_is_the_default() {
    use tracing_subscriber::layer::SubscriberExt;

    let recorder = FlightRecorder::new(100);
    // new() must default to capturing spans.
    let layer = FlightRecorderLayer::new(recorder.clone());

    let subscriber = tracing_subscriber::registry().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        let span = tracing::info_span!("outer", id = "x");
        let _enter = span.enter();
        tracing::warn!("triggered");
    });

    let snap = recorder.snapshot();
    assert_eq!(snap.len(), 1);
    assert_eq!(snap[0].spans.len(), 1, "new() defaults to span capture ON");
    assert_eq!(snap[0].spans[0].name, "outer");
}

// ── Dump metadata envelope tests ─────────────────────────────────────

#[test]
fn dump_envelope_to_file_writes_metadata_and_events() {
    let recorder = FlightRecorder::new(100);
    recorder.push(make_event("envelope-event-1"));
    recorder.push(make_event("envelope-event-2"));

    let dir = tempfile_dir();
    let path = dir.join("envelope.json");
    recorder
        .dump_envelope_to_file(&path, Some("test_reason"))
        .unwrap();

    let contents = std::fs::read_to_string(&path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&contents).unwrap();

    // Envelope is an object, not a bare event array.
    assert!(
        parsed.is_object(),
        "envelope must be a JSON object, not an array"
    );
    assert_eq!(parsed["schema_version"], 1);
    assert_eq!(parsed["event_count"], 2);
    assert_eq!(parsed["trigger_reason"], "test_reason");
    assert!(
        parsed["crate_version"].is_string(),
        "crate_version must be present"
    );
    assert!(
        parsed["captured_at"].is_string(),
        "captured_at must be present"
    );
    assert_eq!(
        parsed["events"].as_array().map_or(0, Vec::len),
        2,
        "events array must contain both events"
    );
}

#[test]
fn dump_envelope_to_json_manual_dump_has_null_reason() {
    let recorder = FlightRecorder::new(100);
    recorder.push(make_event("solo"));
    let json = recorder.dump_envelope_to_json(None).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(
        parsed["trigger_reason"].is_null(),
        "manual dump must have null trigger_reason"
    );
    assert_eq!(parsed["event_count"], 1);
}

#[test]
fn dump_envelope_round_trips_through_json() {
    let recorder = FlightRecorder::new(100);
    recorder.push(make_event("rt-1"));
    let json = recorder.dump_envelope_to_json(Some("roundtrip")).unwrap();
    let parsed: FlightRecorderDump =
        serde_json::from_str(&json).expect("envelope must deserialize back");
    assert_eq!(parsed.schema_version, DUMP_SCHEMA_VERSION);
    assert_eq!(parsed.event_count, 1);
    assert_eq!(parsed.trigger_reason.as_deref(), Some("roundtrip"));
    assert_eq!(parsed.events[0].message, "rt-1");
}

#[test]
fn dump_with_retention_envelope_writes_envelope_format() {
    let recorder = FlightRecorder::new(100);
    recorder.push(make_event("retention-envelope"));

    let dir = tempfile_dir();
    let path = recorder
        .dump_with_retention_envelope(&dir, "env", 5, Some("crash"))
        .unwrap();

    assert!(path.exists());
    let contents = std::fs::read_to_string(&path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&contents).unwrap();
    assert!(parsed.is_object(), "retention envelope must be an object");
    assert_eq!(parsed["trigger_reason"], "crash");
    assert_eq!(parsed["schema_version"], 1);
}

// ── Trigger system integration tests ─────────────────────────────────

fn count_files_matching(dir: &std::path::Path, prefix: &str) -> usize {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(std::result::Result::ok)
        .filter(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|n| n.starts_with(prefix))
        })
        .count()
}

#[test]
fn trigger_dumps_automatically_on_error() {
    use crate::LevelTrigger;
    use tracing_subscriber::layer::SubscriberExt;
    let dir = tempfile_dir();
    let recorder = FlightRecorder::new(100);
    let layer = FlightRecorderLayer::new(recorder.clone()).with_dump_on(
        LevelTrigger::error(),
        dir.clone(),
        "incident",
        10,
    );

    let subscriber = tracing_subscriber::registry().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        tracing::info!("harmless info");
        tracing::error!("something exploded");
    });

    assert_eq!(
        count_files_matching(&dir, "incident"),
        1,
        "exactly one trigger dump expected on ERROR"
    );

    // The buffer retains both events (info + error).
    assert_eq!(recorder.snapshot().len(), 2);
}

#[test]
fn trigger_dump_envelope_carries_reason_and_events() {
    use crate::LevelTrigger;
    use tracing_subscriber::layer::SubscriberExt;
    let dir = tempfile_dir();
    let recorder = FlightRecorder::new(100);
    let layer = FlightRecorderLayer::new(recorder.clone()).with_dump_on(
        LevelTrigger::error(),
        dir.clone(),
        "snap",
        10,
    );

    let subscriber = tracing_subscriber::registry().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        tracing::warn!("warning before");
        tracing::error!("the trigger");
    });

    let dump_path = std::fs::read_dir(&dir)
        .unwrap()
        .find_map(std::result::Result::ok)
        .expect("a dump file must exist");
    let contents = std::fs::read_to_string(dump_path.path()).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&contents).unwrap();

    assert!(
        parsed.is_object(),
        "trigger dump must be an envelope object"
    );
    assert_eq!(parsed["trigger_reason"], "level>=ERROR");
    assert_eq!(
        parsed["event_count"], 2,
        "both events must be in the snapshot"
    );
    assert_eq!(parsed["schema_version"], 1);
}

#[test]
fn once_trigger_dumps_only_once_across_multiple_errors() {
    use crate::{LevelTrigger, OnceTrigger};
    use tracing_subscriber::layer::SubscriberExt;
    let dir = tempfile_dir();
    let recorder = FlightRecorder::new(100);
    let layer = FlightRecorderLayer::new(recorder.clone()).with_dump_on(
        OnceTrigger::new(LevelTrigger::error()),
        dir.clone(),
        "once",
        10,
    );

    let subscriber = tracing_subscriber::registry().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        tracing::error!("first explosion");
        tracing::error!("second explosion");
        tracing::error!("third explosion");
    });

    assert_eq!(
        count_files_matching(&dir, "once"),
        1,
        "OnceTrigger must produce exactly one dump despite three errors"
    );
}

#[test]
fn layer_without_trigger_creates_no_dump_files() {
    use tracing_subscriber::layer::SubscriberExt;

    let dir = tempfile_dir();
    let recorder = FlightRecorder::new(100);
    let layer = FlightRecorderLayer::new(recorder.clone());

    let subscriber = tracing_subscriber::registry().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        tracing::error!("no trigger attached");
    });

    assert_eq!(
        count_files_matching(&dir, ""),
        0,
        "no dump files without a trigger"
    );
}

// ── Arc span-field sharing ───────────────────────────────────────────

#[test]
fn events_in_same_span_share_span_fields_allocation() {
    use tracing_subscriber::layer::SubscriberExt;

    let recorder = FlightRecorder::new(100);
    let layer = FlightRecorderLayer::new(recorder.clone());

    let subscriber = tracing_subscriber::registry().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        let span = tracing::info_span!("shared", request_id = "req-1");
        let _enter = span.enter();
        tracing::warn!("first event");
        tracing::warn!("second event");
    });

    let snap = recorder.snapshot();
    assert_eq!(snap.len(), 2);
    assert_eq!(snap[0].spans.len(), 1);
    assert_eq!(snap[1].spans.len(), 1);
    assert!(
        std::sync::Arc::ptr_eq(&snap[0].spans[0].fields, &snap[1].spans[0].fields),
        "events in the same span must share the span-fields Arc (O(1) clone)"
    );
}

#[test]
fn span_fields_updated_via_record_do_not_mutate_already_captured_events() {
    use tracing_subscriber::layer::SubscriberExt;

    // clone-on-write: recording a new field after an event fired must not
    // retroactively change the already-captured event's span fields.
    let recorder = FlightRecorder::new(100);
    let layer = FlightRecorderLayer::new(recorder.clone());
    let subscriber = tracing_subscriber::registry().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        let span = tracing::info_span!("cow", a = "1", b = tracing::field::Empty);
        let _enter = span.enter();
        tracing::warn!("before record");
        span.record("b", "2");
        tracing::warn!("after record");
    });

    let snap = recorder.snapshot();
    assert_eq!(snap.len(), 2);
    let before = &snap[0].spans[0].fields;
    let after = &snap[1].spans[0].fields;
    // Different allocations (clone-on-write split).
    assert!(
        !std::sync::Arc::ptr_eq(before, after),
        "clone-on-write: updated span fields must not alias the pre-update Arc"
    );
    // The pre-record event sees only {a}, the post-record event sees {a, b}.
    let before_keys: Vec<&str> = before.iter().map(|(k, _)| k.as_str()).collect();
    let after_keys: Vec<&str> = after.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(before_keys, vec!["a"]);
    assert_eq!(after_keys, vec!["a", "b"]);
}

// ── Stress / boundary edge cases ─────────────────────────────────────

#[test]
fn deeply_nested_spans_captures_full_hierarchy() {
    use tracing_subscriber::layer::SubscriberExt;

    let recorder = FlightRecorder::new(100);
    let layer = FlightRecorderLayer::new(recorder.clone());
    let subscriber = tracing_subscriber::registry().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        // 12 levels of nesting — stresses the `from_root()` span walking.
        let s1 = tracing::info_span!("level_01", id = "1");
        let _g1 = s1.enter();
        let s2 = tracing::info_span!("level_02", id = "2");
        let _g2 = s2.enter();
        let s3 = tracing::info_span!("level_03", id = "3");
        let _g3 = s3.enter();
        let s4 = tracing::info_span!("level_04", id = "4");
        let _g4 = s4.enter();
        let s5 = tracing::info_span!("level_05", id = "5");
        let _g5 = s5.enter();
        let s6 = tracing::info_span!("level_06", id = "6");
        let _g6 = s6.enter();
        let s7 = tracing::info_span!("level_07", id = "7");
        let _g7 = s7.enter();
        let s8 = tracing::info_span!("level_08", id = "8");
        let _g8 = s8.enter();
        let s9 = tracing::info_span!("level_09", id = "9");
        let _g9 = s9.enter();
        let s10 = tracing::info_span!("level_10", id = "10");
        let _g10 = s10.enter();
        let s11 = tracing::info_span!("level_11", id = "11");
        let _g11 = s11.enter();
        let s12 = tracing::info_span!("level_12", id = "12");
        let _g12 = s12.enter();
        tracing::error!("bottom of a very deep stack");
    });

    let snap = recorder.snapshot();
    assert_eq!(snap.len(), 1);
    let event = &snap[0];
    assert_eq!(
        event.spans.len(),
        12,
        "all 12 levels of the span hierarchy must be captured"
    );

    // Root-first ordering: level_01 is outermost, level_12 is innermost.
    let names: Vec<&str> = event.spans.iter().map(|s| s.name.as_str()).collect();
    for (i, name) in names.iter().enumerate() {
        let expected = format!("level_{:02}", i + 1);
        assert_eq!(
            *name,
            expected,
            "span at depth {} must be {expected}, got {name}",
            i + 1
        );
    }

    // Each level's `id` field must be present and correct.
    for (i, span) in event.spans.iter().enumerate() {
        let id_field = span
            .fields
            .iter()
            .find(|(k, _)| k == "id")
            .map(|(_, v)| v.as_str());
        assert_eq!(
            id_field,
            Some((i + 1).to_string()).as_deref(),
            "level_{} must carry id={}",
            i + 1,
            i + 1
        );
    }
}

#[test]
fn i128_and_u128_field_boundary_values_round_trip() {
    use tracing_subscriber::layer::SubscriberExt;

    let recorder = FlightRecorder::new(100);
    let layer = FlightRecorderLayer::new(recorder.clone());
    let subscriber = tracing_subscriber::registry().with(layer);

    let imin = i128::MIN;
    let imax = i128::MAX;
    let umax = u128::MAX;
    let uzero: u128 = 0;
    let imin_s = imin.to_string();
    let imax_s = imax.to_string();
    let umax_s = umax.to_string();
    let uzero_s = uzero.to_string();

    tracing::subscriber::with_default(subscriber, || {
        tracing::info!(
            imin = imin,
            imax = imax,
            umax = umax,
            uzero = uzero,
            "wide integer fields"
        );
    });

    let snap = recorder.snapshot();
    assert_eq!(snap.len(), 1);
    let fields: std::collections::HashMap<&str, &str> = snap[0]
        .fields
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();

    assert_eq!(fields.get("imin"), Some(&imin_s.as_str()));
    assert_eq!(fields.get("imax"), Some(&imax_s.as_str()));
    assert_eq!(fields.get("umax"), Some(&umax_s.as_str()));
    assert_eq!(fields.get("uzero"), Some(&uzero_s.as_str()));
}

#[test]
fn dump_to_file_into_readonly_directory_returns_error() {
    // Permission-denied path: the parent directory is not writable, so file
    // creation must fail and propagate as an io::Error rather than panicking
    // or silently dropping the dump.
    let recorder = FlightRecorder::new(100);
    recorder.push(make_event("perm-test"));

    let dir = tempfile_dir();
    let path = dir.join("no-permission.json");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500))
            .expect("set dir read-only");

        let result = recorder.dump_to_file(&path);
        assert!(
            result.is_err(),
            "dump_to_file into a read-only directory must return an error"
        );
        assert!(
            !path.exists(),
            "no file should be written when the directory is read-only"
        );

        // Restore writability so the temp dir can be cleaned up.
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
            .expect("restore dir writability");
    }

    // On non-Unix we cannot reliably simulate a permission failure, so the
    // assertion is Unix-only. Keep the test green elsewhere by asserting the
    // happy path works.
    #[cfg(not(unix))]
    {
        recorder.dump_to_file(&path).expect("write succeeds");
        assert!(path.exists());
    }
}

// ── Compact vs pretty JSON output ────────────────────────────────────

#[test]
fn dump_to_json_is_compact_and_pretty_variant_indents() {
    let recorder = FlightRecorder::new(100);
    recorder.push(make_event("compact-vs-pretty"));

    let compact = recorder.dump_to_json().unwrap();
    let pretty = recorder.dump_to_json_pretty().unwrap();

    // Compact form has no embedded newlines between tokens.
    assert!(
        !compact.contains('\n'),
        "compact JSON must not contain newlines"
    );
    // Pretty form is indented (multi-line).
    assert!(
        pretty.contains('\n'),
        "pretty JSON must be multi-line/indented"
    );

    // Both decode to the same single event.
    let c: serde_json::Value = serde_json::from_str(&compact).unwrap();
    let p: serde_json::Value = serde_json::from_str(&pretty).unwrap();
    assert_eq!(c.as_array().map_or(0, Vec::len), 1);
    assert_eq!(p.as_array().map_or(0, Vec::len), 1);
}

#[test]
fn envelope_pretty_variant_indents_and_round_trips() {
    let recorder = FlightRecorder::new(100);
    recorder.push(make_event("env-pretty"));

    let compact = recorder.dump_envelope_to_json(None).unwrap();
    let pretty = recorder.dump_envelope_to_json_pretty(None).unwrap();

    assert!(!compact.contains('\n'), "envelope compact is single-line");
    assert!(pretty.contains('\n'), "envelope pretty is indented");

    // Pretty still round-trips into the typed envelope.
    let parsed: FlightRecorderDump = serde_json::from_str(&pretty).unwrap();
    assert_eq!(parsed.event_count, 1);
    assert_eq!(parsed.events[0].message, "env-pretty");
}

// ── Observability hooks (on_dump) ────────────────────────────────────

#[test]
fn on_dump_fires_for_manual_file_dump() {
    let collected: std::sync::Arc<std::sync::Mutex<Vec<DumpEvent>>> =
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let hook = std::sync::Arc::clone(&collected);
    let recorder =
        FlightRecorder::new(100).with_on_dump(move |ev| hook.lock().unwrap().push(ev.clone()));
    recorder.push(make_event("hooked"));

    let dir = tempfile_dir();
    let path = dir.join("manual.json");
    recorder.dump_to_file(&path).unwrap();

    let events = collected.lock().unwrap();
    assert_eq!(
        events.len(),
        1,
        "manual dump should fire the hook exactly once"
    );
    let ev = &events[0];
    assert_eq!(ev.source, DumpSource::Manual);
    assert_eq!(ev.path.as_deref(), Some(path.as_path()));
    assert!(ev.bytes_written > 0, "bytes_written must be positive");
    assert!(
        ev.trigger_reason.is_none(),
        "bare dump_to_file carries no reason"
    );
}

#[test]
fn on_dump_fires_for_trigger_dump_with_trigger_source() {
    use crate::LevelTrigger;
    use tracing_subscriber::layer::SubscriberExt;

    let collected: std::sync::Arc<std::sync::Mutex<Vec<DumpEvent>>> =
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let hook = std::sync::Arc::clone(&collected);
    let dir = tempfile_dir();

    let recorder =
        FlightRecorder::new(100).with_on_dump(move |ev| hook.lock().unwrap().push(ev.clone()));
    let layer = FlightRecorderLayer::new(recorder.clone()).with_dump_on(
        LevelTrigger::error(),
        dir.clone(),
        "incident",
        10,
    );
    let subscriber = tracing_subscriber::registry().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        tracing::error!("boom");
    });

    let events = collected.lock().unwrap();
    assert_eq!(
        events.len(),
        1,
        "trigger dump should fire the hook exactly once"
    );
    let ev = &events[0];
    assert_eq!(ev.source, DumpSource::Trigger);
    assert_eq!(ev.trigger_reason.as_deref(), Some("level>=ERROR"));
    assert!(ev.bytes_written > 0);
    assert!(
        ev.path.is_some(),
        "trigger dump must report a destination path"
    );
}

#[test]
fn on_dump_callback_panic_is_contained() {
    use tracing_subscriber::layer::SubscriberExt;

    let recorder = FlightRecorder::new(100).with_on_dump(|_ev| panic!("misbehaving observer"));
    let dir = tempfile_dir();
    let layer = FlightRecorderLayer::new(recorder.clone()).with_dump_on(
        crate::LevelTrigger::error(),
        dir.clone(),
        "boom",
        10,
    );
    let subscriber = tracing_subscriber::registry().with(layer);

    // A panicking callback must NOT abort the trigger path or corrupt state.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tracing::subscriber::with_default(subscriber, || {
            tracing::error!("triggering a dump whose hook panics");
        });
    }));
    assert!(
        result.is_ok(),
        "a panicking on_dump callback must be contained, not propagated"
    );

    // The recorder must remain usable, and a dump file must still have landed.
    recorder.push(make_event("after-panic"));
    assert_eq!(recorder.len(), 2);
    assert!(
        count_files_matching(&dir, "boom") >= 1,
        "the dump file must still be written despite the hook panic"
    );
}

// ── gzip compression (gzip feature) ──────────────────────────────────

#[cfg(feature = "gzip")]
#[test]
fn dump_to_file_gz_writes_valid_gzip_that_decompresses() {
    use std::io::Read;

    let recorder = FlightRecorder::new(100);
    recorder.push(make_event("gz-test-one"));
    recorder.push(make_event("gz-test-two"));

    let dir = tempfile_dir();
    let path = dir.join("dump.json.gz");
    recorder.dump_to_file_gz(&path).unwrap();

    assert!(path.exists());
    let bytes = std::fs::read(&path).unwrap();
    // gzip magic bytes.
    assert_eq!(
        &bytes[0..2],
        &[0x1f, 0x8b],
        "file must start with gzip magic"
    );

    let mut decoder = flate2::read::GzDecoder::new(&bytes[..]);
    let mut out = String::new();
    decoder.read_to_string(&mut out).unwrap();
    let parsed: Vec<serde_json::Value> = serde_json::from_str(&out).unwrap();
    assert_eq!(parsed.len(), 2);

    // Compression should beat the raw compact JSON for non-trivial input.
    let raw = recorder.dump_to_json().unwrap();
    assert!(
        bytes.len() < raw.len(),
        "gzip output ({} B) should be smaller than raw JSON ({} B)",
        bytes.len(),
        raw.len()
    );
}

#[cfg(feature = "gzip")]
#[test]
fn dump_envelope_to_file_gz_decompresses_to_valid_envelope() {
    use std::io::Read;

    let recorder = FlightRecorder::new(100);
    recorder.push(make_event("env-gz"));
    let dir = tempfile_dir();
    let path = dir.join("envelope.json.gz");
    recorder
        .dump_envelope_to_file_gz(&path, Some("crash"))
        .unwrap();

    let bytes = std::fs::read(&path).unwrap();
    let mut decoder = flate2::read::GzDecoder::new(&bytes[..]);
    let mut out = String::new();
    decoder.read_to_string(&mut out).unwrap();

    let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(
        parsed.is_object(),
        "decompressed envelope must be an object"
    );
    assert_eq!(parsed["schema_version"], 1);
    assert_eq!(parsed["trigger_reason"], "crash");
    assert_eq!(parsed["event_count"], 1);
}

#[cfg(feature = "gzip")]
#[test]
fn on_dump_reports_compressed_bytes_for_gz() {
    let collected: std::sync::Arc<std::sync::Mutex<Vec<DumpEvent>>> =
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let hook = std::sync::Arc::clone(&collected);
    let recorder =
        FlightRecorder::new(100).with_on_dump(move |ev| hook.lock().unwrap().push(ev.clone()));
    recorder.push(make_event("hooked-gz"));

    let dir = tempfile_dir();
    let path = dir.join("hooked.json.gz");
    recorder.dump_to_file_gz(&path).unwrap();

    let events = collected.lock().unwrap();
    assert_eq!(events.len(), 1);
    let ev = &events[0];
    assert_eq!(ev.source, DumpSource::Manual);

    // The reported bytes must equal the actual on-disk (compressed) size.
    let on_disk = std::fs::metadata(&path).unwrap().len();
    assert_eq!(
        ev.bytes_written, on_disk as usize,
        "on_dump must report the compressed byte count"
    );
}

// ── Allocation profiling (on-demand) ─────────────────────────────────
//
// A counting global allocator that wraps the system allocator, used to
// characterize how many heap allocations the `on_event` hot path performs
// per event. It delegates to `System` (identical behaviour, plus a relaxed
// atomic increment per allocation), so it is harmless for the rest of the
// suite.
//
// The measurement test is `#[ignore]`d because a global counter is perturbed
// by parallel test execution; run it on demand:
//
//   cargo test profile_allocations -- --ignored --nocapture

use std::alloc::{GlobalAlloc, Layout, System};

static ALLOC_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

struct CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // Only count a realloc as a new allocation when it grows.
        if new_size > layout.size() {
            ALLOC_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        System.realloc(ptr, layout, new_size)
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        System.alloc_zeroed(layout)
    }
}

#[global_allocator]
static GLOBAL_ALLOC: CountingAllocator = CountingAllocator;

#[test]
#[ignore = "allocation profiling; run with --ignored --nocapture"]
fn profile_allocations_on_event_hot_path() {
    use std::sync::atomic::Ordering;
    use tracing_subscriber::layer::SubscriberExt;

    let recorder = FlightRecorder::new(2000);
    let subscriber =
        tracing_subscriber::registry().with(FlightRecorderLayer::new(recorder.clone()));
    let dispatch = tracing::Dispatch::new(subscriber);

    const N: u64 = 1000;
    tracing::dispatcher::with_default(&dispatch, || {
        // Warm up: the first event can trigger lazy allocations in the
        // registry/subscriber; exclude it from the measurement.
        tracing::info!(warm = true, "warmup");

        let before = ALLOC_COUNT.load(Ordering::Relaxed);
        for i in 0..N {
            tracing::info!(count = i, target = "prof", "processing request");
        }
        let after = ALLOC_COUNT.load(Ordering::Relaxed);

        let total = after.saturating_sub(before);
        let per_event = total / N;
        println!(
            "hot-path allocations: {total} total over {N} events ≈ {per_event} allocs/event \
             (FieldVisitor + span capture + push)"
        );
        // Loose characterization bound: catch gross regressions without flaking.
        assert!(
            per_event < 50,
            "per-event allocation count {per_event} exceeds expected range"
        );
    });
}
