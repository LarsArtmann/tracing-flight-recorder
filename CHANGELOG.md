# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

_No changes yet._

## [0.1.0] - 2026-08-10

### Added

- Bounded in-memory ring buffer (`FlightRecorder`) with oldest-first eviction at capacity (`src/layer.rs`)
- `FlightRecorderLayer` implementing `tracing_subscriber::Layer` that feeds every event into the recorder (`src/layer.rs`)
- `CapturedEvent` serializable struct (timestamp, level, target, message, fields) and `FieldVisitor` capturing all `tracing` field value types (`src/capture.rs`)
- Automatic secret redaction: fields named `token`, `password`, `secret`, `api_key`/`apikey`, `credential`, `passphrase`, `private_key` are stored as `[REDACTED]` (`src/capture.rs:91`)
- JSON dump (`dump_to_json`), file dump with parent-dir creation (`dump_to_file`), and timestamped dump with file-count retention (`dump_with_retention`) (`src/layer.rs`)
- Shared-buffer `Clone` semantics: all `FlightRecorder` clones share one `Arc<Mutex<VecDeque>>`
- Poison-safe mutex locking: recovers via `PoisonError::into_inner` instead of poisoning the whole recorder
- `openapi` cargo feature deriving `utoipa::ToSchema` on `CapturedEvent` (`src/capture.rs:13`)
- Strict clippy gate in `Cargo.toml`: `pedantic` + `nursery` denied, plus `unwrap_used`, `indexing_slicing`, `as_conversions`, `arithmetic_side_effects`
- End-to-end test suite using real `tracing::subscriber::with_default` subscribers
- GitHub Actions CI pipeline: fmt check, clippy (all features), test (all features), MSRV 1.86 verification, doc build (`.github/workflows/ci.yml`)
- Three runnable examples: `minimal_dump`, `per_layer_filter`, `retention` (`examples/`)
- Domain language glossary documenting 20 terms across 5 categories (`docs/DOMAIN_LANGUAGE.md`)
- README doctests wired via `#[cfg(doctest)]` — Quick Start and Retention code blocks now compile-tested (`src/lib.rs`)
- OpenAPI schema integration test asserting all `CapturedEvent` fields appear in generated OpenAPI JSON (`src/capture.rs`)
- Property-based eviction invariant test (proptest, 256 cases): random capacity × random event count always satisfies `len == min(events, capacity)`
- Multi-thread stress test: 8 threads × 100 events against a shared `FlightRecorder` — no corruption, exact capacity bound
- Poison-recovery test: recorder remains usable after a thread panics while holding the mutex lock
- Unicode field name redaction test: documents that redaction is ASCII-substring-only (`café_token` → caught; `pässwörd` → not caught)
- Nested-directory dump test: `dump_to_file` creates deeply nested parent directories
- Non-JSON retention pruning test: `.txt` and `.yaml` files survive retention cleanup
- Memory footprint measurement test: 1000 realistic events ≈ 237 KB (asserts < 1 MB ceiling)
- Collision-limit guard tests for extracted `resolve_collision_path` function (error at limit, first-free-slot, primary-when-free)
- `CONTRIBUTING.md` with design philosophy, ASCII data-flow diagram, and PR checklist
- `cargo publish --dry-run` CI job to catch packaging regressions on every push/PR
- Security audit CI job (`rustsec/audit-check@v2.0.0`)
- Dependabot configuration: weekly updates for cargo and github-actions ecosystems

### Changed

- Bumped `utoipa` dependency to v5 with `chrono` feature support (`8c8902b`)
- Replaced leaked `monitor365` project name with neutral `my_app` in doc comments and README
- Purged 680 committed `target/` build artifacts from git history (71MB → 188KB `.git`)
- Softened timing claim in `DEFAULT_CAPACITY` docs: "30-60 seconds" → honest "20-100 seconds at 10-50 events/sec" range
- Extracted collision-resolution logic into testable `resolve_collision_path` function with injectable `COLLISION_LIMIT` (9999) constant
- Examples now write to `std::env::temp_dir()` instead of repository root
- Tightened `exclude` list — internal docs (`/docs/status`, `/docs/planning`, `/AGENTS.md`) excluded from published crate (150.9KiB → 90.2KiB)

### Fixed

- Same-second filename collision in `dump_with_retention`: two dumps within one second no longer silently overwrite; counter suffix (`-1`, `-2`, ...) is appended (`src/layer.rs:134`)

## Notes

- MSRV: 1.86, edition 2021.

[Unreleased]: https://github.com/LarsArtmann/tracing-flight-recorder/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/LarsArtmann/tracing-flight-recorder/releases/tag/v0.1.0
