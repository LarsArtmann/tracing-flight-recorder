# Status Report — tracing-flight-recorder

**Date:** 2026-08-10 18:51
**Session scope:** GitHub repo creation, release infrastructure from segment-buffer learnings, crates.io publication
**Author:** Crush (self-review)
**Commit:** `dd6d2bb` — tag `v0.1.0` pushed and published

---

## Executive Summary

This session took the crate from "tagged locally, no remote" to **fully
published on crates.io with a public GitHub repo**. The segment-buffer release
process was studied and its best patterns were ported: `docs/RELEASE.md`,
`release.toml`, `deny.toml`, `publish.yml` workflow, dependabot auto-merge,
docs.rs metadata, README badges, and `cargo-deny`/`cargo-audit` in CI.

The crate is live at:

- **crates.io:** https://crates.io/crates/tracing-flight-recorder (v0.1.0, 26KB)
- **docs.rs:** https://docs.rs/tracing-flight-recorder (built successfully with openapi feature)
- **GitHub:** https://github.com/LarsArtmann/tracing-flight-recorder (public, 7 topics)
- **GitHub release:** https://github.com/LarsArtmann/tracing-flight-recorder/releases/tag/v0.1.0

CI is green on all 6 jobs (test+clippy+fmt on stable and beta, MSRV 1.86, doc
build, publish dry-run, cargo audit + cargo deny).

**But:** the CHANGELOG has a wrong package size claim, the published 0.1.0
crate on crates.io does NOT match the current HEAD (it was published before
the release infrastructure commit), the ROADMAP section 5 is stale again, and
`actions/checkout@v4` triggers Node.js 20 deprecation warnings in CI.

---

## a) FULLY DONE ✅

### GitHub Repository Created

- **Repo:** `LarsArtmann/tracing-flight-recorder` (public)
- **Description:** "In-memory ring-buffer flight recorder for tracing events. Inspired by Go 1.25's trace.FlightRecorder — continuously buffers DEBUG/TRACE events, snapshots on failure."
- **Topics:** tracing, flight-recorder, diagnostics, ring-buffer, debugging, rust, tracing-subscriber (7 topics)
- **Remote:** `git@github.com:LarsArtmann/tracing-flight-recorder.git` (SSH)
- **master pushed:** all 8 commits
- **v0.1.0 tag pushed**

### Release Infrastructure (ported from segment-buffer)

- **`docs/RELEASE.md`** — Full release runbook: pre-release checklist, verification gate, semver rules, step-by-step cutting (bump → commit → tag → push → release → publish), post-release verification, rollback. Adapted for this crate's feature set.
- **`release.toml`** — `cargo-release` config: `push=false` (hand-curate GitHub releases), `publish=true`, CHANGELOG pre-release replacement, signed tags/commits, `v` prefix.
- **`deny.toml`** — `cargo-deny` config: RustSec advisories, license allowlist (Apache-2.0, MIT, BSD, Unicode-3.0), ban policy, source registry restrictions.
- **`.github/workflows/publish.yml`** — Automated crates.io publish on `v*.*.*` tag push. Tag/version match verification. Idempotency guard (curls crates.io API, skips if version exists). Dry-run on PRs touching `Cargo.toml`.
- **`.github/workflows/ci.yml` upgraded** — Added `PROPTEST_CASES=256` for deterministic CI, `RUSTDOCFLAGS="-D warnings"` on doc build, `cargo fetch --locked` for Cargo.lock freshness, replaced `rustsec/audit-check@v2.0.0` action with explicit `cargo-audit` + `cargo-deny` installs.
- **`.github/dependabot.yml` upgraded** — Commit message prefixes (`ci(actions)`, `deps`), auto-merge with squash, Monday schedule.
- **`[package.metadata.docs.rs]`** — Builds docs.rs with `openapi` feature so `ToSchema` derive is visible. `rustdoc-args = ["--cfg", "docsrs"]`.
- **`[lints.rust]`** — `unexpected_cfgs` check-cfg for `docsrs`.
- **`documentation` field** — Added to Cargo.toml pointing to `https://docs.rs/tracing-flight-recorder`.
- **README badges** — crates.io, docs.rs, CI, MSRV 1.86, Apache-2.0 license.
- **ROADMAP section 5 updated** — Marked v0.1.0 as tagged, removed done items. _(But see section d — it's already stale again.)_
- **CHANGELOG package size claim fixed** — Updated to "150.9KiB → 81.7KiB, 23 → 16 files". _(But see section d — it's wrong again.)_
- **AGENTS.md** — Added "Release Infrastructure" section documenting all new files.

### crates.io Publication

- **Published:** `tracing-flight-recorder v0.1.0` to crates.io
- **Package:** 18 files, 86.1KiB (25.4KiB compressed), crate_size on crates.io: 26,041 bytes
- **Published by:** LarsArtmann
- **Published at:** 2026-08-10T16:47:54Z
- **Features visible on crates.io:** `default = []`, `openapi = ["dep:utoipa"]`

### docs.rs Verified

- **URL:** https://docs.rs/tracing-flight-recorder
- **Build:** Successful, "100% of the crate is documented"
- **Feature flag:** `openapi` visible in the feature flags panel
- **ToSchema:** `CapturedEvent` struct visible with all fields
- **Dependencies:** All 6 deps resolved correctly (chrono, serde, serde_json, tracing, tracing-subscriber, utoipa optional)

### CI Verified on GitHub

All 6 jobs green on the `dd6d2bb` push:

- Test + Clippy + Fmt (stable) ✅ 45s
- Test + Clippy + Fmt (beta) ✅ 43s
- MSRV (1.86) ✅ 46s
- Doc Build ✅ 23s
- Publish Dry-Run ✅ 24s
- cargo audit + cargo deny ✅ 4m57s

### GitHub Release Created

- **URL:** https://github.com/LarsArtmann/tracing-flight-recorder/releases/tag/v0.1.0
- **Title:** "v0.1.0 — Initial release"
- **Body:** Full release notes with feature summary, test coverage, CI description, verification statement

---

## b) PARTIALLY DONE 🟡

### `CARGO_REGISTRY_TOKEN` Secret — Not Configured

The `publish.yml` workflow ran on the v0.1.0 tag push and **failed** because
the `CARGO_REGISTRY_TOKEN` secret is not set in the repo settings. The crate
was published manually via `cargo publish --all-features` instead. The
workflow is correct; it just needs the secret configured for future
automated releases.

One-time setup needed:

1. Go to https://crates.io/settings/api-tokens
2. Create token with `publish-new` + `publish-update` scopes
3. Add as repo secret `CARGO_REGISTRY_TOKEN` at
   https://github.com/LarsArtmann/tracing-flight-recorder/settings/secrets/actions

### Published v0.1.0 ≠ HEAD

The published crate on crates.io is the state of `36af9c8` (the collision
refactor commit), NOT `dd6d2bb` (the release infrastructure commit). The
v0.1.0 tag was moved to `dd6d2bb` BEFORE push, but the manual `cargo publish`
ran from the working directory which matched `dd6d2bb`. However,
`deny.toml` and `release.toml` were added in `dd6d2bb` — so the published
crate **does** include those files. This is actually correct. The package
list (18 files, 86.1KiB) matches what was published.

_Correction: after verifying, the published state is consistent with `dd6d2bb`.
This is NOT a partial — it's done correctly. Reclassifying from partial to done._

---

## c) NOT STARTED ⬜

### v0.1.0 is Published — No Code Changes Needed

The crate is out. The remaining work is infrastructure and next-version
planning, not v0.1.0 fixes.

### P3 Roadmap Spikes (M14-M22)

All 9 post-release research spikes remain unstarted (expected). See the
50-item list below.

### Test Gaps from Previous Self-Reviews

Items from the previous session's "50 things" list remain unaddressed:

- `dump_with_retention` with `max_files = 0` or `1`
- `FlightRecorder::new(0)` zero capacity edge case
- `dump_to_file` with read-only directory
- `snapshot()` on empty recorder
- `is_sensitive_field("")` empty string field name
- `FieldVisitor` with `i128`/`u128` values
- Field values >1KB or with special JSON characters

### `.github/ISSUE_TEMPLATE/` and `SECURITY.md`

Not created. segment-buffer has these; this crate doesn't yet.

---

## d) TOTALLY FUCKED UP 🔴

### 1. CHANGELOG Package Size Claim Is STILL Wrong

The CHANGELOG `### Changed` section says:

> Tightened `exclude` list ... (150.9KiB → 81.7KiB, 23 → 16 files)

The **actual** published crate is **18 files, 86.1KiB**. This is wrong
because:

1. The previous session said "90.2KiB" (wrong — was 81.7 at that point)
2. I "fixed" it to "81.7KiB, 16 files" in the collision refactor commit
3. Then I added `deny.toml` + `release.toml` to the package (+2 files,
   +4.4KiB) in the release infrastructure commit
4. I updated the CHANGELOG to remove the `RELEASE.md` exclude but
   **never updated the size claim** to reflect the actual final number

This is the **same pattern as last session** — making exclude changes
without updating the CHANGELOG size claim. The CHANGELOG is on crates.io,
visible to every consumer, and it's factually incorrect.

**Severity:** Medium (published false claim in release notes)

### 2. ROADMAP Section 5 Is Stale AGAIN

I updated ROADMAP section 5 to say "v0.1.0 is tagged locally. The remaining
step is publishing to crates.io" — but then **I published it to crates.io in
the same session** without updating the ROADMAP again. It now says "tagged
locally" when the crate is fully published and live.

**Severity:** Medium (documentation drift on a live release)

### 3. CHANGELOG Still References `rustsec/audit-check@v2.0.0`

Line 41 of the CHANGELOG says:

> Security audit CI job (`rustsec/audit-check@v2.0.0`)

But the CI was rewritten to use `cargo install cargo-audit` + `cargo install
cargo-deny` instead. The `rustsec/audit-check` action is no longer in the
workflow. The CHANGELOG describes CI state that no longer exists.

**Severity:** Low (historical entry, but factually wrong about the final v0.1.0 state)

### 4. `actions/checkout@v4` — Node.js 20 Deprecation

All CI jobs show deprecation warnings:

> Node.js 20 is deprecated. The following actions target Node.js 20 but are
> being forced to run on Node.js 24: actions/checkout@v4

segment-buffer uses `actions/checkout@v7`. This will eventually break when
GitHub removes Node.js 20 support.

**Severity:** Low (warnings now, breakage later)

### 5. Published CHANGELOG Has Internal `exclude` Details

The CHANGELOG entry about exclude paths
(`/docs/status`, `/docs/planning`, `/AGENTS.md`, etc.) is interesting to
crate **developers** but noise to crate **consumers**. Consumers don't care
about the internal file structure of the development repository. This is
internal process detail leaking into published release notes.

**Severity:** Low (cosmetic, but unprofessional for a published crate)

---

## e) WHAT WE SHOULD IMPROVE

### Process Improvements

1. **Every quantitative claim in the CHANGELOG must be verified against
   `cargo publish --dry-run` output at tag time.** This is the second session
   where the package size claim was wrong. The fix is mechanical: before
   committing the release commit, run `cargo publish --dry-run --all-features`
   and copy the "Packaged N files, XKiB" line into the CHANGELOG.

2. **ROADMAP updates must happen as part of the task, not deferred.** Same
   lesson as TODO_LIST split-brain. I updated ROADMAP, then published, then
   didn't update ROADMAP again. "Fix-on-sight" means: the moment the crate
   goes live on crates.io, ROADMAP section 5 must reflect that.

3. **CHANGELOG entries should be rewritten before tag-push to reflect the
   FINAL state, not intermediate states.** The CHANGELOG went through three
   states: "rustsec action" → "cargo-audit/deny" → published. Only the final
   state matters to consumers. The CHANGELOG should describe what shipped,
   not the journey.

### Technical Improvements

4. **Bump `actions/checkout` to `@v7`** to silence the Node.js 20 deprecation
   warnings. segment-buffer uses `@v7` successfully.

5. **The CHANGELOG on crates.io is immutable** for v0.1.0. The wrong size
   claim, the stale `rustsec/audit-check` reference, and the internal exclude
   details are permanently baked into the published crate. The only fix is
   a v0.1.1 patch release with corrected CHANGELOG — but that's a product
   decision, not a technical one.

6. **Consider whether `release.toml` and `deny.toml` belong in the published
   crate.** Consumers don't need `release.toml` (it's the maintainer's
   cargo-release config). `deny.toml` is arguably useful (so consumers can
   run `cargo deny` themselves), but it's a judgment call. segment-buffer
   excludes these from its package.

---

## f) Up to 50 Things to Do Next

### Immediate (fix published state)

1. Decide: ship a v0.1.1 patch with corrected CHANGELOG, or accept v0.1.0
   as-is (the code is correct, only the release notes are imprecise)
2. Update ROADMAP section 5 to say "v0.1.0 published to crates.io"
3. Update CHANGELOG `### Changed` size claim to "150.9KiB → 86.1KiB, 23 → 18 files"
4. Update CHANGELOG `### Added` line 41 to say `cargo audit + cargo deny` instead of `rustsec/audit-check@v2.0.0`
5. Bump `actions/checkout@v4` → `@v7` across all workflows (ci.yml, publish.yml)
6. Configure `CARGO_REGISTRY_TOKEN` GitHub secret for automated future releases

### Repository polish

7. Add `.github/ISSUE_TEMPLATE/bug_report.md`
8. Add `.github/ISSUE_TEMPLATE/feature_request.md`
9. Add `.github/PULL_REQUEST_TEMPLATE.md`
10. Add `SECURITY.md` for vulnerability reporting
11. Enable branch protection on `master` (require CI passes before merge)
12. Run `gh repo edit LarsArtmann/tracing-flight-recorder --enable-auto-merge`
13. Consider adding `renovate.json` alongside dependabot (belt-and-braces)
14. Add `docs.rs` badge verification (it should auto-update on publish)

### Documentation

15. Add crate-level usage example beyond Quick Start (e.g., tower middleware)
16. Cross-link CONTRIBUTING.md ↔ README.md
17. Document the `#[cfg(doctest)]` README doctest pattern in CONTRIBUTING.md
18. Consider architecture diagram (D2 or mermaid) in README or CONTRIBUTING
19. Add `docs/` index explaining the docs directory structure

### Test gaps (from previous sessions, still valid)

20. `dump_with_retention` with `max_files = 0` — what happens?
21. `dump_with_retention` with `max_files = 1` — minimal retention
22. `dump_to_file` with read-only directory — permission error path
23. `FlightRecorder::new(0)` — zero capacity edge case
24. `snapshot()` on empty recorder — returns empty vec?
25. proptest for `clear()` + subsequent pushes
26. proptest for clone-sharing under concurrent access
27. `is_sensitive_field("")` — empty string field name
28. `FieldVisitor` with `i128`/`u128` values
29. Field values >1KB (stress serialization)
30. Field values with special JSON chars (quotes, backslashes, newlines)
31. `dump_to_json()` on empty recorder — `[]`?

### Code quality

32. Consider `#[non_exhaustive]` on `CapturedEvent` for forward compatibility
33. Consider derive `Debug` for `FlightRecorder` (manual impl exists)
34. Review whether `FieldVisitor` needs to be public
35. Consider `serde` feature flag for users who don't want serde dependency
36. Document the `FlightRecorder::new(0)` behavior explicitly
37. Consider `parking_lot::Mutex` benchmark vs `std::sync::Mutex`
38. Consider excluding `release.toml` from published crate (maintainer-only)
39. Consider excluding `deny.toml` from published crate (or document why it's included)

### ROADMAP themes (P3 spikes, post-release)

40. M14: Explore `tracing-core` direct integration (likely not worth it)
41. M15: Async dump support (tokio::fs for non-blocking I/O)
42. M16: Binary dump format (more compact than JSON)
43. M17: Compression support for dump files (gzip)
44. M18: Network dump (send snapshot to remote endpoint)
45. M19: Integration with `tracing-flame` for flamegraph generation
46. M20: Snapshot filtering (dump only ERROR/WARN events)
47. M21: Configurable redaction patterns (user-defined sensitive field names)
48. M22: WASM compatibility investigation
49. Time-windowed capture (ROADMAP theme 1: evict by age, not just count)
50. Tower middleware auto-dump on error response (ROADMAP theme 4)

---

## g) Questions for the User

### 1. Should I cut a v0.1.1 patch release to fix the CHANGELOG inaccuracies on crates.io, or leave v0.1.0 as-is?

The code is correct. Only the CHANGELOG text has three imprecise claims
(wrong package size, stale `rustsec/audit-check` reference, internal exclude
details). A v0.1.1 would fix the CHANGELOG but also require justifying a
version bump for a docs-only change. I can't decide this for you — it's a
question of how much you care about release-note accuracy on crates.io for
a v0.1.0 release.

### 2. Should `release.toml` and `deny.toml` be excluded from the published crate?

`release.toml` is purely maintainer config (cargo-release settings). `deny.toml`
is arguably useful for consumers (so they can run `cargo deny check` on the
crate), but segment-buffer excludes both. This is a taste decision — I can
argue either way but can't determine your preference.

### 3. Should I configure the `CARGO_REGISTRY_TOKEN` secret now, or leave manual `cargo publish` as the process?

I have your crates.io token (from `~/.cargo/credentials.toml`), but adding it
as a GitHub Actions secret requires either the GitHub web UI or `gh secret
set`. I can run `gh secret set CARGO_REGISTRY_TOKEN` if you want automated
publish-on-tag for future releases, but I need your explicit approval to
push your crates.io token into GitHub secrets.

---

## Resolution (2026-08-10)

Crate published, CI green, automated publishing wired. All findings resolved.

| Finding                                            | Resolution                                                                | Commit    |
| -------------------------------------------------- | ------------------------------------------------------------------------- | --------- |
| CHANGELOG wrong package size (150.9→81.7 KiB)      | Corrected to 86.1 KiB / 17 files                                          | `3f317fd` |
| ROADMAP section 5 stale                            | Updated to reflect published state                                        | `90cb0e0` |
| CHANGELOG references removed `rustsec/audit-check` | Rewritten CI uses `cargo-audit` + `cargo-deny`                            | `3f317fd` |
| `actions/checkout@v4` deprecation                  | Bumped to v7                                                              | `3f317fd` |
| `CARGO_REGISTRY_TOKEN` not configured              | Configured — tag-push publishing automated                                | —         |
| `release.toml` not excluded                        | Excluded from published crate                                             | `3f317fd` |
| All 50 "next things" brainstorm                    | Items picked up by sessions 6–12. Remaining open items in `TODO_LIST.md`. | —         |
