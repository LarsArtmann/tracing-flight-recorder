# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

_No changes yet._

## [0.2.0] - 2026-08-11

### Added

- **Span context capture** — events fired inside spans now record their full span hierarchy (names + key-value fields, root-first) via the new `CapturedEvent.spans: Vec<SpanContext>` field. The layer implements `on_new_span` and `on_record` to store span fields as extension data through `LookupSpan`, then `on_event` walks the scope to build the hierarchy. This was the #1 development priority identified in the comparative review (`src/capture.rs`, `src/layer.rs`)
- **`SpanContext` struct** — public type re-exported from the crate root: `name: String` + `fields: Vec<(String, String)>` (`src/capture.rs`)
- **`dump_to_writer`** — streams pretty-printed JSON to any `impl Write` without buffering the full string (`src/layer.rs`)
- **`dump_to_json_lines`** — NDJSON output (one compact JSON object per line) for stream ingestion into log pipelines (`src/layer.rs`)
- **`dump_to_writer_lines`** — NDJSON streaming to any `impl Write` without buffering (`src/layer.rs`)
- **Expanded redaction patterns** — added `authorization`, `auth`, `bearer`, `cookie`, `session_id`, `access_code` to the sensitive-field pattern list (14 total patterns, case-insensitive substring match) (`src/capture.rs`)
- **`examples/span_context.rs`** — runnable example demonstrating span context capture with nested spans (`examples/`)
- **README "Span Context Capture" section** with code example showing nested spans and the resulting `spans` field
- **Trigger system** — automatic snapshot-on-failure via the `Trigger` trait, `LevelTrigger` (fires at/above a severity), and `OnceTrigger` (fires at most once until `reset`). Wire one in with `FlightRecorderLayer::with_dump_on(trigger, dir, prefix, max_files)`; when the trigger fires the buffer is written automatically as an envelope with the trigger's name as `trigger_reason`. This is the central value proposition of a flight recorder: zero I/O until something goes wrong, then a self-describing snapshot is persisted with no caller wiring (`src/trigger.rs`, `src/layer.rs`)
- **Dump metadata envelope** — `FlightRecorderDump` struct wrapping events with `schema_version`, `captured_at`, `crate_version`, `event_count`, and `trigger_reason`. Available via `dump_envelope`, `dump_envelope_to_json`, `dump_envelope_to_file`, and `dump_with_retention_envelope`. Existing array-only dump methods are unchanged for backward compatibility (`src/capture.rs`, `src/layer.rs`)
- **`DUMP_SCHEMA_VERSION`** constant (currently `1`) so envelope consumers can branch on a stable integer (`src/capture.rs`)
- **Configurable span context capture** — `FlightRecorderLayer::with_span_capture(recorder, bool)` disables span field storage and per-event scope walking for high-throughput pipelines that don't need request context. `new()` defaults to capture-on as before (`src/layer.rs`)
- **`Arc<Vec<…>>` span field sharing** — `SpanContext.fields` is now `Arc<Vec<(String, String)>>`, so all events inside the same span share one allocation (O(1) reference bump per event instead of an O(fields) deep copy). Updates via `span.record()` use clone-on-write so already-captured events keep their original field snapshot. Enabled by serde's `rc` feature and utoipa's `rc_schema` feature; serializes as a plain JSON array (`src/capture.rs`, `src/layer.rs`)

### Changed

- **BREAKING: `CapturedEvent` has a new required field** — `spans: Vec<SpanContext>`. Code that constructs `CapturedEvent` manually must add `spans: Vec::new()` (or populate it). Events captured through the layer are populated automatically
- **BREAKING: `FlightRecorderLayer` now requires `S: Subscriber + for<'lookup> LookupSpan<'lookup>`** — the `Layer` impl bound was tightened to enable span context capture. Subscribers built via `tracing_subscriber::registry()` already implement `LookupSpan`, so most users are unaffected
- **BREAKING: `CapturedEvent.level` is now `Cow<'static, str>`** instead of `String` — eliminates one heap allocation per event since the 5 known `tracing::Level` variants are stored as `Cow::Borrowed` (zero-copy). Serializes and deserializes identically
- **BREAKING: `SpanContext.fields` is now `Arc<Vec<(String, String)>>`** instead of `Vec<(String, String)>` — serializes identically (serde `rc`) and auto-derefs to `Vec`/slice, so most reads (`.iter()`, `.len()`, `.is_empty()`) compile unchanged. Code that moves the `Vec` out of the field must add `.as_ref()` or dereference
- **`push` is now `pub(crate)`** — prevents external callers from injecting synthetic events into the diagnostic record
- **`FieldVisitor` removed from public re-exports** — it remains `pub` in the private `capture` module (crate-internal) but is no longer part of the public API surface
- **Zero-allocation redaction matching** — `is_sensitive_field` now uses byte-level `windows()` + `eq_ignore_ascii_case` instead of `to_lowercase()`, eliminating one heap allocation per field name per event
- **Per-sensitive-field allocation eliminated** — `record_common` now takes `&str` instead of `String`, so sensitive `record_str` fields skip the value formatting entirely (was: format value → discard → format `"[REDACTED]"`; now: just format `"[REDACTED]"`)
- **`REDACTED` constant** — extracted `"[REDACTED]"` literal to `const REDACTED: &str` for clarity
- **`max_files = 0` means unlimited** — `dump_with_retention(_, _, 0)` no longer deletes its own dump. Matches the Go sibling project's convention
- README dependency claim corrected: "Zero non-tracing dependencies" → "Minimal dependencies — tracing ecosystem + serde/chrono for serialization"
- Memory footprint test now measures true deep size (every `String`/`Vec` **capacity**, not just `len()`), revealing the 1000-event buffer is ~385 KB (previously reported ~237 KB — a 62% undercount)

### Fixed

- **capacity=0 retained 1 event** — `FlightRecorder::new(0)` silently stored 1 event because `pop_front()` on an empty deque is a no-op, then `push_back` ran anyway. Now `push()` returns early when `capacity == 0` (`src/layer.rs:41`)
- **`dump_with_retention(_, _, 0)` deleted its own dump** — `cleanup_old_snapshots` with `max_files=0` computed `excess = 1` and deleted the snapshot that was just written (silent data loss). Now treats `max_files=0` as "unlimited" — no cleanup performed (`src/layer.rs:227`)

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

[Unreleased]: https://github.com/LarsArtmann/tracing-flight-recorder/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/LarsArtmann/tracing-flight-recorder/releases/tag/v0.2.0
[0.1.1]: https://github.com/LarsArtmann/tracing-flight-recorder/releases/tag/v0.1.1
[0.1.0]: https://github.com/LarsArtmann/tracing-flight-recorder/releases/tag/v0.1.0
