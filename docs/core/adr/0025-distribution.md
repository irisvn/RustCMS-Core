# ADR 0025: Distribution: binaries and guide in irisvn/RustCMS, Git dependencies on Core

- Status: accepted (M24a)
- Date: 2026-10-16

## Context

Team developers should only need the `rustcms` command: create an application,
let Cargo fetch everything, build. The maintainer works on the framework
(`irisvn/RustCMS-Core`, public, MIT OR Apache-2.0). Applications compile
together with the framework (Rust has no stable binary library format), so
Cargo must reach Core's source. crates.io is not used yet.

## Decision

- Two public repositories: **RustCMS-Core** (source, the only place anything
  is edited) and **RustCMS** (what developers use): GitHub Releases with the
  CLI per target, `SHA256SUMS`, `manifest.json` (Core tag and commit); `main`
  with README and install scripts; Pages (`gh-pages`) with the guide (root =
  latest stable, `/v/X.Y.Z/` per release), `install.sh` and
  `latest.txt`. Everything there comes from Core's release workflow (`dist/`,
  `docs/guide`).
- `rustcms new` always depends on `rustcms` and `rustcms-testing` through the
  same Git URL and the tag `v{CLI version}`; a local path only with
  `--rustcms-path`. The old "built from a checkout" detection was dropped: a
  CLI installed with `cargo install --git` found its Cargo cache checkout and
  wrote that path into applications. It then runs `cargo generate-lockfile`:
  a network failure is a warning, a missing tag or a resolution failure fails.
  Applications commit `Cargo.lock` and their generated CI uses `--locked`.
- A tag `vX.Y.Z` (or `-rc.N`) runs the release workflow: the tag matches every
  version (`scripts/release_version.py check`), the CI run of the tagged commit
  on `main` passed (waited for, not repeated), then per target a
  native runner builds, packages, installs with the install script from the
  packaged asset, creates an application from the tag and builds and tests it.
  Only then: a draft release, every asset uploaded and checked against
  `SHA256SUMS`, published; Pages updated and the served files checked. A
  release candidate never changes `latest`, the root guide or the scripts.
- Targets: `aarch64-apple-darwin`, `x86_64-apple-darwin` (macOS 11+),
  `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl` (static). A
  target that fails its checks is not published. Windows is not supported
  (the maintainer's choice; WSL runs the Linux build): it built and passed in
  the M24a spike, so adding it back is a matrix entry and an install script.
- The install script accepts HTTPS only, check the checksum before opening the
  archive, take only the executable, install next to the destination then
  rename (an existing install survives a failure), and record `install.json`
  (used by `self-update`, M24b).

## Consequences

- No credentials anywhere for developers or application CI; the first build
  clones Core (~25 MB) once per machine.
- A checksum published beside the archive detects corruption, not a
  compromised release; signing (and macOS notarization) is future work.
- Release tags are never moved: applications pin them, and their lockfiles pin
  the commit.
- Publishing to crates.io later only changes the dependency `rustcms new`
  writes.
- The release workflow needs `DIST_TOKEN` (fine-grained, Contents write on
  irisvn/RustCMS only), given to the publish job alone.
