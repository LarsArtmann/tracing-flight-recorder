# Roadmap

> Long-term direction and raw ideas. Items here are NOT actionable tasks.
> When an idea is refined into bounded work, it moves to TODO_LIST.md.

## Themes

### 1. Time-windowed capture
The recorder currently evicts by **event count** (`capacity = N`). The Go
inspiration and the README describe capturing "the last N *seconds*" of context,
but there is no time-based dimension. Explore capacity that is expressed as a
duration (e.g. "keep events from the last 60s") instead of, or in addition to, a
count.

Raw ideas:

- Time-based eviction policy alongside count-based
- Hybrid capacity: `max_events` OR `max_age`, whichever fills first
- Report the actual time span covered by the current buffer in metadata

### 2. Performance

The hot path takes a `std::sync::Mutex` lock and clones all fields into owned
`String`s. Span context capture adds a scope walk per event. Property-based
eviction tests and concurrency stress tests are in place, but there are no
benchmarks yet.

Raw ideas:

- Evaluate `parking_lot::Mutex` or a lock-free ring buffer (e.g. `crossbeam-queue`)
- Pre-allocated, reusable field buffers (`SmallVec` for <8 fields) — see `TODO_LIST.md` for why this is currently rejected
- Zero-copy snapshot handle (iterator over the buffer) instead of cloning into a `Vec`
- Optional async channel + background writer so `on_event` never serializes

### 3. Output formats

JSON (pretty + NDJSON), file dumps, retention pruning, and writer streaming
are all shipped. Remaining ideas for diagnostic tool integration:

Raw ideas:

- Chrome Trace Event format (opens in `chrome://tracing`)
- OpenTelemetry export for cross-correlation
- Human-readable pretty-text dump for incident chat paste

### 4. Framework ergonomics

Span context capture is configurable (`with_span_capture`). The trigger system
provides automatic snapshot-on-failure. Manual `dump_to_file` on error still
requires the caller to wire every failure path — explore deeper integrations.

Raw ideas:

- `tower` middleware that dumps the buffer on `Response` error status
- `axum` extractor / `on_response` hook for automatic incident capture
- Panic-hook integration that dumps before the process exits
- Async/non-blocking capture: background dump thread, drain on shutdown
- `FlightRecorderBuilder`: capacity, span capture toggle, redaction patterns, output format
- `parking_lot::Mutex` for reduced lock overhead
- `Arc<CapturedEvent>` in buffer for cheap snapshot clones
- Pre-allocated, reusable field buffers to avoid per-event allocation

### 5. Crates.io publication

Published and automated. v0.1.1 is live on
[crates.io](https://crates.io/crates/tracing-flight-recorder) and
[docs.rs](https://docs.rs/tracing-flight-recorder) (built with the `openapi`
feature). Pushing a `v*.*.*` tag triggers
[`publish.yml`](https://github.com/LarsArtmann/tracing-flight-recorder/blob/master/.github/workflows/publish.yml)
which publishes automatically via the `CARGO_REGISTRY_TOKEN` secret. See
[`docs/RELEASE.md`](RELEASE.md) for the full release runbook.

Raw ideas:

- Add more runnable examples (e.g. tower middleware auto-dump, panic-hook integration)

## Non-goals

Things we are deliberately NOT pursuing and why:

- **Replacing `tracing-subscriber` / `fmt` layer:** This crate complements an
  existing subscriber setup; it does not replace console output or structured
  logging backends.
- **Distributed / OpenTelemetry-native tracing backend:** This is a local,
  in-process, single-node diagnostic tool. Cross-process correlation is out of
  scope (though OTLP *export* of a snapshot may appear under theme 3).
- **Persistent log storage / log rotation daemon:** The buffer is intentionally
  ephemeral and bounded. Long-term retention belongs in a real log collector.
- **GUI viewer:** Snapshots are JSON files for external tooling; a built-in
  viewer would broaden scope beyond this crate.
- **Configurable span context capture:** ~~Span capture is always-on.~~ Now
  configurable via `FlightRecorderLayer::with_span_capture`. Remaining gap: a
  full `FlightRecorderBuilder` unifying capacity, span capture, redaction
  patterns, and output format.
- **`no_std` / embedded support:** Investigated and rejected short-term. The
  crate depends on `chrono` (wall-clock timestamps), `std::sync::Mutex`
  (shared ring buffer), and `std::fs` (file/retention dumps). A `no_std` port
  would need a spin/`critical-section` mutex, a timestamp abstraction, and the
  filesystem API feature-gated out. Not actionable until a concrete embedded
  use case demands it.

---

<!-- Guidance for the builder:
  - NO bounded actionable tasks here. If it has a clear scope and effort
    estimate, it belongs in TODO_LIST.md.
  - NO status indicators on individual items. This is vision, not inventory.
  - Ideas should be raw and unrefined by design.
  - Non-goals are as important as goals: they prevent scope creep.
  - Revisit quarterly to prune stale directions.
-->
