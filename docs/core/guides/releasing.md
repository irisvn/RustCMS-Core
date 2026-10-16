# Releasing

What a release publishes and why: [ADR 0025](../adr/0025-distribution.md).

## Once

- `DIST_TOKEN` secret in RustCMS-Core: a fine-grained personal access token,
  repository access *only* `irisvn/RustCMS`, permission *Contents: read and
  write*. It expires: set a reminder; a release with an expired token fails in
  `publish` and can be re-run after `gh secret set DIST_TOKEN -R irisvn/RustCMS-Core`.
- Pages of irisvn/RustCMS: source "Deploy from a branch", `gh-pages`, `/`
  (`gh api repos/irisvn/RustCMS/pages -X POST -f 'source[branch]=gh-pages' -f 'source[path]=/'`).

## A release

1. `just release-bump X.Y.Z` (or `X.Y.Z-rc.N`): every version of the
   workspace, the examples' metadata and `Cargo.lock`.
2. `docs/core/CHANGELOG.md`: a `## [X.Y.Z] — date` section (a release
   candidate may stay under `[Unreleased]`; its notes are taken from there).
   `docs/guide/src/upgrade.md`: the steps from the previous version.
3. Gate, commit, push `main`, then `git tag vX.Y.Z && git push origin vX.Y.Z`.
4. Watch the Release workflow: `verify`, `token`, `ci` (waits for the CI run
   of the tagged commit on `main`; nothing is tested twice), `docs`, one
   `target` job per platform (build, package, install with the script, new
   application from the tag, build, test), then `publish`. Push the commit to
   `main` before the tag; a newer push to `main` cancels the older CI run, so
   tag the last commit or re-run its CI.

Never move or delete a published tag: applications depend on it. A broken
release is fixed by the next one.

## When something fails

- `verify`: a version does not match the tag. Delete the tag (not yet
  published), fix with `release-bump`, tag again.
- `ci` (the CI run of the commit failed or was cancelled) or a `target`:
  nothing was published. A cancelled CI run: re-run it, then re-run the
  Release workflow. Fix on `main`, delete and
  re-create the tag only if no release exists for it on irisvn/RustCMS;
  otherwise release the next patch version.
- `publish`: re-run the job. A draft release gets its missing assets; a
  published one is left alone; Pages and `main` are only committed when they
  change.

## Release candidates

`X.Y.Z-rc.N` is published as a pre-release: the guide under `/v/X.Y.Z-rc.N/`
only, `latest.txt`, the root guide and the install scripts are untouched.
Install one with `RUSTCMS_VERSION=vX.Y.Z-rc.N`.
