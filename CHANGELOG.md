# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

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

### Changed

- Bumped `utoipa` dependency to v5 with `chrono` feature support (`8c8902b`)
- Replaced leaked `monitor365` project name with neutral `my_app` in doc comments and README
- Purged 680 committed `target/` build artifacts from git history (71MB → 188KB `.git`)
- Softened timing claim in `DEFAULT_CAPACITY` docs: "30-60 seconds" → honest "20-100 seconds at 10-50 events/sec" range

### Fixed

- Same-second filename collision in `dump_with_retention`: two dumps within one second no longer silently overwrite; counter suffix (`-1`, `-2`, ...) is appended (`src/layer.rs:134`)

## Notes

- No releases tagged yet; crate version is `0.1.0` in `Cargo.toml` (unreleased). The first git tag will populate a versioned section above.
- MSRV: 1.86, edition 2021.
