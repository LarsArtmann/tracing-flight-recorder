# TODO List

> Short-term, actionable, bounded work items, verified against the actual code.
> For long-term vision and unrefined ideas, use ROADMAP.md.
> Items are ranked by impact. Status is verified, not assumed.

## Status legend

| Status           | Meaning                                                     |
| ---------------- | ----------------------------------------------------------- |
| 🔴 `TODO`        | Not started. Needs doing.                                   |
| 🟡 `IN_PROGRESS` | Actively being worked on.                                   |
| 🔵 `BLOCKED`/`DEFERRED` | Cannot proceed now — external dependency, decision, or deliberately postponed to a future milestone. |
| 🔴 `REJECTED`    | Investigated and decided against (with reasoning in Notes). |
| 🟢 `DONE`        | Completed. Remove from this list and log in `CHANGELOG.md`. |

## High Impact

_All high-impact items shipped in v0.2.0_ — configurable span capture, `Arc`-shared
span fields, trigger system + once-semantics, dump metadata envelope, and the
memory-footprint test accuracy fix. See `CHANGELOG.md` for details.

## Medium Impact

_Recently shipped (will be in v0.3.0, see `CHANGELOG.md`):_ pretty-print opt-in
(compact default + `_pretty` variants), observability `on_dump` hooks, gzip
compression (`gzip` feature), criterion benchmarks, and allocation-count
profiling.

| Task | Status | Effort | Notes |
|------|--------|--------|-------|
| Async/non-blocking capture | 🔵 `DEFERRED` | ~3h | Background dump thread via `std::thread::spawn`, drain on shutdown. Deferred to v0.3.0 scope — it is a non-trivial lifecycle change (join/drain semantics, backpressure) that deserves its own release focus. |

## Low Impact

_Recently shipped:_ edge-case tests (`dump_to_file` read-only dir, `i128`/`u128`
boundaries, 12-deep nested spans), redaction fuzz test (proptest vs reference
impl). See `CHANGELOG.md`.

| Task | Status | Effort | Notes |
|------|--------|--------|-------|
| `parking_lot::Mutex` | 🔵 `DEFERRED` | ~1h | Reduces lock overhead. Deferred to v0.4.0 scope to batch the lock-related perf work. |
| `Arc<CapturedEvent>` in buffer | 🔵 `DEFERRED` | ~2h | Cheap snapshot clones. Deferred to v0.4.0 scope alongside `parking_lot`. |
| `SmallVec` for fields | 🔴 `REJECTED` (for now) | ~1h | Most events have <8 fields, so an inline buffer avoids one heap alloc/event. **But** changing `CapturedEvent.fields` from `Vec` to `SmallVec` is a breaking public-type change AND breaks the `utoipa::ToSchema` derive on `CapturedEvent` under `openapi` (no built-in SmallVec schema). Revisit only if paired with a custom schema + a major version. Allocation profiling (now ~9 allocs/event) should guide whether this is worth the churn. |
| Evaluate `no_std` compatibility | 🔴 `REJECTED` (for now) | ~4h | Not feasible short-term: the crate depends on `chrono` (wall-clock timestamps), `std::sync::Mutex` (shared ring buffer), and `std::fs` (file dumps). A `no_std` port would require `critical-section`/spin-lock mutex, a timestamp abstraction, and stripping the file/retention API behind a feature. Documented as a long-term direction in `ROADMAP.md`, not an actionable task. |

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
