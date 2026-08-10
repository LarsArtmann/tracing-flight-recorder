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

### 2. Hot-path performance

Every captured event takes a `std::sync::Mutex` lock and clones all fields into
owned `String`s under the tracing hot path, and `snapshot()` clones the entire
buffer. For high-throughput services this is a bottleneck worth investigating.

Raw ideas:

- Evaluate `parking_lot::Mutex` or a lock-free ring buffer (e.g. `crossbeam-queue`)
- Pre-allocated, reusable field buffers to avoid per-event allocation
- Zero-copy snapshot handle (iterator over the buffer) instead of cloning into a `Vec`
- Optional async channel + background writer so `on_event` never serializes

### 3. Output formats

JSON is the only output today. Flight-recorder snapshots are most valuable when
they drop straight into existing diagnostic tooling.

Raw ideas:

- Chrome Trace Event format (opens in `chrome://tracing`)
- OpenTelemetry export for cross-correlation
- Newline-delimited JSON for stream ingestion
- Human-readable pretty-text dump for incident chat paste

### 4. Framework ergonomics

Manual `dump_to_file` on error requires the caller to wire every failure path.
Explore integrations that auto-dump on failure conditions.

Raw ideas:

- `tower` middleware that dumps the buffer on `Response` error status
- `axum` extractor / `on_response` hook for automatic incident capture
- Panic-hook integration that dumps before the process exits
- Macro helper: `fr_on_error!(recorder, || { ... })`

### 5. Crates.io publication

v0.1.0 is tagged locally. The remaining step is publishing to crates.io,
which requires a crates.io API token and `cargo publish`. See
[`docs/RELEASE.md`](RELEASE.md) for the full release runbook.

Raw ideas:

- Publish v0.1.0 to crates.io
- Verify `Cargo.toml` metadata renders correctly on crates.io
- Verify docs.rs builds with the `openapi` feature
- Set up `CARGO_REGISTRY_TOKEN` secret for automated publish-on-tag
- Add a minimal `examples/` directory (binary examples beyond doc tests)

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
  viewer would broaden scope beyond a zero-dependency tracing crate.

---

<!-- Guidance for the builder:
  - NO bounded actionable tasks here. If it has a clear scope and effort
    estimate, it belongs in TODO_LIST.md.
  - NO status indicators on individual items. This is vision, not inventory.
  - Ideas should be raw and unrefined by design.
  - Non-goals are as important as goals: they prevent scope creep.
  - Revisit quarterly to prune stale directions.
-->
