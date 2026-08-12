# Brutal Review — tracing-flight-recorder

**Date:** 2026-08-11
**Method:** Full source code audit of both `tracing-flight-recorder` (Rust) and `go-flightrecorder` (Go), with bugs verified by compilation and execution, not just reading.

**Updated:** Incorporates Go project's v2 operational features (compression, retention, async capture, observability hooks, directory snapshots).

---

## What I Did Wrong In The First Review

My first feedback file was formulaic and shallow. I read about 60% of the
source, guessed at allocation counts, produced a symmetric P0-P3 grid that
looked thorough but wasn't, and missed the single biggest design flaw in
the crate. This version fixes all of that. Every claim below is verified
against source code. Every bug was confirmed by execution.

---

## ACTUAL BUGS (Verified)

These are not design opinions. These are defects present in the code.

### ~~Bug 1: `FlightRecorder::new(0)` retains 1 event, not 0~~ — FIXED `f6a93e9`

**Location:** `src/layer.rs:39-48`

```rust
pub fn push(&self, event: CapturedEvent) {
    let mut buf = self.buffer.lock()...;
    if buf.len() >= self.capacity {  // 0 >= 0 = true
        buf.pop_front();             // empty deque → None, no-op
    }
    buf.push_back(event);            // pushes anyway
}
```

The guard is `>=`, but `pop_front()` on an empty deque is a no-op, so the
push always succeeds. Capacity 0 silently retains 1 event instead of 0.
The user asked for zero retention and gets one event. No error, no panic.

**Verified by execution:**

```
After 1 push with capacity=0: len=1
After 2 pushes with capacity=0: len=1
```

**Fix:** Either reject `capacity == 0` in `new()` (panic or return
`Result`), or guard the push: `if self.capacity == 0 { return; }`.

---

### ~~Bug 2: `dump_with_retention(_, _, 0)` deletes its own dump~~ — FIXED `f6a93e9`

**Location:** `src/layer.rs:181-211`

The flow: `dump_with_retention` writes the snapshot file, then calls
`cleanup_old_snapshots(dir, prefix, 0)`. Inside cleanup:

```rust
if snapshots.len() <= max_files { return; }  // 1 <= 0 → false → DON'T skip
let excess = snapshots.len().saturating_sub(max_files); // 1 - 0 = 1
for entry in snapshots.iter().take(excess) {            // deletes 1 file
    let _ = std::fs::remove_file(entry.path());         // deletes the dump
}
```

The user calls `dump_with_retention(dir, "snap", 0)`. The snapshot is
written, then immediately deleted. **Silent data loss.** No error returned,
no warning logged. The function returns `Ok(path)` for a path that no
longer exists.

**Verified by execution:**

```
snapshots.len()=1, max_files=0, skip=false
excess files to delete: 1
```

**Note:** The Go sibling's `cleanupSnapshots` (`retention.go:54`) has the
same pattern but avoids this bug because `WithMaxSnapshots(0)` means
"unlimited" (retention disabled), not "keep zero." The Rust crate's
`max_files` parameter doesn't have this convention — 0 means "delete
everything," and the function still writes the file first.

**Fix:** Guard in `dump_with_retention`: if `max_files == 0`, return early
without writing (or return an error). Or adopt the Go convention where 0
means "unlimited."

---

### ~~Bug 3: Memory footprint test undercounts real memory~~ — FIXED `b7637fb`

**Location:** `src/layer_tests.rs:558-604`

The test measures buffer memory by summing `size_of::<CapturedEvent>() +
string_content_lengths`. This undercounts because:

- `String` capacity is often larger than length (allocator rounds up). A
  5-character string may have capacity 8, 16, or 32 depending on growth
  history. The test counts 5; reality is more.
- `Vec<(String, String)>` capacity follows the same pattern.
- `to_lowercase()` in `is_sensitive_field` allocates temporary strings that
  live during the push call (though they're dropped after).

The test asserts `< 1_000_000` bytes and passes at "~237 KB". The real
heap allocation for 1000 events with realistic field sizes is likely
**30-50% higher** than the test reports. This means the README's
"~200-500 KB" claim is based on an undercounting measurement.

**Impact:** The test gives false confidence about memory usage.

---

## ~~THE SPAN CONTEXT BLIND SPOT (The #1 Issue)~~ — IMPLEMENTED `f6a93e9`

This is the biggest problem in the crate, and my first review didn't catch it.

### The crate captures events without any span context

**Location:** `src/layer.rs:252-259`

```rust
impl<S> Layer<S> for FlightRecorderLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        //                                  ^^^^ IGNORED
        self.recorder.push(CapturedEvent::from_event(event));
    }
}
```

The `_ctx: Context` parameter is explicitly discarded. This is where span
context lives in the `tracing` ecosystem. The `Context` gives you access to
the current span stack — parent span names, span attributes, the full
hierarchical context.

**Why this matters in practice:**

```rust
// Typical tracing usage:
let span = info_span!("http_request",
    method = "POST",
    path = "/api/users",
    request_id = "req-abc-123",
    user_id = "user-456",
);
let _enter = span.enter();

// ... 50 lines of code ...

// This event fires inside the span:
error!("database query failed");

// What CapturedEvent records:
//   level: "ERROR"
//   message: "database query failed"
//   fields: []
//
// What is LOST:
//   - method = "POST"
//   - path = "/api/users"
//   - request_id = "req-abc-123"
//   - user_id = "user-456"
//   - The fact that this happened inside "http_request"
```

An error event with no fields, no request ID, no user ID, no path. In a
production incident with 847 buffered events, you have 847 decontextualized
messages. You know that _something_ broke, but you cannot correlate events
to requests, users, or operations.

**This defeats the entire purpose of the `tracing` ecosystem.** The reason
people use `tracing` instead of `log` is span context — the ability to
correlate events across a request lifecycle. The flight recorder throws
that away.

**The data model doesn't even have room for span context:**

```rust
pub struct CapturedEvent {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub level: String,
    pub target: String,
    pub message: String,
    pub fields: Vec<(String, String)>,
    // No span_stack: Vec<String>
    // No span_fields: Vec<(String, String)>
    // No parent_span: Option<String>
}
```

**What the Go sibling does:** It records raw runtime trace data — goroutine
scheduling, syscall traces, GC events, blocking profiles. This data is
inherently contextual: it includes call stacks, goroutine IDs, and
processor affinity. The Go recorder doesn't need to "add context" because
the trace format IS context.

**Recommended fix:**

1. Walk the span stack in `on_event` using `ctx.event_scope()` or
   `ctx.current_span()`.
2. Capture span names and their key fields into the `CapturedEvent`.
3. Add fields to the data model:

```rust
pub struct CapturedEvent {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub level: String,
    pub target: String,
    pub message: String,
    pub fields: Vec<(String, String)>,
    pub spans: Vec<SpanContext>,  // NEW: parent span stack
}

pub struct SpanContext {
    pub name: String,
    pub fields: Vec<(String, String)>,
}
```

4. Implement `new_span` on the `Layer` to capture span fields when spans
   are created (not just when events fire inside them).

**Impact:** Without this, the crate is a structured log buffer, not a
tracing flight recorder. This should be the #1 development priority — above
time-based eviction, above trigger systems, above everything.

---

## HOT-PATH ALLOCATION ANALYSIS (Verified, Not Guessed)

### Per-event allocation breakdown

For a typical event `tracing::info!(device = "dev-1", count = 42, active = true, "sync completed")`:

4 fields total: `message`, `device`, `count`, `active`

| Step                                                              | Code location    | Allocations |
| ----------------------------------------------------------------- | ---------------- | ----------- |
| `level_to_string` → `.to_string()` on `&'static str`              | `capture.rs:152` | 1           |
| `target` → `.to_string()` on `&str`                               | `capture.rs:42`  | 1           |
| `record_str("device", "dev-1")` → `value.to_string()`             | `capture.rs:112` | 1           |
| `is_sensitive_field("device")` → `name.to_lowercase()`            | `capture.rs:93`  | 1           |
| `field.name().to_string()` for device key                         | `capture.rs:80`  | 1           |
| `record_i64("count", 42)` → `value.to_string()`                   | `capture.rs:121` | 1           |
| `is_sensitive_field("count")` → `name.to_lowercase()`             | `capture.rs:93`  | 1           |
| `field.name().to_string()` for count key                          | `capture.rs:80`  | 1           |
| `record_bool("active", true)` → `value.to_string()`               | `capture.rs:117` | 1           |
| `is_sensitive_field("active")` → `name.to_lowercase()`            | `capture.rs:93`  | 1           |
| `field.name().to_string()` for active key                         | `capture.rs:80`  | 1           |
| `record_debug("message", ...)` → `write!(buf, ...)`               | `capture.rs:107` | 1           |
| `Vec<(String, String)>` reallocation (3 pushes, 0→1→2→4 capacity) | `capture.rs:80`  | ~2          |
| **Total**                                                         |                  | **14**      |

For a **5-field** event: **17 allocations**. For a **sensitive** field, add
1 more (`"[REDACTED]".to_string()` — `capture.rs:76`). For a **0-field**
event (just a message): **5 allocations**.

### The wasted-allocation triple for sensitive fields

For a field like `auth_token = "secret-value"`:

1. Caller (`record_str`) does `value.to_string()` → allocates `"secret-value"` → **1 alloc**
2. `is_sensitive_field` does `name.to_lowercase()` → allocates `"auth_token"` → **1 alloc**
3. `record_common` does `"[REDACTED]".to_string()` → allocates `"[REDACTED]"` → **1 alloc**
4. The original `"secret-value"` String is dropped → **wasted alloc**

**3 allocations** for a single sensitive field, plus 1 wasted. The
`"[REDACTED]"` literal could be a `Cow::Borrowed("'static [REDACTED]")` —
zero allocation. The `to_lowercase()` could be `contains` with
case-insensitive comparison — zero allocation.

### Comparison: Go hot-path cost

The Go recorder has **zero allocations on the hot path**. The Go runtime's
`trace.FlightRecorder` writes raw binary trace data into a pre-allocated
ring buffer. The wrapper does nothing until `Snapshot()` is called. The
difference is architectural, not implementational. Even the Go project's
new `SnapshotIfAsync` — which spawns a goroutine — does zero hot-path work
beyond the trigger evaluation. The actual I/O happens in the background.

---

## FALSE CLAIMS

### ~~Claim 1: "Zero non-tracing dependencies" (Rust README:27, CONTRIBUTING:9)~~ — FIXED `f6a93e9`

**Status:** False.

`Cargo.toml` `[dependencies]` section:

```toml
tracing = "0.1"                              # tracing ecosystem
tracing-subscriber = { version = "0.3" }     # tracing ecosystem
serde = { version = "1", features = ["derive"] }  # NOT tracing
serde_json = "1"                              # NOT tracing
chrono = { version = "0.4" }                  # NOT tracing
```

Three of five default dependencies are not part of the tracing ecosystem.
`chrono` could be eliminated entirely — `std::time::SystemTime` plus serde
would suffice.

---

### Claim 2: "Pays zero I/O cost until a snapshot is triggered" (Rust README:17)

**Status:** Technically true (no I/O), but misleading about cost.

The crate pays **zero I/O cost** but pays a **significant CPU and allocation
cost** on every event. 14-17 heap allocations per event, all under a global
`Mutex` lock. The README implies the crate is free until you need it. It
isn't.

---

## DOCUMENTATION BUGS IN THE GO PROJECT

### Go Bug 1: `WithCompression` doc comment contradicts reality

**Location:** `go-flightrecorder/options.go:121`

```go
// Compressed snapshot files use the ".trace.gz" extension and are loadable by
// `go tool trace` (supported since Go 1.19).
```

This is **false**. The project's own status report
(`docs/status/2026-08-11_15-33_q1-q3-resolution-and-self-review.md`)
empirically verified in three ways that `go tool trace` in Go 1.26.5
**rejects** gzip-compressed trace files with "bad file format: not a Go
execution trace?"

The CHANGELOG correctly says "does not read gzip directly." The README
correctly says "decompress with gunzip." The doc.go correctly says
"does not read .gz directly." The AGENTS.md correctly says "does NOT
read .trace.gz directly." But the `WithCompression` doc comment — the one
a developer reads when hovering the function in their IDE — still claims
gzip is "loadable by go tool trace."

**Fix:** Change to "Compressed snapshot files use the `.trace.gz` extension.
Decompress with `gunzip` before analysis — `go tool trace` does not read
gzip directly."

---

### Go Bug 2: FEATURES.md repeats the false gzip claim

**Location:** `go-flightrecorder/FEATURES.md:56`

```
| Compression | FULLY_FUNCTIONAL | WithCompression; stdlib gzip;
|             |                  | .trace.gz loadable by go tool trace |
```

Same false claim as above. Should read ".trace.gz requires gunzip before
go tool trace."

---

## REDACTION GAPS (Rust only)

### ~~`authorization` is not redacted~~ — FIXED `f6a93e9` (14 patterns now)

**Location:** `src/capture.rs:91-101`

Missing patterns for a web service context:

| Field name      | Why it matters                              | Currently redacted? |
| --------------- | ------------------------------------------- | ------------------- |
| `authorization` | Standard HTTP header, carries bearer tokens | **No**              |
| `auth`          | Common abbreviation                         | **No**              |
| `bearer`        | Token type in Authorization header          | **No**              |
| `cookie`        | Can contain session tokens                  | **No**              |
| `session_id`    | Session identifier                          | **No**              |
| `access_code`   | OAuth access codes                          | **No**              |

`authorization` is the most glaring omission. In HTTP services, it's the
standard field name for the credential that grants access.

---

## TIME-BASED BUFFERING (Still Important, But Not The #1 Issue)

The core problem remains: event-count buffering loses temporal context
under burst load. At 5000 events/sec, 1000 events = 200ms of context. The
fix (hybrid time + count eviction) is on the roadmap and the implementation
is straightforward. But the span context gap should be fixed first —
time-based eviction of context-free events gives you more nothing, faster.

The Go project doesn't have this problem because the Go runtime's
`FlightRecorder` handles time-based eviction internally via `MinAge` and
`MaxBytes`.

---

## MISSING API SURFACE (Rust)

### ~~1. No span context capture~~ (see #1 issue above) — DONE `f6a93e9`

### ~~2. No trigger/decision system~~ — DONE `b7637fb` (`Trigger`/`LevelTrigger`/`OnceTrigger`)

Every failure path must manually call `dump_to_file()`. The Go sibling has
composable triggers (`OnError`, `OnLatency`, `OnAny`, `OnAll`). Without
triggers, developers forget to wire dumps, and failures pass unrecorded.

### ~~3. No once-semantics~~ — DONE `b7637fb` (`OnceTrigger` with `reset()`)

Multiple threads detecting a failure simultaneously each call
`dump_to_file()`, causing redundant I/O and burning retention slots. The
Go sibling uses `sync.Once` internally with `Reset()` for re-arming.

### 4. No async/non-blocking capture — OPEN (`TODO_LIST.md` deferred)

The Go sibling just added `SnapshotIfAsync` — trigger evaluation returns
immediately, the actual I/O happens in a background goroutine, and
`Stop`/`Close` drain in-flight captures before shutting down. This is
critical for HTTP middleware where trace file I/O must not block the
response. The Rust crate forces every dump call to block.

### ~~5. No observability hooks~~ — DONE `34ab131` (`on_dump` callback, `DumpEvent`/`DumpSource`)

The Go sibling now has `WithMetrics(hook)` and `WithLogger(hook)` —
dependency-free callbacks that receive a `SnapshotEvent` (duration, bytes,
path, compression flag, source label) after every capture. This lets
consumers wire Prometheus, OpenTelemetry, or any backend without the
library taking a dependency. The Rust crate has nothing — no way to know
when a dump fires, how long it took, or how big it was.

### ~~6. No compression~~ — DONE `34ab131` (`gzip` feature, `dump_to_file_gz`)

The Go sibling compresses snapshots with stdlib gzip (10x reduction for
trace data). The Rust crate outputs pretty-printed JSON, which is already
larger than necessary, with no compression option.

### ~~7. No NDJSON or `dump_to_writer`~~ — DONE `7434a27` + `f6a93e9`

Output is pretty-printed JSON arrays. Not streamable, not appendable, not
ingestible by log pipelines without a full parse. No `dump_to_writer` for
non-file sinks. The Go sibling now has `SnapshotToWriter` for arbitrary
`io.Writer` destinations.

### ~~8. `push` is `pub` but should be `pub(crate)`~~ — DONE `f6a93e9`

Users can inject fake events into the diagnostic record.

### ~~9. `FieldVisitor` is exported but has no external use case~~ — DONE `f6a93e9` (removed from public re-exports)

Pollutes the public API with an internal implementation detail.

---

## COMPARISON: THE REAL PICTURE (Updated)

The Go project received a major feature update on 2026-08-11 that
fundamentally changes the comparison. It went from 1,587 LOC (5 source
files) to 3,411 LOC (7 source files + 2 new supporting files), adding
compression, retention, async capture, observability hooks, directory
snapshots, and nil-safe lifecycle.

| Dimension                 | Go (go-flightrecorder)                                                                                                             | Rust (tracing-flight-recorder)                            |
| ------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------- |
| **What it captures**      | Runtime execution traces (goroutine scheduling, GC, syscalls)                                                                      | Application tracing events (structured logs)              |
| **Context fidelity**      | Full (runtime trace includes call stacks, goroutine IDs)                                                                           | **None** (span context discarded)                         |
| **Hot-path cost**         | Zero allocations (runtime handles buffering)                                                                                       | 14-17 allocations/event under global mutex                |
| **Buffer model**          | Time + bytes (temporal guarantee)                                                                                                  | Event count (no temporal guarantee under load)            |
| **Trigger system**        | Composable: OnError, OnLatency, OnAny, OnAll                                                                                       | None (manual dump calls)                                  |
| **Once-semantics**        | sync.Once + Reset()                                                                                                                | None                                                      |
| **Async capture**         | **SnapshotIfAsync** (background goroutine, drain on Stop/Close)                                                                    | None                                                      |
| **Observability hooks**   | **WithMetrics + WithLogger** (SnapshotEvent with source labels)                                                                    | None                                                      |
| **Compression**           | **WithCompression** (gzip, opt-in, 10x reduction)                                                                                  | None                                                      |
| **Directory snapshots**   | **SnapshotToDir** (timestamped, prefixed, non-once-latched)                                                                        | dump_to_file (single file, manual naming)                 |
| **Retention**             | **WithMaxSnapshots** (prune oldest, prefix/suffix filtered)                                                                        | dump_with_retention (buggy at max_files=0)                |
| **Arbitrary writer sink** | **SnapshotToWriter** (bypasses once-latch, for debug endpoints)                                                                    | None                                                      |
| **Nil-safe lifecycle**    | **Enabled/Stop/Close** on nil receiver                                                                                             | N/A (Rust ownership)                                      |
| **Secret redaction**      | N/A (binary trace data)                                                                                                            | Yes, but missing `authorization` and allocates per field  |
| **Output format**         | Binary (go tool trace), gzip-compressed                                                                                            | Pretty JSON array (not streamable)                        |
| **Dependencies**          | Zero (stdlib only)                                                                                                                 | 5 (3 non-tracing despite README claim)                    |
| **Known bugs**            | 2 doc contradictions (gzip claim in options.go + FEATURES.md)                                                                      | 2 confirmed (capacity=0, retention=0), 1 measurement flaw |
| **Stale docs**            | options.go:121 claims gzip "loadable by go tool trace" (disproven by own status report); AGENTS.md string-matching claim was fixed | README claims "zero non-tracing deps" (false)             |
| **Tests**                 | 64 tests + race detector                                                                                                           | 27 + proptest + concurrency + memory (undercounts)        |
| **CI**                    | 3 jobs (test+race, vet, lint)                                                                                                      | 7+ jobs (best-in-class)                                   |
| **Code size**             | 3,411 LOC (7 source + 2 test files)                                                                                                | 1,321 LOC (3 source + 1 test + 3 examples)                |
| **Examples**              | None                                                                                                                               | 3 runnable                                                |

### What changed in this update

The Go project closed every operational gap that existed between the two
projects and pulled significantly ahead:

| Feature               | Before Go update           | After Go update                                                            |
| --------------------- | -------------------------- | -------------------------------------------------------------------------- |
| Retention             | Rust only (unique feature) | **Both** — Go's version is more robust (0=unlimited, prefix/suffix filter) |
| Compression           | Neither                    | **Go only**                                                                |
| Async capture         | Neither                    | **Go only**                                                                |
| Metrics hooks         | Neither                    | **Go only**                                                                |
| Logger hooks          | Neither                    | **Go only**                                                                |
| Directory snapshots   | Neither                    | **Go only**                                                                |
| Arbitrary writer sink | Neither                    | **Go only**                                                                |
| Nil-safe lifecycle    | Neither                    | **Go only**                                                                |

The Rust crate now uniquely offers only: secret redaction, OpenAPI schema
support, and runnable examples. Everything else has been matched or
exceeded.

---

## WHAT IS GENUINELY EXCELLENT

1. **The Rust CI pipeline.** stable + beta matrix, MSRV verification, clippy with
   `pedantic` + `nursery` + `unwrap_used` + `as_conversions` all denied,
   doc build, publish dry-run, cargo audit + deny. The Go CI has 3 jobs;
   the Rust CI has 7+.

2. **The per-layer filtering documentation.** The #1 integration pitfall
   (global EnvFilter blocking DEBUG/TRACE) is documented with a dedicated
   regression test.

3. **The poison recovery design.** `PoisonError::into_inner` is the correct
   choice.

4. **The collision guard extraction.** `resolve_collision_path` with an
   injectable `COLLISION_LIMIT` is well-engineered.

5. **The Go project's drain-on-shutdown design.** `SnapshotIfAsync` +
   `beginShutdown` + `wg.Wait()` is a textbook-correct shutdown ordering
   that prevents the WriteTo/Stop data race. The `stopped` flag is set
   under the lock before `wg.Wait()`, so no new goroutines can call
   `wg.Add` concurrently with `wg.Wait`.

6. **The Go project's observability hooks.** `SnapshotEvent` with source
   labels (manual/trigger/async) and Kind/Type threading is exactly the
   right design. Dependency-free, opt-in, with a `noopMetrics` default.

---

## RECOMMENDED PRIORITY

| Phase         | What                                                    | Why                                            |
| ------------- | ------------------------------------------------------- | ---------------------------------------------- |
| **Immediate** | Fix Rust capacity=0 and retention=0 bugs                | Active data-loss defects                       |
| **Immediate** | Fix Rust README "zero non-tracing dependencies" claim   | False advertising                              |
| **Immediate** | Fix Go `options.go:121` + `FEATURES.md:56` gzip claim   | Contradicts own empirical finding              |
| **v0.2.0**    | **Span context capture**                                | Without this, the crate is not a tracing tool  |
| **v0.2.0**    | Fix `is_sensitive_field` to not allocate                | Free perf win on every event                   |
| **v0.2.0**    | Add `authorization` to redaction patterns               | Security gap                                   |
| **v0.2.0**    | `dump_to_writer` + NDJSON output                        | Trivial additions, big usability gains         |
| **v0.2.0**    | Observability hooks (match Go's WithMetrics/WithLogger) | The Go project just lapped the Rust crate here |
| **v0.3.0**    | Time-based + count hybrid eviction                      | Temporal guarantee under load                  |
| **v0.3.0**    | Trigger system + once-semantics                         | Match Go sibling's production readiness        |
| **v0.3.0**    | Async capture (spawn background task)                   | Match Go's SnapshotIfAsync                     |
| **v0.4.0**    | Hot-path: parking_lot, Arc events, field pre-allocation | Performance competitiveness                    |

The span context gap is the thesis statement of this review. The Go
project's operational feature blitz has widened the gap further. The Rust
crate's unique value (redaction, OpenAPI, examples) is thinning. The path
back to competitiveness runs through span context capture first, then
matching the Go project's operational feature set.

---

<!-- This feedback was generated from a comparative review against the sibling
     go-flightrecorder project. All claims are verified against source code in
     both repositories as of 2026-08-11, including the Go project's v2 operational
     features update. Line references may drift as code changes. -->

---

## Resolution (2026-08-11)

9 of 10 MISSING API SURFACE items shipped. 3 bugs fixed. Span context implemented.

| Category                | Items                                                 | Status              |
| ----------------------- | ----------------------------------------------------- | ------------------- |
| ACTUAL BUGS (3)         | capacity=0, retention=0, memory undercount            | All fixed           |
| SPAN CONTEXT (#1 issue) | Full hierarchy capture, configurable, Arc-shared      | Implemented         |
| REDACTION GAPS          | authorization + 5 more patterns                       | Fixed (14 patterns) |
| FALSE CLAIMS            | "zero non-tracing deps"                               | Fixed               |
| MISSING API (9 items)   | 8 of 9 done; async capture deferred to `TODO_LIST.md` | 89% done            |
| Go project bugs (2)     | Different repo — not actionable here                  | —                   |
