# AGENTS.md

In-memory ring-buffer flight recorder for `tracing` events. Pure Rust **library crate** (no binary, no `flake.nix`) — Cargo is the only build tool. Inspired by Go 1.25's `trace.FlightRecorder`: continuously buffers DEBUG/TRACE events and snapshots them on failure.

## Commands

```sh
cargo build                         # build the library
cargo build --all-features          # build including the `openapi` feature (enables utoipa)
cargo test                          # run all unit + doc tests
cargo test --all-features           # canonical gate: 64 unit + 6 doctests (includes openapi + proptest)
cargo clippy --all-features --all-targets -- -D warnings   # strict lint gate
cargo fmt --check                   # format check
cargo doc --all-features --no-deps  # generate docs
```

Always run clippy with `--all-features` so the `openapi`-gated code paths are checked. MSRV is **1.86**, edition 2021. `proptest` is a dev-dependency for property-based tests.

## Release Infrastructure

- **`docs/RELEASE.md`** — full release runbook (pre-release checklist, verification gate, semver rules, step-by-step cutting, post-release verification, rollback).
- **`release.toml`** — `cargo-release` config. `push = false` (don't auto-push), `publish = true`. Run `cargo release version <x.y.z> --execute` to cut a release.
- **`.github/workflows/publish.yml`** — automated crates.io publishing on `v*.*.*` tag push. Verifies tag matches `Cargo.toml` version, idempotency guard against re-publish. `CARGO_REGISTRY_TOKEN` secret is configured. Pushing a tag publishes automatically.
- **`deny.toml`** — `cargo-deny` config (advisories, licenses, bans, sources). Run with `cargo deny check`.
- **`[package.metadata.docs.rs]`** in `Cargo.toml` — builds docs.rs with the `openapi` feature so `ToSchema` appears on the published docs page.
- **GitHub repo**: `git@github.com:LarsArtmann/tracing-flight-recorder.git` (public). Topics: tracing, flight-recorder, diagnostics, ring-buffer, debugging, rust, tracing-subscriber.

## Strict Clippy — Read Before Writing Any Code

This crate enforces an unusually harsh clippy configuration (`[lints.clippy]` in `Cargo.toml`). In **non-test** code the following are `deny` and will break the build:

- `unwrap_used`, `expect_used` — no `.unwrap()` / `.expect()`
- `indexing_slicing` — no `slice[i]`; use `get()` / `get_mut()`
- `string_slice` — no `s[a..b]` on strings
- `arithmetic_side_effects` — no `+`/`-`/`*` without checked/wrapping/saturating ops
- `as_conversions` — no `as` casts; use `From`/`Into`/`try_from`
- `panic`, `unreachable`, `todo`, `unimplemented`, `exit`, `panic_in_result_fn`
- `pedantic` and `nursery` lints (entire groups, `deny`)

**How the crate itself complies** — patterns to copy:
- Mutex locks recover from poison explicitly: `lock().unwrap_or_else(PoisonError::into_inner)`. This is intentional (poison shouldn't kill the recorder); don't "fix" it to propagate.
- Fallible ops return `Result` and propagate with `?` (see `dump_to_file`, `dump_to_json`).
- Formatting uses `write!(buf, ...)` with `let _ =` to discard the infallible `Result` for `String`.

**Test code is exempt.** Two mechanisms relax these lints in tests — match the existing pattern when adding tests:
- `src/lib.rs` carries `#![cfg_attr(test, allow(...))]` covering the whole crate for test builds.
- `src/layer.rs` adds a local `#[allow(...)]` on its `#[cfg(test)] mod tests`.
- In tests you may freely use `.unwrap()` / `.expect()` / indexing.

## Code Organization

Five source files, all under `src/`:

| File             | Responsibility                                                                                          |
| ---------------- | ------------------------------------------------------------------------------------------------------- |
| `lib.rs`         | Crate docs, lint configuration (crate-level `cfg_attr(test, allow)`), module wiring, public re-exports, `DEFAULT_CAPACITY` constant. |
| `capture.rs`     | `CapturedEvent` + `SpanContext` + `FlightRecorderDump` structs (serializable diagnostic types), `DUMP_SCHEMA_VERSION`, `FieldVisitor` (`tracing::field::Visit` impl), secret redaction (`is_sensitive_field`, `contains_ascii_case_insensitive`), `level_to_string`. Has inline `#[cfg(test)] mod tests`. |
| `layer.rs`       | `FlightRecorder` (the `Arc<Mutex<VecDeque>>` ring buffer + dump methods incl. envelope variants) and `FlightRecorderLayer` (`tracing_subscriber::Layer` impl with `on_new_span`/`on_record`/`on_event` for span context capture + trigger-driven auto-dump). `CapturedSpanFields` extension wrapper (`Arc<Vec<…>>`, clone-on-write), `capture_span_context` helper, `DumpConfig` + `fire_dump` for the trigger system. Wires its tests in via `#[path = "layer_tests.rs"]`. |
| `trigger.rs`     | `Trigger` trait (`should_dump` + `name`), `LevelTrigger` (severity threshold), `OnceTrigger` (fires once until `reset`). Has inline `#[cfg(test)] mod tests`. |
| `layer_tests.rs` | Tests for `layer.rs`. Lives in its own file (not inline) — included by `#[cfg(test)] #[path = "layer_tests.rs"] mod tests;` at the bottom of `layer.rs`. Use `use super::*;` and `use crate::capture::CapturedEvent;`. |

**Data flow:** `tracing` event → `FlightRecorderLayer::on_event` → `CapturedEvent::from_event` (runs `FieldVisitor`, which redacts sensitive fields) + `capture_span_context` (walks `ctx.event_scope().from_root()`, reads `CapturedSpanFields` extensions stored by `on_new_span`/`on_record`, shares span fields via `Arc::clone`) → `FlightRecorder::push` (evicts oldest if at capacity) → `VecDeque` ring buffer → (if a `Trigger` is attached and fires) `fire_dump` → `dump_with_retention_envelope`. Snapshot/dump methods clone and serialize on demand.

## Conventions

- **Public API**: `FlightRecorder`, `FlightRecorderLayer`, `CapturedEvent`, `SpanContext`, `FlightRecorderDump`, `Trigger`, `LevelTrigger`, `OnceTrigger`, `DUMP_SCHEMA_VERSION`. All re-exported from `lib.rs`. (`FieldVisitor` and `push` are `pub(crate)` — internal implementation details, not for external use.)
- **`FlightRecorder` is `Clone` and cheap** — all clones share one `Arc<Mutex<VecDeque>>`. Pattern: create one, clone it into the layer, keep the original for dumping.
- **Docs use `///` + module-level `//!`**. Doc examples are `no_run`. Public items have `# Errors` / `# Panics` sections where relevant.
- **`#[must_use]`** on all constructors and accessors returning owned data.
- **Span context capture**: `FlightRecorderLayer` implements `on_new_span` + `on_record` to store span fields as a `CapturedSpanFields` extension on span data (via `LookupSpan`). In `on_event`, `capture_span_context` walks `ctx.event_scope(event).from_root()` to populate `CapturedEvent.spans` (root-first ordering). Span fields are `Arc<Vec<(String,String)>>` so events in the same span share one allocation (O(1) clone); `on_record` uses `Arc::make_mut` for clone-on-write so already-captured events keep their snapshot. The `Layer` impl requires `S: Subscriber + for<'lookup> LookupSpan<'lookup>`. Capture is on by default; disable with `with_span_capture(recorder, false)`. Tests: `event_inside_single_span_captures_span_context`, `event_inside_nested_spans_captures_full_hierarchy`, `sensitive_span_fields_are_redacted`, `span_fields_updated_via_record_are_captured`, `events_in_same_span_share_span_fields_allocation`, `span_fields_updated_via_record_do_not_mutate_already_captured_events`.
- **Secret redaction is automatic and over-broad**: any field whose name contains `token`, `password`, `secret`, `api_key`/`apikey`, `credential`, `passphrase`, `private_key`, `authorization`, `auth`, `bearer`, `cookie`, `session_id`, or `access_code` (case-insensitive, substring match via zero-allocation ASCII comparison in `contains_ascii_case_insensitive`) is stored as `[REDACTED]`. Applies to both event fields and span fields. Over-redaction is intentional — do not narrow it without strong reason.
- **Feature flag `openapi`**: enables `dep:utoipa` (with `rc_schema` so `Arc` fields are schema-transparent) and derives `utoipa::ToSchema` on `CapturedEvent`, `SpanContext`, and `FlightRecorderDump` behind `#[cfg_attr(feature = "openapi", derive(...))]`. When adding fields to any struct, no extra work is needed — the derive picks them up automatically under the feature. Paired with serde's `rc` feature so `Arc<Vec<…>>` serializes as a plain array.

## Critical Gotcha: Per-Layer Filtering

**This is the central design insight of the crate and the #1 integration pitfall.** A *global* `EnvFilter` on the subscriber drops DEBUG/TRACE events before they reach any layer — defeating the entire purpose of the flight recorder. Consumers **must** give `FlightRecorderLayer` its own broader per-layer filter (e.g. `EnvFilter::new("my_app=debug,warn")`) and a narrower filter to the console `fmt` layer.

There is a dedicated regression test guarding this: `flight_recorder_sees_events_blocked_by_other_layer_filter` in `layer_tests.rs`. When changing the layer, ensure that test still passes.

## Testing Approach

- **End-to-end tests install a real subscriber** via `tracing::subscriber::with_default(subscriber, || { ... })` rather than mocking — emit real `tracing::debug!`/`info!`/`warn!` events and assert they land in the recorder. Prefer this style for new layer/pipeline tests.
- **Temp files**: no `tempfile` crate dependency. Tests build a unique dir from `std::env::temp_dir()` + PID + nanos (`tempfile_dir()` helper in `layer_tests.rs`). Reuse that helper rather than adding a dependency.
- **Ring-buffer edge cases** (capacity 1, eviction order, clone-sharing) have explicit unit tests — keep them green when touching `push`/`snapshot`.
- **Property tests** (`proptest`) verify the eviction invariant across random capacity/event-count combinations.
- **Concurrency tests** stress the `Arc<Mutex<VecDeque>>` under multi-thread contention (8 threads × 100 events).
- **Memory footprint test** measures deep bytes (every `String`/`Vec` capacity, not just `len()`) of a 1000-event buffer (~385 KB) and asserts it stays within the README-claimed ~200-500 KB range.
- **Trigger system**: `src/trigger.rs` defines the `Trigger` trait + `LevelTrigger`/`OnceTrigger`; `FlightRecorderLayer::with_dump_on(trigger, dir, prefix, max_files)` attaches automatic snapshot-on-failure. The dump fires synchronously in `on_event` (best-effort, errors discarded) and writes a `FlightRecorderDump` envelope with the trigger's `name()` as `trigger_reason`. The `OnceTrigger` consumes its token in `should_dump` (before the dump), so a failed dump does not retry — document this to users.
- **Collision guard** logic is extracted into `resolve_collision_path` (`layer.rs`) with an injectable `COLLISION_LIMIT` (9999) so the upper bound is unit-tested without creating thousands of files. Tests cover: same-second non-overwrite, limit-exceeded error, first-free-slot, primary-when-free.
