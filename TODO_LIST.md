# TODO List

> Short-term, actionable, bounded work items, verified against the actual code.
> For long-term vision and unrefined ideas, use ROADMAP.md.
> Items are ranked by impact. Status is verified, not assumed.

## Status legend

| Status           | Meaning                                                     |
| ---------------- | ----------------------------------------------------------- |
| 🔴 `TODO`        | Not started. Needs doing.                                   |
| 🟡 `IN_PROGRESS` | Actively being worked on.                                   |
| 🔵 `BLOCKED`     | Cannot proceed, external dependency or decision needed.     |
| 🟢 `DONE`        | Completed. Remove from this list and log in `CHANGELOG.md`. |

## High Impact

_All high-impact items shipped in v0.2.0_ — configurable span capture, `Arc`-shared
span fields, trigger system + once-semantics, dump metadata envelope, and the
memory-footprint test accuracy fix. See `CHANGELOG.md` for details.

## Medium Impact

| Task | Status | Effort | Notes |
|------|--------|--------|-------|
| Make pretty-print opt-in (default to compact JSON) | 🔴 `TODO` | ~30m | `dump_to_json` defaults to pretty. Consider compact default with `dump_to_json_pretty`. Breaking change. |
| Observability hooks — `on_dump` callback | 🔴 `TODO` | ~1h | `DumpEvent` struct (duration, bytes, path, source). Callback on every dump. |
| Compression option (`flate2` behind feature flag) | 🔴 `TODO` | ~1h | Optional gzip compression for dump output. |
| Async/non-blocking capture | 🔴 `TODO` | ~3h | Background dump thread via `std::thread::spawn`, drain on shutdown. v0.3.0 scope. |
| Benchmark hot path with `criterion` | 🔴 `TODO` | ~2h | Push/dump latency benchmarks. No benchmarks exist yet. |
| Profile allocation count | 🔴 `TODO` | ~1h | Use `cargo-dhat` or global allocator counter to verify before/after hot-path improvements. |

## Low Impact

| Task | Status | Effort | Notes |
|------|--------|--------|-------|
| Edge case test: `dump_to_file` with read-only directory | 🔴 `TODO` | ~15m | Permission error handling. |
| Edge case test: `i128`/`u128` field values with edge values | 🔴 `TODO` | ~15m | Min/max boundaries. |
| Edge case test: deeply nested spans (10+ levels) | 🔴 `TODO` | ~15m | Stress the span walking. |
| Fuzz test the redaction logic | 🔴 `TODO` | ~30m | proptest with random field names. |
| `parking_lot::Mutex` | 🔴 `TODO` | ~1h | Reduces lock overhead. v0.4.0 scope. |
| `Arc<CapturedEvent>` in buffer | 🔴 `TODO` | ~2h | Cheap snapshot clones. v0.4.0 scope. |
| `SmallVec` for fields | 🔴 `TODO` | ~1h | Most events have <8 fields. |
| Evaluate `no_std` compatibility | 🔴 `TODO` | ~4h | For embedded use cases. |

---

<!-- Guidance for the builder filling this in:
  - Source of truth is the CODE. Verify each item before adding, many
    documented TODOs are already done.
  - One task per row. If it takes more than ~2 hours, split it into smaller
    tasks.
  - Cite evidence (file:line) so the next person can verify without re-deriving.
  - DONE items should be REMOVED, not kept. Use CHANGELOG.md for history.
  - If a task is vague ("improve X"), refine it into concrete steps or move
    it to ROADMAP.md.
  - Deduplicate by semantic intent, not by text match.
  - For 80/20 impact prioritization, use the pareto-planning skill AFTER
    building the list here.
-->
