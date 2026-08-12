# TODO List

> Short-term, actionable, bounded work items, verified against the actual code.
> For long-term vision and unrefined ideas, see `ROADMAP.md`.
> Items are ranked by impact. Status is verified, not assumed.

## Status legend

| Status           | Meaning                                                        |
| ---------------- | -------------------------------------------------------------- |
| 🔴 `TODO`        | Not started. Needs doing.                                      |
| 🟡 `IN_PROGRESS` | Actively being worked on.                                      |
| 🔵 `DEFERRED`    | Deliberately postponed to a future milestone (with reasoning). |
| 🔴 `REJECTED`    | Investigated and decided against (with reasoning in Notes).    |

> `DONE` items are removed from this list and logged in `CHANGELOG.md`.

## Release

| Task                   | Status    | Effort | Notes                                                                                                                                                                                                                                                                                                        |
| ---------------------- | --------- | ------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Tag and publish v0.3.0 | 🔴 `TODO` | ~15m   | **v0.2.0 skipped** — was never tagged, no users have it; all changes batched into v0.3.0. `Cargo.toml` is at `0.3.0`, CHANGELOG merged, `cargo publish --dry-run --all-features` passes. Remaining: commit, `git tag v0.3.0`, `git push origin v0.3.0` (triggers `publish.yml`), verify crates.io + docs.rs. |

## Low Impact — Features & Polish

| Task                                  | Status    | Effort | Notes                                                                                                                                                                                                                                                                       |
| ------------------------------------- | --------- | ------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Wire gzip into trigger/retention path | 🔴 `TODO` | ~2h    | The `gzip` feature only covers manual `dump_to_file_gz` / `dump_envelope_to_file_gz`. Automatic trigger dumps (`fire_dump` → `retention_write`) and `dump_with_retention` are always uncompressed. Need `dump_with_retention_gz` or a compression config on `with_dump_on`. |
| `FlightRecorderBuilder`               | 🔴 `TODO` | ~4h    | Unify capacity, span capture, `on_dump`, compression, and retention into one builder. Currently scattered across `FlightRecorder::new`/`with_on_dump` and `FlightRecorderLayer::new`/`with_span_capture`/`with_dump_on`. See `ROADMAP.md` theme 4.                          |
| Configurable redaction patterns       | 🔴 `TODO` | ~2h    | Sensitive-field patterns are hardcoded (14 patterns, `src/capture.rs`). Users with custom secret names (e.g. `x-api-key`) cannot add them without forking. Accept a `HashSet<String>` or predicate.                                                                         |

## Deferred

| Task                           | Status        | Effort | Notes                                                                                                                                                                     |
| ------------------------------ | ------------- | ------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Async/non-blocking capture     | 🔵 `DEFERRED` | ~3h    | Background dump thread via `std::thread::spawn`, drain on shutdown. Non-trivial lifecycle change (join/drain semantics, backpressure). Deferred to its own release focus. |
| `parking_lot::Mutex`           | 🔵 `DEFERRED` | ~1h    | Reduces lock overhead. Batch with other lock-related perf work.                                                                                                           |
| `Arc<CapturedEvent>` in buffer | 🔵 `DEFERRED` | ~2h    | Cheap snapshot clones (avoid deep-copy on `snapshot()`). Batch with `parking_lot`.                                                                                        |

## Rejected

| Task                        | Status        | Notes                                                                                                                                                                                                                                                                                                                                                          |
| --------------------------- | ------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `SmallVec` for fields       | 🔴 `REJECTED` | Changing `CapturedEvent.fields` from `Vec` to `SmallVec` is a breaking public-type change AND breaks the `utoipa::ToSchema` derive (no built-in SmallVec schema). Revisit only if paired with a custom schema + a major version. Allocation profiling (~9 allocs/event) should guide whether this is worth the churn.                                          |
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
