# Status Report — Comparative Review Session

**Date:** 2026-08-11 15:58
**Session scope:** Compare `go-flightrecorder` (Go) and `tracing-flight-recorder` (Rust), produce a feedback document for the Rust project, update it when the Go project received new features mid-session.
**Author:** Crush (self-review)

---

## Executive Summary

This session produced three deliverables: a comparison table of both
projects, a verbal analysis of buffer-unit design, and a 569-line feedback
document at `docs/feedback/new/2026-08-11_comparative-review-feedback.md`.

The first version of the feedback was formulaic and shallow. The user
called it out. I rewrote it with verified bugs, precise allocation counts,
and a span-context finding I had missed entirely. Then the Go project
received a major feature update, and I incorporated those changes.

The report is now solid but has real gaps. I simulated bugs with
standalone code instead of writing actual test cases in the crate. I never
profiled allocation counts — I counted them by reading. I never verified
the `go tool trace` gzip claim empirically. I didn't read the Go test file
(1,892 lines). These are documented below.

---

## a) FULLY DONE

### 1. Comparison table delivered

Produced a comprehensive table comparing both projects across: purpose,
language, buffer model, hot-path cost, trigger system, once-semantics,
retention, secret redaction, output format, dependencies, CI, tests,
examples, and roadmap direction. Delivered in the first response.

### 2. Buffer-unit analysis delivered

Answered the user's question about JSON array efficiency and event-count
vs time+bytes buffering. Correctly identified that time+bytes is the
better model for a flight recorder because the purpose is temporal.
Explained why event count fails under burst load.

### 3. Feedback document written and rewritten

- **v1** (472 lines): Formulaic P0-P3 grid, guessed allocation counts,
  missed span context entirely. User rejected it.
- **v2** (560 lines): Rewrote from scratch. Found 2 real bugs by
  execution. Precise allocation count (14-17, not ~10). Found the span
  context blind spot. Found false claims. Found security gaps.
- **v3** (569 lines): Incorporated Go project's mid-session feature update.
  Found 2 Go documentation contradictions.

Final location: `tracing-flight-recorder/docs/feedback/new/2026-08-11_comparative-review-feedback.md`

### 4. Bugs verified by execution

- **Rust capacity=0 bug:** Confirmed via standalone Rust program that
  `VecDeque` with capacity 0 retains 1 event after push. The `pop_front()`
  on an empty deque is a no-op.
- **Rust retention=0 bug:** Confirmed via standalone Rust program that
  `saturating_sub(0)` produces excess=1, causing self-deletion.
- **Go gzip doc contradiction:** Found by grep. The `WithCompression` doc
  comment claims `.trace.gz` is "loadable by go tool trace (supported
  since Go 1.19)" but the project's own status report empirically disproved
  this in three ways.

### 5. Allocation count verified by code reading

Counted 14 allocations for a 4-field event, 17 for a 5-field event. Found
the hidden allocation source (`is_sensitive_field` calls `to_lowercase()`
on every field name). This was the single most wasteful pattern in the hot
path.

### 6. Both test suites verified green

- Rust: 27 unit + 4 doctests pass with `cargo test --all-features`
- Go: 64 tests pass with `go test -race -count=1`

---

## b) PARTIALLY DONE

### 1. Span context finding is correct but the fix sketch is incomplete

I identified that `_ctx: Context` is discarded in `on_event`, losing all
span context. I proposed adding `spans: Vec<SpanContext>` to
`CapturedEvent`. But I did not:

- Verify which `tracing` API methods are needed (`ctx.event_scope()`,
  `ctx.current_span()`, or `ctx.scope()` — they have different semantics)
- Check whether `new_span` needs to be implemented on the `Layer` to
  capture span fields at creation time
- Prototype the fix to see if it actually works
- Consider whether span context should be opt-in (performance cost) or
  always-on

**Impact:** The finding is valid but the recommended fix is underspecified.

### 2. Go documentation bugs found but not fixed

I found that `options.go:121` and `FEATURES.md:56` claim gzip is loadable
by `go tool trace`, contradicting the project's own empirical finding. I
documented this in the feedback file but did not fix it. The Go project is
not the one I was asked to write feedback for, but the contradictions are
real and fixable.

### 3. Allocation count is from reading, not profiling

I counted allocations by tracing every code path in `from_event` and
`FieldVisitor`. This is more accurate than guessing (my v1 said "~10") but
less authoritative than actually running an allocation profiler. Rust has
tools for this (`cargo install cargo-dhat`, or a global allocator wrapper
that counts). I did not use them.

**Impact:** The allocation count is almost certainly correct but carries
no empirical proof. A reviewer could challenge the methodology.

---

## c) NOT STARTED

### 1. Did not read the Go test file

`recorder_test.go` is now 1,892 lines — the largest file in either
project. I never read a single line of it. It could contain test quality
issues, missing edge cases, vacuous assertions, or test smells that would
inform the comparison. The Go status report itself mentions a "vacuous
stress-test assertion" that I could have found independently.

### 2. Did not verify `go tool trace` gzip claim empirically

The Go status report claims `go tool trace` rejects gzip files, verified
"three ways." I trusted this claim without independently verifying it.
I could have captured a compressed trace and run `go tool trace` myself.

### 3. Did not write actual test cases in the Rust crate

I verified bugs with standalone Rust programs in `/tmp`. I did not add
test cases to the actual crate to confirm the bugs in-context. This would
have been more authoritative and could have been the basis for a PR.

### 4. Did not check the Go project's `errors.AsType` generic helper

`recorder.go:609` uses `errors.AsType[*SnapshotError](err)` — a generic
function that is not in the standard `errors` package as I know it. It
might be a Go 1.26 addition, or it might be defined somewhere I didn't
look. I never found its definition. The tests pass so it exists, but I
don't know where or how.

### 5. Did not read the Go consumer feedback documents

The Go project has two new docs that drove the feature update:

- `docs/feedback/new/2026-08-11_real-world-consumer-feedback-from-project-discovery-sdk.md` (331 lines)
- `docs/status/2026-08-11_14-49_operational-features-from-consumer-feedback.md` (275 lines)

I only skimmed the commit message. The actual feedback content could
contain insights relevant to the Rust project's direction.

### 6. Did not check whether Rust `FieldVisitor` export is needed by tests

I recommended making `FieldVisitor` private (`pub(crate)`), but the test
in `capture.rs:193-209` uses `utoipa::OpenApi` derive — not `FieldVisitor`
directly. I should have verified that no test or example actually imports
`FieldVisitor` before recommending its removal from the public API.

### 7. Did not verify the Go project's async drain correctness

I praised `beginShutdown` + `wg.Wait()` as "textbook-correct shutdown
ordering" without deeply analyzing the memory ordering. The `stopped` flag
is set under `r.mu`, and `wg.Wait()` happens after `beginShutdown` returns.
But is there a proper happens-before relationship between `wg.Add(1)` in
`SnapshotIfAsync` and `wg.Wait()` in `Stop`? Go's mutex provides
happens-before, so this should be safe — but I didn't prove it.

---

## d) TOTALLY FUCKED UP

### 1. The first feedback report was garbage

**What happened:** I produced a 472-line report with a symmetric P0-P3
grid that looked thorough but was shallow. I guessed at allocation counts
("~10"), didn't verify a single bug by execution, and completely missed
the span context issue — the most important finding in the entire review.

**Root cause:** I optimized for structure and apparent completeness over
depth and verification. I read ~60% of the source and filled gaps with
plausible-sounding analysis. The format (P0/P1/P2/P3 with equal items per
tier) created an illusion of rigor that the content didn't support.

**Why it matters:** A feedback document that looks comprehensive but
contains guesses is worse than one that admits uncertainty. It gives false
confidence to the reader and wastes their time on unverified claims.

**Fix applied:** Rewrote from scratch after the user called it out. The
v2/v3 report verifies bugs by execution, counts allocations by code
reading (with the method documented), and leads with the span context
finding.

### 2. Did not verify claims before encoding them

Multiple examples:

- Said "zero non-tracing dependencies" claim is false — correct, but I
  could have checked if serde/chrono are behind a feature flag (they're
  not, but I should have verified in `Cargo.toml` `[features]`)
- Said `push` should be `pub(crate)` — didn't check if any test uses it
- Said `FieldVisitor` should not be exported — didn't check if tests
  import it

---

## e) WHAT WE SHOULD IMPROVE

### Process improvements for future reviews

1. **Verify everything.** Every claim about code behavior should be
   backed by execution, not reading. "I read the code and think X happens"
   is analysis. "I ran the code and X happens" is evidence.

2. **Read 100% of the source before writing.** The span context gap was
   in `layer.rs:252` — a file I read, but I read it for the ring buffer
   implementation, not for the `Layer` impl. I skipped the last 15 lines
   of the file on first read.

3. **Don't produce symmetric frameworks.** The P0-P3 grid forced me to
   invent items to fill tiers. The v2/v3 report leads with the thesis
   (span context) and organizes everything around it. That's better.

4. **Profile, don't count.** Allocation counts from reading code are
   better than guesses but worse than measurements. Next time, use a
   profiling tool.

5. **Read test files.** Tests reveal what the code actually does, what
   edge cases are covered, and where the author's confidence lies. I
   skipped the largest file in the project (1,892-line Go test file).

### Improvements to the feedback document itself

6. The span context fix recommendation should be prototyped, not just
   sketched.
7. The allocation count should be verified with a profiling tool.
8. The Go documentation bugs should be fixed, not just reported.
9. The `errors.AsType` generic helper should be traced to its definition.
10. The consumer feedback docs that drove the Go update should be read
    for Rust-relevant insights.

---

## f) UP TO 50 THINGS TO DO NEXT

Grouped by project and priority.

### Rust project — Immediate fixes (P0)

1. **Fix `FlightRecorder::new(0)` bug** — guard against capacity 0 in `new()`
   or in `push()`. Panic or return `Result`.
2. **Fix `dump_with_retention(_, _, 0)` self-delete bug** — guard against
   `max_files == 0` or adopt Go's "0 means unlimited" convention.
3. **Fix README "zero non-tracing dependencies" claim** — either drop
   chrono/serde to optional features, or change the claim.
4. **Add `authorization` to redaction patterns** — security gap.
5. **Fix `is_sensitive_field` allocation** — replace `to_lowercase()` with
   case-insensitive comparison. Free hot-path perf win.

### Rust project — Span context (P0)

6. **Research `tracing::layer::Context` API** — determine which methods
   expose the span stack (`event_scope`, `current_span`, `scope`).
7. **Prototype span context capture** — implement `on_event` with span
   walking, add `SpanContext` struct to `CapturedEvent`.
8. **Implement `new_span` on the Layer** — capture span fields at creation
   time, not just at event time.
9. **Test span context capture end-to-end** — emit events inside nested
   spans, verify span names and fields appear in the snapshot.
10. **Decide opt-in vs always-on** — span walking has a performance cost.
    Consider a feature flag or a configuration option.

### Rust project — Output improvements (P1)

11. **Add `dump_to_writer`** — trivial addition, big usability gain.
12. **Add NDJSON output format** — streamable, ingestible by log pipelines.
13. **Add dump metadata envelope** — timestamp, buffer span, event count,
    crate version, trigger reason.
14. **Make pretty-print opt-in** — default to compact JSON.

### Rust project — Operational features (P1, matching Go sibling)

15. **Add trigger system** — `Trigger` trait, `dump_if(trigger, ctx, path)`.
16. **Add once-semantics** — `AtomicBool` flag, `dump_once` + `reset`.
17. **Add async capture** — `tokio::spawn` or `std::thread::spawn`
    background dump, drain on shutdown.
18. **Add observability hooks** — `on_dump` callback with `DumpEvent`
    (duration, bytes, path, source).
19. **Add compression option** — `flate2` behind a feature flag.
20. **Add `dump_to_dir` with retention** — timestamped files, max-files
    pruning, prefix filtering.

### Rust project — Performance (P2)

21. **Profile allocation count with `cargo-dhat`** — verify the 14-17
    count empirically.
22. **Switch to `parking_lot::Mutex`** — reduces lock overhead.
23. **Use `Arc<CapturedEvent>` in buffer** — cheap clones, avoids deep
    copy on `snapshot()`.
24. **Use `Cow<'static, str>` for level** — 5 known values, zero alloc.
25. **Pre-allocate field capacity in `FieldVisitor`** — avoid Vec reallocs.
26. **Fix memory footprint test** — use a proper allocator tracker, not
    `size_of` + string length summation.

### Rust project — Hot-path cleanup (P2)

27. **Make `push` `pub(crate)`** — prevents buffer pollution by users.
28. **Un-export `FieldVisitor`** — internal implementation detail.
29. **Use `&'static str` for `"[REDACTED]"`** — avoid per-field allocation.

### Rust project — Testing (P2)

30. **Add test for capacity=0 edge case** — verify fix for bug #1.
31. **Add test for retention=0 edge case** — verify fix for bug #2.
32. **Add test for `dump_to_file` with read-only directory.**
33. **Add test for `snapshot()` on empty recorder.**
34. **Add test for `is_sensitive_field("")` empty string.**
35. **Add test for `i128`/`u128` field values with edge values.**

### Rust project — Time-based eviction (P3)

36. **Design hybrid `max_events OR max_age` eviction** — check oldest
    event timestamp on every push.
37. **Add time-span metadata to dumps** — report actual temporal coverage.

### Go project — Documentation fixes (P0)

38. **Fix `options.go:121`** — change "loadable by go tool trace" to
    "requires gunzip before go tool trace."
39. **Fix `FEATURES.md:56`** — same correction.

### Go project — Deeper analysis (P1)

40. **Read `recorder_test.go` (1,892 lines)** — find test quality issues,
    vacuous assertions, missing edge cases.
41. **Read the consumer feedback doc (331 lines)** — understand what drove
    the feature update, extract Rust-relevant insights.
42. **Verify `go tool trace` gzip rejection empirically** — don't trust
    the status report, reproduce it.
43. **Trace `errors.AsType` generic helper** — find its definition, verify
    it's a Go 1.26 stdlib addition.
44. **Read updated Go AGENTS.md** — verify it reflects the new features.

### Cross-project (P2)

45. **Write actual bug-fix PRs for the Rust crate** — not just a feedback
    document, but real code.
46. **Benchmark both projects' hot paths** — apples-to-oranges (runtime
    trace vs tracing events), but the comparison is still informative.
47. **Write a span-context prototype in the Rust crate** — prove the fix
    works before recommending it.
48. **Profile Rust allocation count with a real tool** — `cargo-dhat` or
    global allocator counter.
49. **Verify Go async drain correctness** — analyze happens-before
    guarantees in `beginShutdown` + `wg.Wait()`.
50. **Write a comparison matrix that includes feature parity** — track
    which Rust features match/exceed/lag the Go sibling.

---

## g) QUESTIONS I CANNOT ANSWER MYSELF

### 1. Should the Go documentation bugs be fixed now, or left for the Go project's own workflow?

The Go project has an auto-commit daemon and a separate workflow. I found
two documentation contradictions (`options.go:121` and `FEATURES.md:56`
both claim gzip is loadable by `go tool trace`, contradicting empirical
findings in their own status report). I could fix them in this session,
but the Go project wasn't the target of this review — the Rust project
was. Should I fix Go docs anyway, or leave them as findings in the
feedback file?

### 2. Is this feedback document meant to drive actual changes to the Rust crate, or is it purely analytical?

The feedback file contains 50 recommended next steps, but the Rust project
also has its own workflow and auto-commit daemon. Should I start
implementing the P0 fixes (capacity=0 bug, retention=0 bug, redaction
gaps) in the Rust crate now, or is this document for a future session to
pick up?

### 3. Should I read and incorporate the Go consumer feedback document into the Rust review?

`go-flightrecorder/docs/feedback/new/2026-08-11_real-world-consumer-feedback-from-project-discovery-sdk.md`
(331 lines) contains the consumer feedback that drove the Go project's
entire feature update. It likely contains requirements and use cases that
apply equally to the Rust project. Should I read it and extract
Rust-relevant insights, or keep the feedback document focused on what I
found myself?

---

## Resolution (2026-08-11)

Feedback document delivered. All bugs it found were fixed in the next session.

| Finding                                              | Resolution                                                                 | Commit    |
| ---------------------------------------------------- | -------------------------------------------------------------------------- | --------- |
| Span context blind spot (#1 issue)                   | Implemented in session 8 (`on_new_span`/`on_record`/`on_event` scope walk) | `f6a93e9` |
| capacity=0 retains 1 event                           | Fixed — early return guard in `push()`                                     | `f6a93e9` |
| retention=0 deletes own dump                         | Fixed — `max_files=0` means unlimited                                      | `f6a93e9` |
| `is_sensitive_field` allocation                      | Fixed — zero-alloc `windows()` + `eq_ignore_ascii_case`                    | `f6a93e9` |
| Allocation count (claimed 14-17 by reading)          | Profiled empirically: ~9 allocs/event (after fixes)                        | `34ab131` |
| README "zero non-tracing deps" false claim           | Corrected to "minimal dependencies"                                        | `f6a93e9` |
| Go project doc bugs (options.go:121, FEATURES.md:56) | Different repo — not actionable here                                       | —         |
| All 50 "next things" brainstorm                      | Items picked up by sessions 8–11. Remaining open items in `TODO_LIST.md`.  | —         |
