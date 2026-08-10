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

| Task                                                            | Status    | Impact | Effort | Evidence                                                                                  |
| --------------------------------------------------------------- | --------- | ------ | ------ | ----------------------------------------------------------------------------------------- |
| Concurrency stress test (multi-thread push + snapshot)         | 🔴 `TODO` | High   | 20min  | No multi-thread test exists; `Arc<Mutex<VecDeque>>` is thread-safe but unexercised under contention |
| Proptest eviction invariant (push > capacity → len == capacity) | 🔴 `TODO` | Med    | 25min  | Eviction tested with fixed inputs only; no property-based testing of the invariant       |

## Medium Impact

| Task                                                                     | Status    | Impact | Effort | Evidence                                                                              |
| ------------------------------------------------------------------------ | --------- | ------ | ------ | ------------------------------------------------------------------------------------- |
| Poison-recovery test (panicked thread → recorder still usable)           | 🔴 `TODO` | Med    | 15min  | Poison-safe locking is a design choice but no test exercises the recovery path         |
| Unicode field name redaction test                                        | 🔴 `TODO` | Low-Med | 10min  | `is_sensitive_field` lowercases then substring-matches; Unicode edge cases untested   |
| Non-JSON files survive retention pruning                                 | 🔴 `TODO` | Low-Med | 10min  | `cleanup_old_snapshots` filters by `.json` extension; no test verifies non-JSON files are left alone |

## Low Impact

_(none currently)_

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
