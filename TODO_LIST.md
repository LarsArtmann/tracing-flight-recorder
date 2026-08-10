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
| Untrack `target/` from git (680 build-artifact files committed) | 🔴 `TODO` | High   | 10min  | `git ls-files target/ \| wc -l` = 680; `.gitignore` has `target/` but files remain tracked |
| Remove leaked `monitor365` project name from public docs        | 🔴 `TODO` | High   | 10min  | `src/layer.rs:203` (doc comment example target) and `README.md:21` ("Zero monitor365 dependencies") — nonsensical in a public crate |

## Medium Impact

| Task                                                                     | Status    | Impact | Effort | Evidence                                                                              |
| ------------------------------------------------------------------------ | --------- | ------ | ------ | ------------------------------------------------------------------------------------- |
| Add GitHub Actions CI (build, `cargo test --all-features`, clippy deny)  | 🔴 `TODO` | Med    | 30min  | No `.github/` dir; no CI; strict clippy gate exists but only runs locally             |
| Guard against same-second filename collision in `dump_with_retention`    | 🔴 `TODO` | Med    | 20min  | `src/layer.rs:140` uses `%Y%m%dT%H%M%S` (second precision); two dumps in one second silently overwrite |
| Add test asserting `utoipa::ToSchema` output for `CapturedEvent`         | 🔴 `TODO` | Med    | 20min  | Derive compiles under `--all-features` (`src/capture.rs:13`) but no test verifies the generated schema (FEATURES.md flags this as PARTIALLY_FUNCTIONAL) |

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
