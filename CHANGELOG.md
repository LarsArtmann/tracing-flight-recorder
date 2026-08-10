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
- End-to-end test suite (15 tests) using real `tracing::subscriber::with_default` subscribers

### Changed

- Bumped `utoipa` dependency to v5 with `chrono` feature support (`8c8902b`)

### Fixed

- _(none)_

## Notes

- No releases tagged yet; crate version is `0.1.0` in `Cargo.toml` (unreleased). The first git tag will populate a versioned section above.
- MSRV: 1.86, edition 2021.
