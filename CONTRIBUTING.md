# Contributing

Thanks for your interest in improving `tracing-flight-recorder`! This is a small,
focused crate — contributions are welcome but should match the crate's design
philosophy.

## Design Philosophy

1. **Zero non-tracing dependencies in the default feature set** — the crate
   stays lean. Optional features behind feature flags are fine.
2. **Poison-safe by design** — a panicked thread must never make the recorder
   unusable. Recovery via `PoisonError::into_inner` is intentional.
3. **Per-layer filtering is the user's responsibility** — the crate documents
   this prominently. Don't add global-filter "magic" that would hide the
   requirement.
4. **Strict clippy gate** — `pedantic` + `nursery` denied. All new code must
   pass `cargo clippy --all-features --all-targets -- -D warnings`.

## Development Setup

```sh
cargo test --all-features          # canonical test gate (24 unit + 3 doctests)
cargo clippy --all-features --all-targets -- -D warnings
cargo fmt --check
cargo doc --all-features --no-deps
```

## Data Flow

```
tracing::event!
    │
    ▼
FlightRecorderLayer::on_event()
    │
    ▼
CapturedEvent::from_event()
    ├── FieldVisitor::record_*()  ← collects key-value fields
    ├── is_sensitive_field()      ← redacts secrets → [REDACTED]
    └── level_to_string()         ← maps Level → string
    │
    ▼
FlightRecorder::push()
    ├── lock buffer (poison-safe)
    ├── evict oldest if at capacity
    └── push_back
    │
    ▼
FlightRecorder::snapshot() / dump_to_json() / dump_to_file() / dump_with_retention()
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
