# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

_Nothing yet._

## [0.3.0] - 2026-08-12

_First release since v0.1.1 — v0.2.0 was never tagged, so both batches of
work are combined here. 8 breaking changes total._

### Added

- **Span context capture** — events fired inside spans now record their full span hierarchy (names + key-value fields, root-first) via the new `CapturedEvent.spans: Vec<SpanContext>` field. The layer implements `on_new_span` and `on_record` to store span fields as extension data through `LookupSpan`, then `on_event` walks the scope to build the hierarchy (`src/capture.rs`, `src/layer.rs`)
- **`SpanContext` struct** — public type re-exported from the crate root: `name: String` + `fields: Arc<Vec<(String, String)>>` (`src/capture.rs`)
- **Trigger system** — automatic snapshot-on-failure via the `Trigger` trait, `LevelTrigger` (fires at/above a severity), and `OnceTrigger` (fires at most once until `reset`). Wire one in with `FlightRecorderLayer::with_dump_on(trigger, dir, prefix, max_files)`; when the trigger fires the buffer is written automatically as an envelope with the trigger's name as `trigger_reason` (`src/trigger.rs`, `src/layer.rs`)
- **Dump metadata envelope** — `FlightRecorderDump` struct wrapping events with `schema_version`, `captured_at`, `crate_version`, `event_count`, and `trigger_reason`. Available via `dump_envelope`, `dump_envelope_to_json`, `dump_envelope_to_file`, `dump_envelope_to_writer`, and `dump_with_retention_envelope`. Existing array-only dump methods remain for backward compatibility (`src/capture.rs`, `src/layer.rs`)
- **`DUMP_SCHEMA_VERSION`** constant (currently `1`) so envelope consumers can branch on a stable integer (`src/capture.rs`)
- **Observability hooks** — `DumpEvent` (path, `bytes_written`, `duration`, `trigger_reason`, `source`, `success`, `error`) and `DumpSource` (`Manual` / `Trigger`). Register a callback with `FlightRecorder::with_on_dump`; it fires after every dump that persists to a file. The callback is best-effort: a panicking observer is contained via `catch_unwind` (`src/layer.rs`, `src/capture.rs`)
- **`DumpEvent.success` and `DumpEvent.error`** — the `on_dump` callback receives `success: bool` and `error: Option<String>`. Trigger dumps that fail (disk full, permission denied) fire the callback with `success: false` and a human-readable error so the host can alert on the missed capture (`src/capture.rs`, `src/layer.rs`)
- **`Debug` for `FlightRecorderLayer`** — operators can now `dbg!()` the layer to inspect trigger state, dump directory, and capture flag (`src/layer.rs`)
- **Gzip compression** — optional `gzip` feature (behind `dep:flate2`) adds `dump_to_file_gz` and `dump_envelope_to_file_gz`, writing snapshots 5-10× smaller. The `on_dump` callback reports the *compressed* byte count (`src/layer.rs`)
- **Configurable span context capture** — `FlightRecorderLayer::with_span_capture(recorder, bool)` disables span field storage for high-throughput pipelines. `new()` defaults to capture-on (`src/layer.rs`)
- **`Arc<Vec<…>>` span field sharing** — `SpanContext.fields` is `Arc<Vec<(String, String)>>`, so all events inside the same span share one allocation. Updates via `span.record()` use clone-on-write (`src/capture.rs`, `src/layer.rs`)
- **`dump_to_writer`** — streams JSON to any `impl Write` (`src/layer.rs`)
- **`dump_to_json_lines`** — NDJSON output (one compact JSON object per line) for stream ingestion (`src/layer.rs`)
- **`dump_to_writer_lines`** — NDJSON streaming to any `impl Write` (`src/layer.rs`)
- **`dump_envelope_to_writer` / `dump_envelope_to_writer_pretty`** — streams the envelope as compact or indented JSON to any `impl Write` (`src/layer.rs`)
- **Expanded redaction patterns** — added `authorization`, `auth`, `bearer`, `cookie`, `session_id`, `access_code` to the sensitive-field pattern list (14 total, case-insensitive substring match) (`src/capture.rs`)
- **Criterion benchmarks** — `benches/push_dump.rs` covers the `on_event` capture path, `snapshot`, and `dump_to_json` at varying buffer sizes
- **Allocation-count profiling** — an `#[ignore]`d test backed by a counting global allocator characterizes the `on_event` hot path at ~9 allocations/event
- **Runnable examples** — `span_context.rs`, `compression.rs`, `observability.rs` added to the existing set (`examples/`)
- **README "Span Context Capture" section** with a code example showing nested spans and the resulting `spans` field
- **Edge-case & fuzz tests** — 12-deep nested span hierarchy, `i128`/`u128` min/max, read-only directory dump, and a 512-case proptest cross-validating the zero-allocation redaction matcher

### Changed

- **BREAKING: `CapturedEvent` has a new required field** — `spans: Vec<SpanContext>`. Code that constructs `CapturedEvent` manually must add `spans: Vec::new()`. Events captured through the layer are populated automatically
- **BREAKING: `FlightRecorderLayer` now requires `S: Subscriber + for<'lookup> LookupSpan<'lookup>`** — enables span context capture. Subscribers built via `tracing_subscriber::registry()` already implement `LookupSpan`
- **BREAKING: `CapturedEvent.level` is now `Cow<'static, str>`** instead of `String` — eliminates one heap allocation per event. Serializes identically
- **BREAKING: `SpanContext.fields` is now `Arc<Vec<(String, String)>>`** — serializes identically (serde `rc`); auto-derefs so most reads compile unchanged
- **BREAKING: `Trigger` trait now requires `std::fmt::Debug`** — enables `Debug` for `FlightRecorderLayer`. Built-in triggers derive `Debug`. Custom triggers must add `#[derive(Debug)]` or a manual impl (`src/trigger.rs`)
- **BREAKING: `DumpEvent` has two new required fields** — `success: bool` and `error: Option<String>`. Code that constructs `DumpEvent` manually must add these fields (`src/capture.rs`)
- **BREAKING: dump methods now default to compact JSON.** `dump_to_json`, `dump_to_writer`, `dump_to_file`, `dump_envelope_to_json`, and `dump_envelope_to_file` emit compact output. New `_pretty` companions provide indented output. Compact is the better default because snapshots are frequently persisted automatically (trigger dumps, retention) where size matters
- **BREAKING: retention dumps are compact** — `dump_with_retention` and `dump_with_retention_envelope` write compact JSON (consistent with the new default)
- **`push` is now `pub(crate)`** — prevents external callers from injecting synthetic events
- **`FieldVisitor` removed from public re-exports** — remains `pub` in the internal `capture` module but is not part of the public API surface
- **Zero-allocation redaction matching** — `is_sensitive_field` uses byte-level `windows()` + `eq_ignore_ascii_case` instead of `to_lowercase()`, eliminating one allocation per field name per event
- **Per-sensitive-field allocation eliminated** — `record_common` takes `&str` instead of `String`, so sensitive fields skip value formatting
- **`REDACTED` constant** — extracted `"[REDACTED]"` literal to `const REDACTED: &str`
- **`max_files = 0` means unlimited** — `dump_with_retention(_, _, 0)` no longer deletes its own dump. Matches the Go sibling project's convention
- README dependency claim corrected: "Zero non-tracing dependencies" → "Minimal dependencies — tracing ecosystem + serde/chrono for serialization"
- Memory footprint test now measures true deep size (every `String`/`Vec` capacity, not just `len()`), revealing ~385 KB for 1000 events

### Fixed

- **`OnceTrigger` race condition** — under concurrent error bursts, the non-atomic `load` → `store` pattern allowed multiple dumps. Now uses `compare_exchange(false, true, AcqRel, Acquire)` for true atomic test-and-set: exactly one dump regardless of thread scheduling (`src/trigger.rs`)
- **`fire_dump` silently swallowed errors** — `on_event` discarded I/O errors from trigger dumps via `let _result =`. Now `fire_dump` fires the `on_dump` callback with `success: false` and a human-readable error (`src/layer.rs`)
- **capacity=0 retained 1 event** — `FlightRecorder::new(0)` silently stored 1 event because `pop_front()` on an empty deque is a no-op. Now `push()` returns early when `capacity == 0` (`FlightRecorder::push`)
- **`dump_with_retention(_, _, 0)` deleted its own dump** — `cleanup_old_snapshots` with `max_files=0` computed `excess = 1` and deleted the snapshot that was just written. Now treats `max_files=0` as "unlimited" — no cleanup performed (`cleanup_old_snapshots`)

## [0.1.1] - 2026-08-10

### Changed

- Excluded `release.toml` (maintainer-only `cargo-release` config) from the published crate — 18 → 17 files, 86.1KiB → 85.4KiB
- Bumped `actions/checkout` from v4 to v7 across all CI workflows to silence Node.js 20 deprecation warnings
- Corrected v0.1.0 CHANGELOG entries: accurate package size, correct CI job reference, removed internal exclude-path details

## [0.1.0] - 2026-08-10

### Added

- Bounded in-memory ring buffer (`FlightRecorder`) with oldest-first eviction at capacity (`src/layer.rs`)
- `FlightRecorderLayer` implementing `tracing_subscriber::Layer` that feeds every event into the recorder (`src/layer.rs`)
- `CapturedEvent` serializable struct (timestamp, level, target, message, fields) and `FieldVisitor` capturing all `tracing` field value types (`src/capture.rs`)
- Automatic secret redaction: fields named `token`, `password`, `secret`, `api_key`/`apikey`, `credential`, `passphrase`, `private_key` are stored as `[REDACTED]` (`src/capture.rs:91`)
- JSON dump (`dump_to_json`), file dump with parent-dir creation (`dump_to_file`), and timestamped dump with file-count retention (`dump_with_retention`) (`src/layer.rs`)
- Shared-buffer `Clone` semantics: all `FlightRecorder` clones share one `Arc<Mutex<VecDeque>>`
- Poison-safe mutex locking: recovers via `PoisonError::into_inner` instead of poisoning the whole recorder
- `openapi` cargo feature deriving `utoipa::ToSchema` on `CapturedEvent` (`src/capture.rs:13`)
- Strict clippy gate in `Cargo.toml`: `pedantic` + `nursery` denied, plus `unwrap_used`, `indexing_slicing`, `as_conversions`, `arithmetic_side_effects`
- End-to-end test suite using real `tracing::subscriber::with_default` subscribers
- GitHub Actions CI pipeline: fmt check, clippy (all features), test (all features), MSRV 1.86 verification, doc build (`.github/workflows/ci.yml`)
- Three runnable examples: `minimal_dump`, `per_layer_filter`, `retention` (`examples/`)
- Domain language glossary documenting 20 terms across 5 categories (`docs/DOMAIN_LANGUAGE.md`)
- README doctests wired via `#[cfg(doctest)]` — Quick Start and Retention code blocks now compile-tested (`src/lib.rs`)
- OpenAPI schema integration test asserting all `CapturedEvent` fields appear in generated OpenAPI JSON (`src/capture.rs`)
- Property-based eviction invariant test (proptest, 256 cases): random capacity × random event count always satisfies `len == min(events, capacity)`
- Multi-thread stress test: 8 threads × 100 events against a shared `FlightRecorder` — no corruption, exact capacity bound
- Poison-recovery test: recorder remains usable after a thread panics while holding the mutex lock
- Unicode field name redaction test: documents that redaction is ASCII-substring-only (`café_token` → caught; `pässwörd` → not caught)
- Nested-directory dump test: `dump_to_file` creates deeply nested parent directories
- Non-JSON retention pruning test: `.txt` and `.yaml` files survive retention cleanup
- Memory footprint measurement test: 1000 realistic events ≈ 237 KB (asserts < 1 MB ceiling)
- Collision-limit guard tests for extracted `resolve_collision_path` function (error at limit, first-free-slot, primary-when-free)
- `CONTRIBUTING.md` with design philosophy, ASCII data-flow diagram, and PR checklist
- `cargo publish --dry-run` CI job to catch packaging regressions on every push/PR
- `cargo audit` + `cargo deny` CI job for supply-chain security (RustSec advisories, license compliance, source bans)
- Dependabot configuration: weekly updates for cargo and github-actions ecosystems

### Changed

- Bumped `utoipa` dependency to v5 with `chrono` feature support (`8c8902b`)
- Replaced leaked `monitor365` project name with neutral `my_app` in doc comments and README
- Purged 680 committed `target/` build artifacts from git history (71MB → 188KB `.git`)
- Softened timing claim in `DEFAULT_CAPACITY` docs: "30-60 seconds" → honest "20-100 seconds at 10-50 events/sec" range
- Extracted collision-resolution logic into testable `resolve_collision_path` function with injectable `COLLISION_LIMIT` (9999) constant
- Examples now write to `std::env::temp_dir()` instead of repository root
- Tightened `exclude` list to keep internal development files (status reports, planning docs, AGENTS.md, etc.) out of the published crate (150.9KiB → 86.1KiB, 23 → 18 files)

### Fixed

- Same-second filename collision in `dump_with_retention`: two dumps within one second no longer silently overwrite; counter suffix (`-1`, `-2`, ...) is appended (`src/layer.rs:134`)

## Notes

- MSRV: 1.86, edition 2021.

[Unreleased]: https://github.com/LarsArtmann/tracing-flight-recorder/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/LarsArtmann/tracing-flight-recorder/releases/tag/v0.3.0
[0.1.1]: https://github.com/LarsArtmann/tracing-flight-recorder/releases/tag/v0.1.1
[0.1.0]: https://github.com/LarsArtmann/tracing-flight-recorder/releases/tag/v0.1.0
