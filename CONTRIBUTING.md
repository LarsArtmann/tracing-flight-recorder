# Contributing

Thanks for your interest in improving `tracing-flight-recorder`! This is a small,
focused crate — contributions are welcome but should match the crate's design
philosophy.

## Design Philosophy

1. **Minimal dependencies** — `tracing` ecosystem + `serde`/`chrono` for
   serialization. Optional features behind feature flags are fine.
2. **Poison-safe by design** — a panicked thread must never make the recorder
   unusable. Recovery via `PoisonError::into_inner` is intentional.
3. **Per-layer filtering is the user's responsibility** — the crate documents
   this prominently. Don't add global-filter "magic" that would hide the
   requirement.
4. **Strict clippy gate** — `pedantic` + `nursery` denied. All new code must
   pass `cargo clippy --all-features --all-targets -- -D warnings`.

## Development Setup

```sh
cargo test --all-features          # canonical test gate (includes openapi + gzip + proptest)
cargo clippy --all-features --all-targets -- -D warnings
cargo fmt --check
cargo doc --all-features --no-deps
cargo bench                        # optional: hot-path benchmarks
```

## Data Flow

```
tracing::event!
    │
    ▼
FlightRecorderLayer::on_new_span() / on_record()
    │   └── FieldVisitor captures span fields → stored as CapturedSpanFields extension
    │
    ▼
FlightRecorderLayer::on_event()
    │
    ├── capture_span_context()    ← walks scope.from_root(), builds Vec<SpanContext>
    │                               (fields shared via Arc<Vec>, O(1) clone)
    ├── CapturedEvent::from_event()
    │   ├── FieldVisitor::record_*()  ← collects key-value fields
    │   ├── is_sensitive_field()      ← redacts secrets → [REDACTED]
    │   └── level_to_string()         ← maps Level → Cow<'static, str> (zero-alloc)
    │
    ▼
FlightRecorder::push()
    ├── lock buffer (poison-safe)
    ├── evict oldest if at capacity
    └── push_back
    │
    ▼
Trigger check → fire_dump() (if attached)   ← automatic snapshot on failure
    │
    ▼
FlightRecorder::snapshot() / dump_to_json() / dump_to_json_lines() /
dump_to_writer() / dump_to_writer_lines() / dump_to_file() / dump_with_retention() /
dump_envelope_to_file() / dump_with_retention_envelope()
```

## Pull Request Checklist

- [ ] `cargo test --all-features` passes
- [ ] `cargo clippy --all-features --all-targets -- -D warnings` passes
- [ ] `cargo fmt --check` passes
- [ ] New features have tests
- [ ] FEATURES.md updated if a feature ships
- [ ] CHANGELOG.md `[Unreleased]` section updated
- [ ] No secrets in code, tests, or example output

## Adding a New Dump Format

The crate currently outputs JSON. To add a new format:

1. Add a method on `FlightRecorder` (e.g., `dump_to_chrome_trace()`)
2. Test it with realistic data
3. Update FEATURES.md
4. Add CHANGELOG entry

Avoid introducing a `DumpFormat` trait abstraction until there are 3+ formats
(YAGNI — the current design is simple and direct).

## Reporting Issues

Include:
- Rust version (`rustc --version`)
- Crate version
- Minimal reproduction (ideally as a test)
- Expected vs actual behavior
