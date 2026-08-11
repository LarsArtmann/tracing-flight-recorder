# External Feedback — tracing-flight-recorder

**Date:** 2026-08-11
**Source:** Comparative review against sibling project `go-flightrecorder`
**Scope:** Architecture, performance, API design, and feature gaps
**Severity model:** P0 (design flaw) > P1 (significant gap) > P2 (improvement) > P3 (polish)

---

## Executive Summary

The crate is well-engineered for what it does: the ring buffer works, the
`tracing` integration is correct, the test suite is thorough (27 unit + 4
doctests + proptest + concurrency stress), and the CI pipeline is
best-in-class for a crate this size. Secret redaction is a genuinely nice
touch. The release infrastructure is solid.

**But the crate has a fundamental architectural mismatch with its own stated
purpose.** It calls itself a *flight recorder* — a tool that captures "the
last N seconds of context" — yet its buffer unit is event count, not time.
Under burst load (exactly when failures happen), 1000 events can cover
sub-second context. The crate's own inspiration (Go's `trace.FlightRecorder`)
uses time + bytes. This is the single biggest issue, and everything else
flows from it.

The Go sibling project (`go-flightrecorder`) is in many ways the more mature
design despite being simpler: it has composable triggers, once-semantics,
time-based buffering, and zero allocations on the hot path (the runtime does
the work). The Rust crate should learn from its sibling.

---

## P0 — Design Flaws

### 1. Event-count buffer is the wrong abstraction for a flight recorder

**The core problem.** The entire purpose of a flight recorder is temporal:
"give me the context leading up to the failure." That is a *time* question.
The crate answers a different question: "give me the last N things that
happened." Under burst load — which is exactly when failures occur — 1000
events might cover 0.2 seconds. The README itself admits a 5x variance
("20-100 seconds at 10-50 events/sec"). At 5000 events/sec (realistic for a
busy Rust service), you get **200 milliseconds** of context. Useless.

**Why this matters.** A flight recorder that loses temporal context under
load is not a flight recorder — it is a bounded log buffer. The name
promises something the implementation does not deliver under the conditions
where it matters most.

**The Go sibling gets this right.** Its buffer unit is `MinAge` (time) +
`MaxBytes` (space). High load = denser data, same time window. That is the
correct behavior for a diagnostic tool. The Go runtime handles the
complexity of byte-rate-based eviction internally; the wrapper just
configures the window.

**The roadmap acknowledges this** (Theme #1: "Time-windowed capture"), but
it is filed as a P3 long-term spike alongside output formats and framework
ergonomics. This is not a P3. **This is the defining architectural decision
the crate got wrong, and it should be the #1 priority for v0.2.0.**

**Recommended fix:** Hybrid model — `max_events OR max_age, whichever
fills first`. This gives:
- A hard memory ceiling (event count bounds allocations)
- A temporal floor (max age guarantees minimum context window)
- Honest marketing ("captures the last N seconds OR M events, whichever
  fills first")

The implementation is straightforward: check the timestamp of the oldest
event on every `push`. If `now - oldest.timestamp > max_age`, evict it.
This is O(1) on a `VecDeque` with `front()`/`pop_front()`.

**Impact:** The difference between "useful diagnostic tool" and "bounded
log buffer that occasionally captures enough context by accident."

---

### 2. Hot path is allocation-heavy under a global mutex

**The problem.** Every single `tracing` event that passes the layer filter
takes this path:

1. `FlightRecorderLayer::on_event` → `CapturedEvent::from_event`
2. `from_event` allocates: `String` for level, `String` for target,
   `String` for message, `Vec<(String, String)>` for fields, plus a
   `String` per field value (all field visitors call `.to_string()`)
3. All of this happens under `Arc<Mutex<VecDeque>>` — one global lock
4. Then `push` potentially calls `pop_front` (another operation under lock)

For a single event with 5 fields, that is roughly **10 heap allocations**
on the hot path, all serialized through a single mutex. On a busy service
emitting 1000+ events/sec, this is a measurable performance tax — and the
user pays it *continuously*, even if the buffer is never dumped.

**The Go sibling pays zero allocation cost on the hot path.** The Go
runtime's flight recorder writes raw trace bytes into a pre-allocated
ring buffer internally. The wrapper does nothing until `Snapshot` is
called. Zero overhead until you need it.

**This is acknowledged** in the roadmap (Theme #2: "Hot-path performance"),
but again filed as P3. For a crate whose value proposition is "continuously
buffer without paying I/O cost," the CPU/allocation cost of continuous
buffering is directly relevant to the value proposition.

**Recommended improvements (ranked by effort/impact):**

| Fix | Effort | Impact |
|-----|--------|--------|
| Replace `std::sync::Mutex` with `parking_lot::Mutex` (no syscall on uncontended lock) | Low | Medium — reduces lock overhead but doesn't solve allocation |
| Pre-allocate field capacity in `FieldVisitor` (avoid Vec reallocations) | Low | Low — per-event allocs still dominate |
| Use `&'static str` or `Cow<'static, str>` for level (5 known values) | Low | Low |
| Object pool / slab allocator for `CapturedEvent` (reuse allocations) | Medium | High — eliminates per-event heap pressure |
| Lock-free ring buffer (`crossbeam-queue` or custom `AtomicPtr`-based) | High | High — eliminates lock contention |
| Async channel + background writer (events queued, not serialized inline) | High | High — decouples tracing hot path from buffer management |

**Minimum viable improvement for v0.2:** `parking_lot::Mutex` + field
capacity pre-allocation + `Cow<'static, str>` for level. This is ~30
minutes of work and cuts the allocation count roughly in half.

---

## P1 — Significant Feature Gaps

### 3. No trigger/decision system

**The gap.** The Go sibling has a rich composable trigger system:
`OnLatency(threshold)`, `OnError()`, `OnErrorOrLatency(threshold)`,
`OnAlways()`, `OnAny(triggers...)` (OR), `OnAll(triggers...)` (AND).
These are the decision layer that turns "I have a flight recorder" into
"my flight recorder automatically captures exactly when it should."

The Rust crate has **nothing**. The caller must manually call
`dump_to_file()` on every failure path. In a real codebase, this means:

1. Every error handler needs `recorder.dump_to_file(path).ok()`
2. Every middleware needs to wire the dump manually
3. Developers forget — and the dump never fires

This is the difference between "a buffer you have to remember to flush"
and "a diagnostic tool that thinks for itself."

**Recommended design for Rust:**

```rust
pub trait Trigger: Send + Sync {
    fn should_dump(&self, context: &TriggerContext) -> bool;
}

pub struct TriggerContext<'a> {
    pub duration: Option<Duration>,
    pub error: Option<&'a dyn std::error::Error>,
    pub status_code: Option<u16>,
    pub metadata: &'a [(&'a str, &'a str)],
}
```

Then `FlightRecorder::dump_if(trigger, context, path)` or a
`tower` middleware that auto-evaluates triggers on response.

**Impact:** Without this, the crate is a building block, not a solution.
The Go sibling ships as a solution.

---

### 4. No once-semantics (concurrent dump races)

**The gap.** The Go sibling uses `sync.Once` internally: when multiple
goroutines detect a problem simultaneously (common in cascading failures),
only the **first** `Snapshot()` call writes. All subsequent calls are
silent no-ops. `Reset()` re-arms the latch for subsequent captures.

The Rust crate has **no such guard**. If 5 threads all detect errors and
call `dump_to_file()` concurrently:
- 5 files get written (or the same file gets overwritten 5 times)
- 5x the I/O cost on the failure path
- The `dump_with_retention` collision counter increments 5 times,
  consuming retention slots

**Recommended fix:** An `AtomicBool` flag inside `FlightRecorder`:
```rust
fn dump_once(&self, ...) -> Result<()> {
    let already_dumped = self.dumped
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::Relaxed)
        .is_err();
    if already_dumped { return Ok(()); }
    // ... actual dump ...
}
```
Plus a `reset_dump_flag()` method for re-arming.

**Impact:** Prevents I/O storms during cascading failures. This is a
production-readiness concern.

---

### 5. Output format: pretty-printed JSON array is the wrong default

**The problem.** `dump_to_json()` calls `serde_json::to_string_pretty()`,
which:
- Adds ~20-30% whitespace overhead (indentation, newlines)
- Serializes the entire buffer into one `String` in memory before writing
- Produces a JSON array (requires a parser to consume; not streamable)

For a diagnostic dump, this is suboptimal in three ways:

| Issue | Impact |
|-------|--------|
| Pretty-printed | Larger files, slower serialization, no ingestion benefit |
| Full in-memory serialization | 1000 events = ~237 KB String allocated upfront. At 100K events, this would be ~24 MB allocated just for serialization |
| JSON array format | Not streamable, not appendable, requires full parse to consume |

**Recommended alternatives (ranked):**

1. **NDJSON (newline-delimited JSON)** — one JSON object per line. This is
   the industry standard for log/event data. Streamable, appendable,
   ingestible by every log pipeline (Loki, Elastic, Datadog, jq). Should
   be the default.

2. **Compact JSON** — `to_string()` not `to_string_pretty()`. Smaller,
   faster. Pretty-print only on opt-in.

3. **Streaming write** — serialize event-by-event to the `Write` sink
   instead of building a full `String`. Avoids the double-memory pattern
   (buffer + serialized string).

**Impact:** NDJSON would make the crate instantly compatible with
existing log tooling. This is a usability multiplier.

---

### 6. `snapshot()` clones the entire buffer

**The problem.** `FlightRecorder::snapshot()` clones every `CapturedEvent`
out of the `VecDeque` into a new `Vec`. For 1000 events at ~237 bytes each,
that is a ~237 KB allocation + 1000 individual `clone()` calls (each
cloning 4-5 `String`s).

**Why this matters:** `snapshot()` is called on the failure path — the
same path where you are trying to diagnose a problem. Adding a quarter-meg
allocation + clone storm to the failure path is counterproductive. The
user wanted to debug a problem, not create a new one.

**Recommended fix:** Return an iterator or a `Vec<&CapturedEvent>` (borrowed
snapshot under lock scope). Or use `Arc<CapturedEvent>` in the buffer so
clones are cheap (reference count bump, not deep copy).

**Note:** `Arc<CapturedEvent>` in the buffer would also make the hot path
cheaper — `push(Arc<CapturedEvent>)` would not need to own the event, and
eviction (`pop_front`) just decrements a refcount.

**Impact:** Reduces failure-path latency. The `Arc` approach also reduces
hot-path cost.

---

## P2 — Improvements

### 7. Secret redaction is ASCII-only

`is_sensitive_field` does `name.to_lowercase()` then substring matching.
This means:

- `café_token` → **caught** (the ASCII substring `token` matches)
- `pässwörd` → **not caught** (the non-ASCII characters break substring matching against `password`)
- `Ｓｅｃｒｅｔ` (fullwidth Unicode) → **not caught**

This is documented in a test (`unicode_field_name_redaction_is_ascii_only`),
so it is a **known limitation**, not a bug. But it is worth noting that:

1. The documentation is in a test name, not in the public API docs
2. A Rust service handling internationalized field names (e.g., from JSON
   APIs with non-ASCII keys) could leak secrets into the ring buffer
3. The fix is simple: use `unicode-normalization` to NFKD-normalize before
   matching, or document the limitation in the doc comment on
   `is_sensitive_field` / `CapturedEvent::from_event`

**Impact:** Low-moderate. Most field names are ASCII, but a security tool
that silently fails on non-ASCII input is a footgun.

---

### 8. Missing edge case handling

Several edge cases are untested or have undefined behavior:

| Edge case | Current behavior | Risk |
|-----------|-----------------|------|
| `FlightRecorder::new(0)` | `push` evicts immediately; buffer is always empty. No error. | Silent failure — user thinks they're recording but nothing is retained |
| `dump_with_retention(dir, prefix, 0)` | Writes one file, then deletes all files matching prefix (including the one just written) | Data loss — the dump is immediately pruned |
| `dump_with_retention(dir, prefix, 1)` | Writes file, then deletes all but 1. But which 1? `sort_by_key` on mtime — the newest. OK. | Works but subtle |
| `dump_to_file` with read-only dir | Returns `io::Error` propagated to caller | Fine, but no typed error |
| `is_sensitive_field("")` | `"".to_lowercase()` → empty string → no substring matches → not redacted | Fine, but untested |
| `FieldVisitor` with `i128`/`u128` values | Tested for capture, but not for edge values (`i128::MAX`, `u128::MAX`) | Overflow in `to_string()`? (No — `Display` handles it, but untested) |

**Recommended fixes:**
- `new(0)` should panic or return `Result` (capacity of 0 is never useful)
- `dump_with_retention(dir, prefix, 0)` should return early or log a warning
- Add tests for the above edge cases

**Impact:** Low individually, but these are the kind of edge cases that
cause silent data loss in production.

---

### 9. No `dump_to_writer` for streaming/non-file sinks

The crate has `dump_to_json()` (returns `String`) and `dump_to_file()`
(writes to `Path`). But there is no `dump_to_writer(&mut impl Write)`.
This means:

- Cannot dump to `stdout()` / `stderr()`
- Cannot dump to a network socket
- Cannot dump to a compressed writer (`flate2::GzEncoder`)
- Cannot dump to an in-memory buffer without going through `String`

**Recommended fix:** Add `pub fn dump_to_writer(&self, writer: &mut impl std::io::Write) -> std::io::Result<()>`.

**Impact:** Medium. This is a trivial addition that significantly expands
the output surface.

---

### 10. No metadata in dumps

The dump is a bare JSON array of events. There is no metadata about the
dump itself:

- When was the dump triggered? (timestamp)
- What triggered it? (error message, trigger name, panic info)
- How many events are in the buffer vs. capacity?
- What time span does the dump cover? (oldest event timestamp to newest)
- What version of the crate produced this dump?

**Recommended format (wrapping the array in an envelope):**

```json
{
  "dump_timestamp": "2026-08-11T12:00:00Z",
  "buffer_span": { "oldest": "...", "newest": "..." },
  "event_count": 847,
  "capacity": 1000,
  "crate_version": "0.1.1",
  "trigger": "manual",
  "events": [ ... ]
}
```

**Impact:** Medium. A dump without context is just data. A dump with
metadata is an incident report.

---

## P3 — Polish

### 11. No `Clone` bound on `FlightRecorderLayer`

`FlightRecorderLayer` is not `Clone`. If a user wants to attach the same
recorder to multiple subscribers (e.g., in a test harness with multiple
isolated subscribers), they must manually reconstruct the layer each time.
Since `FlightRecorder` is already `Clone` (cheap, shares the `Arc`), the
layer should be too.

**Fix:** `#[derive(Clone)]` on `FlightRecorderLayer`. One line.

---

### 12. `level_to_string` allocates on every event

`level_to_string` returns `String` via `.to_string()` on a `&'static str`.
Since there are exactly 5 levels (`ERROR`, `WARN`, `INFO`, `DEBUG`,
`TRACE`), this could be `&'static str` — zero allocation.

Alternatively, store the `tracing::Level` directly (it is `Copy` + `Serialize`)
and let serde handle the string conversion at dump time.

**Impact:** Low (5 `String` allocations per event become zero), but it is
a free win.

---

### 13. No `Display` impl on `FlightRecorder`

The manual `Debug` impl shows `capacity` and `len`, which is good. But
there is no `Display` impl for user-facing output (e.g., logging the
recorder state at startup). Minor ergonomic gap.

---

### 14. `dump_with_retention` filename format uses local conventions, not RFC 3339

The timestamp format is `%Y%m%dT%H%M%S` (e.g., `20260811T120000`). This is
compact but:
- Not sortable as a string across year boundaries (it is — same format)
- Not RFC 3339 (which would be `2026-08-11T12:00:00Z`)
- No timezone indicator (uses `Utc::now()` but doesn't include `Z` in the
  filename)

Adding `Z` or `-` separators would make the filenames more self-documenting
and standards-compliant.

**Impact:** Cosmetic.

---

## What The Crate Gets Right (Credit Where Due)

To balance the critique, here is what is genuinely excellent:

| Area | Assessment |
|------|-----------|
| **Test quality** | End-to-end tests with real `tracing` subscribers (not mocks), proptest for eviction invariant, 8-thread concurrency stress, memory footprint measurement, poison recovery test. This is above the bar for most crates. |
| **CI pipeline** | stable + beta matrix, MSRV verification, clippy with insanely strict config (`pedantic` + `nursery` + `unwrap_used` + `as_conversions` denied), fmt check, doc build, publish dry-run, cargo audit + deny. Best-in-class for a crate this size. |
| **Secret redaction** | Genuinely useful. Over-redaction is the right default for a security feature. The substring-match approach is pragmatic. |
| **Retention pruning** | `dump_with_retention` with same-second collision guard and oldest-file pruning is a production feature. The collision limit (9999) with extracted testable function is well-engineered. |
| **OpenAPI support** | `utoipa::ToSchema` behind a feature flag is a thoughtful integration for services that expose diagnostics endpoints. |
| **Per-layer filtering documentation** | The crate's #1 integration pitfall (global `EnvFilter` blocking DEBUG/TRACE) is documented extensively with a regression test. This saves users hours of confusion. |
| **Poison recovery** | `PoisonError::into_inner` is the correct design choice — a panicked thread should not kill the recorder. |
| **Release infrastructure** | `publish.yml` with idempotency guard, `release.toml`, `docs/RELEASE.md` runbook, `deny.toml`. Professional-grade. |

---

## Comparison Summary: Sibling Projects

| Dimension | `go-flightrecorder` (Go) | `tracing-flight-recorder` (Rust) | Winner |
|-----------|--------------------------|-----------------------------------|--------|
| **Buffer unit** | Time + bytes (adaptive) | Event count (fixed) | **Go** — temporal guarantee |
| **Hot-path cost** | Zero (runtime does the work) | ~10 allocations/event + mutex lock | **Go** — by a mile |
| **Trigger system** | Composable: `OnError`, `OnLatency`, `OnAny`, `OnAll` | None | **Go** — automatic vs manual |
| **Once-semantics** | `sync.Once` + `Reset()` | None | **Go** — race-safe |
| **Secret redaction** | N/A (binary trace data) | Automatic, 8 patterns | **Rust** — unique feature |
| **Retention pruning** | None | `dump_with_retention` with collision guard | **Rust** — unique feature |
| **Output format** | Binary (for `go tool trace`) | Pretty JSON array | Tie — both fit their ecosystem |
| **OpenAPI/schema** | No | `utoipa::ToSchema` | **Rust** — unique feature |
| **Test quality** | 27 tests + `-race` | 27 + proptest + concurrency + memory | **Tie** — both excellent |
| **Dependencies** | Zero (stdlib only) | 5 (tracing ecosystem) | **Go** — but Rust needs them |
| **Examples** | None (README inline) | 3 runnable examples | **Rust** |
| **Context cancellation** | Pre-write `ctx.Done()` check | N/A (synchronous) | **Go** |

**Bottom line:** The Go sibling wins on the *core flight-recorder properties*
(buffer model, hot-path cost, trigger system, once-semantics). The Rust
crate wins on *ecosystem integration* (redaction, retention, OpenAPI,
examples). The Rust crate should prioritize adopting the Go sibling's core
properties — time-based eviction, trigger system, and once-semantics — to
match the quality of its integration features.

---

## Recommended Priority Order

If I were the maintainer, here is the order I would work through:

| Priority | Item | Effort | Impact on "is this a real flight recorder?" |
|----------|------|--------|---------------------------------------------|
| **v0.2.0** | Time-based + count hybrid eviction (P0 #1) | Medium | **Critical** — fixes the core design flaw |
| **v0.2.0** | Trigger system (P1 #3) | Medium | **High** — turns buffer into tool |
| **v0.2.0** | Once-semantics (P1 #4) | Low | **High** — production safety |
| **v0.2.0** | NDJSON output format (P1 #5) | Low | **Medium** — tooling integration |
| **v0.2.1** | `dump_to_writer` (P2 #9) | Low | Medium |
| **v0.2.1** | Dump metadata envelope (P2 #10) | Low | Medium |
| **v0.2.1** | `parking_lot::Mutex` + `Cow` for level (P0 #2) | Low | Medium |
| **v0.3.0** | `Arc<CapturedEvent>` in buffer (P1 #6 + P0 #2) | Medium | High — fixes both hot path and snapshot |
| **v0.3.0** | Edge case hardening (P2 #8) | Low | Low |
| **v0.3.0** | Unicode-safe redaction (P2 #7) | Low | Low-moderate |

The first three items in v0.2.0 would transform this crate from "a bounded
log buffer with nice ergonomics" into "a real flight recorder with nice
ergonomics." That is the gap worth closing.

---

<!-- This feedback was generated from a comparative review against the sibling
     go-flightrecorder project. All claims are verified against source code in
     both repositories as of 2026-08-11. Line references may drift as code changes. -->
