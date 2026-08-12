# Release runbook

How to cut a tracing-flight-recorder release. Follow this end-to-end; do not skip steps.

## Principles

1. **Draft release notes BEFORE tagging.** A tag-without-release window confuses
   downstream consumers and breaks link checkers. Draft → tag → push → publish.
2. **Never ship a breaking release without explicit user approval of the scope.**
   The CHANGELOG documents the breakage; the approval gates the release.
3. **One release at a time.** No two breaking releases in the same day. Let a
   release soak for at least a day before cutting the next.
4. **The verification gate is non-negotiable.** If it is not green, the release
   does not ship.

## Pre-release checklist

- [ ] All planned work for this release is merged to `master`.
- [ ] The latest CI run on `master` is green: `gh run list --limit 4`
      shows `success` on the commit you intend to tag.
      Local-only green is NOT sufficient.
- [ ] `CHANGELOG.md` has an entry for the new version under `## [Unreleased]`
      (or a specific `[x.y.z]` header if you prefer to stage it).
- [ ] `README.md` badges reflect the new version (crates.io badge auto-updates;
      verify the MSRV badge is still correct).
- [ ] `FEATURES.md` and `TODO_LIST.md` are updated for any feature that shipped
      or any TODO that completed in this release.
- [ ] `ROADMAP.md` is updated — no stale items in the "publication readiness"
      section.
- [ ] `Cargo.toml` version is bumped (see semver rules below).

## Verification gate (run all of these, capture exit codes)

```bash
cargo fmt --all -- --check
cargo clippy --all-features --all-targets -- -D warnings
cargo test --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --all-features --no-deps
cargo audit               # RustSec advisories
cargo deny check          # advisories + licenses + bans + sources
cargo publish --dry-run --all-features  # catch packaging issues before the real publish
```

If any of these fail, **stop**. Do not ship a release on a red gate.

## Semver rules

| Change                                         | Bump  | Example                               |
| ---------------------------------------------- | ----- | ------------------------------------- |
| New public API (additive)                      | minor | new dump format, new constructor      |
| Bug fix (no API change)                        | patch | collision counter fix                 |
| Breaking API change (field rename, removed fn) | major | `FlightRecorder` internal restructure |
| MSRV bump                                      | minor | 1.86 → 1.87 (document in CHANGELOG)   |

`#[non_exhaustive]` on public structs (if added) means adding a field is a minor
bump, not a major one.

## Cutting the release

### 1. Bump version

Edit `Cargo.toml`:

```toml
version = "<new>"  # was <old>, e.g. 0.1.0 → 0.1.1
```

Update `Cargo.lock` to match:

```bash
cargo update -p tracing-flight-recorder --precise <new>
```

### 2. Move CHANGELOG section

Rename `## [Unreleased]` to `## [<new>] - <date>` and add a fresh empty
`## [Unreleased]` section above it.

### 3. Commit the version bump

```bash
git add Cargo.toml Cargo.lock CHANGELOG.md README.md
git commit -m "release(v<new>): <one-line summary of what shipped>"
```

### 4. Tag

Before tagging, confirm CI is actually green on this commit (not just
locally):

```bash
gh run list --limit 4        # every run on this branch must show `success`
```

If any run is not green, **stop** — do not tag a release on a commit whose
CI is red or still running.

```bash
git tag -a v<new> -m "v<new>"
```

### 5. Draft the GitHub release notes (BEFORE pushing the tag)

Write the release notes now, while you can still edit freely. Source material:
the CHANGELOG section for this version, the diff since the last tag
(`git log v<old>..HEAD --oneline`).

### 6. Push

```bash
git push origin master
git push origin v<new>
```

### 7. Create the GitHub release

```bash
gh release create v<new> --title "v<new> — <summary>" --notes-file release-notes.md
```

For breaking releases, include a **Migration** section at the top of the notes
with before/after code snippets.

### 8. Publish to crates.io

The publish is automated via `.github/workflows/publish.yml`: pushing a tag
`v*.*.*` triggers `cargo publish --all-features` with
`CARGO_REGISTRY_TOKEN` injected from GitHub Actions secrets. The workflow
verifies the tag matches `Cargo.toml`'s version before publishing.

**One-time setup (repo admin):**

1. Create a crates.io API token at
   <https://crates.io/settings/api-tokens> with `publish-new` and
   `publish-update` scopes.
2. Add it as a repository secret named `CARGO_REGISTRY_TOKEN` at
   <https://github.com/LarsArtmann/tracing-flight-recorder/settings/secrets/actions>.
3. Verify the next tag push triggers the publish workflow in the Actions
   tab.

**If the secret is not configured,** the publish workflow fails at the
`cargo publish` step with a clear error; the tag still exists and the
GitHub release still lands, but the crate is not on crates.io. Manual
fallback:

```bash
cargo publish --all-features
```

Verify at https://crates.io/crates/tracing-flight-recorder and
https://docs.rs/tracing-flight-recorder (docs.rs takes ~5 minutes to build).

## Post-release verification

- [ ] `docs.rs/tracing-flight-recorder` shows the new version.
- [ ] `crates.io/crates/tracing-flight-recorder` shows the new version.
- [ ] The GitHub release URL resolves (no 404).
- [ ] `CHANGELOG.md` `[Unreleased]` section is empty or contains only
      post-release changes.

## Rollback

If the release has a critical bug:

1. **Yank** (does not remove, just hides from new resolves):
   ```bash
   cargo yank --version <new>
   ```
2. Cut a patch release with the fix.
3. Do NOT force-push or delete the tag — downstream consumers may have it cached.
