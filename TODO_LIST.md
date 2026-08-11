# TODO List

> Short-term, actionable, bounded work items, verified against the actual code.
> For long-term vision and unrefined ideas, see `ROADMAP.md`.
> Items are ranked by impact. Status is verified, not assumed.

## Status legend

| Status           | Meaning                                                     |
| ---------------- | ----------------------------------------------------------- |
| 🔴 `TODO`        | Not started. Needs doing.                                   |
| 🟡 `IN_PROGRESS` | Actively being worked on.                                   |
| 🔵 `DEFERRED`    | Deliberately postponed to a future milestone (with reasoning). |
| 🔴 `REJECTED`    | Investigated and decided against (with reasoning in Notes). |

> `DONE` items are removed from this list and logged in `CHANGELOG.md`.

## Release

| Task | Status | Effort | Notes |
|------|--------|--------|-------|
| Tag and publish v0.2.0 + v0.3.0 | 🔴 `TODO` | ~1h | v0.1.1 is the latest published version. `master` has two releases' worth of untagged changes: v0.2.0 (span context, triggers, envelope, `Arc` span fields — 4 breaking changes) and v0.3.0 (gzip, `on_dump`, compact-default, benchmarks — 1 breaking change). `Cargo.toml` is at `0.2.0`. Follow `docs/RELEASE.md` checklist: bump version, verify `cargo publish --dry-run --all-features`, tag, push. |

## High Impact — Correctness & Reliability

| Task | Status | Effort | Notes |
|------|--------|--------|-------|
| Fix `OnceTrigger` race condition | 🔴 `TODO` | ~30m | `should_dump` uses non-atomic load-check-store (`load(Acquire)` → `store(Release)`) instead of `compare_exchange`. Under concurrent error bursts, multiple dumps fire despite `OnceTrigger`. The code comment acknowledges this ("at worst two dumps") but for a diagnostic tool, two dumps = ambiguity about which is canonical. Fix: `compare_exchange(false, true, …)`. `src/trigger.rs:141-147` |
| Surface trigger dump failures | 🔴 `TODO` | ~1h | `on_event` discards the `fire_dump` result: `let _result = self.fire_dump(&reason);` (`src/layer.rs:778`). If the dump fails (disk full, permissions denied), the operator thinks the incident was captured but it wasn't. The `OnceTrigger` makes this worse: the token is consumed before the dump runs, so there's no retry. Wire failures into the `on_dump` callback or emit a `tracing::error!`. Identified in `docs/status/2026-08-11_18-51…md` section d.2. |

## Medium Impact — API Completeness & Testing

| Task | Status | Effort | Notes |
|------|--------|--------|-------|
| Implement `Debug` for `FlightRecorderLayer` | 🔴 `TODO` | ~30m | The layer holds `Option<DumpConfig>` with `Box<dyn Trigger>` — none of which implement `Debug`, so operators can't `dbg!()` the layer to inspect trigger state. Consider requiring `Trigger: Debug` or a manual impl. `src/layer.rs:611` |
| Add `dump_envelope_to_writer` | 🔴 `TODO` | ~1h | API asymmetry: array dumps have `dump_to_writer` + `dump_to_writer_lines`, but the envelope API (`dump_envelope`, `dump_envelope_to_json`, `dump_envelope_to_file`) has no streaming writer variant. `src/layer.rs` |
| Close pretty-variant test gaps | 🔴 `TODO` | ~1h | `dump_to_writer_pretty`, `dump_to_file_pretty`, `dump_envelope_to_file_pretty` are implemented but have no tests. The compact variants are tested; the pretty variants differ only in `to_writer_pretty` vs `to_writer` but should still be covered. Identified in `docs/status/2026-08-11_19-32…md` section e. |
| Close `on_dump` coverage gaps | 🔴 `TODO` | ~1h | `on_dump` callback is tested for manual `dump_to_file` and trigger dumps, but NOT for `dump_with_retention` or `dump_envelope_to_file` (same shared `write_and_report` path, but no explicit test). Identified in `docs/status/2026-08-11_19-32…md` section b. |
| Add `examples/compression.rs` + `examples/observability.rs` | 🔴 `TODO` | ~1h | Gzip compression and `on_dump` hooks shipped with zero runnable examples. The crate has 5 examples but none cover the two newest features. |

## Low Impact — Features & Polish

| Task | Status | Effort | Notes |
|------|--------|--------|-------|
| Wire gzip into trigger/retention path | 🔴 `TODO` | ~2h | The `gzip` feature only covers manual `dump_to_file_gz` / `dump_envelope_to_file_gz`. Automatic trigger dumps (`fire_dump` → `retention_write`) and `dump_with_retention` are always uncompressed. Need `dump_with_retention_gz` or a compression config on `with_dump_on`. |
| `FlightRecorderBuilder` | 🔴 `TODO` | ~4h | Unify capacity, span capture, `on_dump`, compression, and retention into one builder. Currently scattered across `FlightRecorder::new`/`with_on_dump` and `FlightRecorderLayer::new`/`with_span_capture`/`with_dump_on`. See `ROADMAP.md` theme 4. |
| Configurable redaction patterns | 🔴 `TODO` | ~2h | Sensitive-field patterns are hardcoded (14 patterns, `src/capture.rs`). Users with custom secret names (e.g. `x-api-key`) cannot add them without forking. Accept a `HashSet<String>` or predicate. |
| Document `with_dump_on` builder ordering caveat | 🔴 `TODO` | ~10m | `with_dump_on` consumes `self` and must be called BEFORE `with_filter` (which wraps the layer in `Filtered<L, F, S>`). Documented only in `examples/trigger.rs`, not in `AGENTS.md` gotchas. |

## Deferred

| Task | Status | Effort | Notes |
|------|--------|--------|-------|
| Async/non-blocking capture | 🔵 `DEFERRED` | ~3h | Background dump thread via `std::thread::spawn`, drain on shutdown. Non-trivial lifecycle change (join/drain semantics, backpressure). Deferred to its own release focus. |
| `parking_lot::Mutex` | 🔵 `DEFERRED` | ~1h | Reduces lock overhead. Batch with other lock-related perf work. |
| `Arc<CapturedEvent>` in buffer | 🔵 `DEFERRED` | ~2h | Cheap snapshot clones (avoid deep-copy on `snapshot()`). Batch with `parking_lot`. |

## Rejected

| Task | Status | Notes |
|------|--------|-------|
| `SmallVec` for fields | 🔴 `REJECTED` | Changing `CapturedEvent.fields` from `Vec` to `SmallVec` is a breaking public-type change AND breaks the `utoipa::ToSchema` derive (no built-in SmallVec schema). Revisit only if paired with a custom schema + a major version. Allocation profiling (~9 allocs/event) should guide whether this is worth the churn. |
| `no_std` / embedded support | 🔴 `REJECTED` | The crate depends on `chrono` (wall-clock timestamps), `std::sync::Mutex` (shared ring buffer), and `std::fs` (file/retention dumps). A `no_std` port would need a spin/`critical-section` mutex, a timestamp abstraction, and the filesystem API feature-gated out. Not actionable until a concrete embedded use case demands it. See `ROADMAP.md` non-goals. |

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
