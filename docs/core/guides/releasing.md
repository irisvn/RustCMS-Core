# Releasing

What a release publishes and why: [ADR 0025](../adr/0025-distribution.md).

## Once

- Environment `pages` in RustCMS-Core (Settings → Environments), deployment
  branches and tags: *Selected*, branch `main` and tag `v*`. Its secret
  `DIST_TOKEN`: a fine-grained personal access token, repository access *only*
  `irisvn/RustCMS`, permission *Contents: read and write*. Only the jobs that
  declare the environment (`token`, `publish` of Release; `publish` of Docs)
  see it; the jobs that run npm packages never do. It expires: set a reminder;
  a release with an expired token fails in `publish` and can be re-run after
  `gh secret set DIST_TOKEN --env pages -R irisvn/RustCMS-Core`.
- Pages of irisvn/RustCMS: source "Deploy from a branch", `gh-pages`, `/`
  (`gh api repos/irisvn/RustCMS/pages -X POST -f 'source[branch]=gh-pages' -f 'source[path]=/'`).

## A release

1. `just release-bump X.Y.Z` (or `X.Y.Z-rc.N`): every version of the
   workspace, the examples' metadata and `Cargo.lock`.
2. `docs/core/CHANGELOG.md`: a `## [X.Y.Z] — date` section (a release
   candidate may stay under `[Unreleased]`; its notes are taken from there).
   `docs/guide/src/upgrade.md`: the steps from the previous version;
   `docs/guide/src/release-notes.md`: what the release brings. Not bumped by
   the script: the SDK's `VERSION` (`admin-ui/sdk/VERSION`, copied to
   `crates/rustcms-cli/templates/sdk` and `examples/demo/resources/admin/sdk`;
   `just generated` checks the copies) and the `tag = "vX.Y.Z"` pins in the
   guide's installation and deployment pages and in
   `docs/core/guides/deployment.vi.md`.
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

## The guide without a release

The `Docs` workflow (Actions → Docs → Run workflow, from `main`):

- `dev`: the guide of `main` at `/dev/` (a banner says it is unreleased,
  search engines skip it).
- `stable`: a fix to the guide of the latest release. Branch
  `docs/vX.Y.Z` from the tag `vX.Y.Z` (the version in `latest.txt`),
  cherry-pick the doc-only commits, push the branch, run the workflow. It
  refuses a branch that does not contain the tag or changes anything but
  `docs/guide/` (and `examples/demo/tests/docs_snippets.rs`). It rewrites the
  root guide and `/v/X.Y.Z/`; `install.sh` and `latest.txt` stay. Releases
  before the move to VitePress cannot be republished this way.

Release and Docs write `gh-pages` one at a time (job concurrency group
`gh-pages`). GitHub keeps one waiting run per group: a run that was
replaced while waiting shows as cancelled; run it again.
`scripts/pages-publish.sh` places the guides (tested by
`scripts/test-pages-publish.sh`); a built guide never contains `install.sh`,
`latest.txt`, `v/`, `dev/`, `CNAME`, `.nojekyll` or a symlink.

## Release candidates

`X.Y.Z-rc.N` is published as a pre-release: the guide under `/v/X.Y.Z-rc.N/`
only, `latest.txt`, the root guide and the install scripts are untouched.
Install one with `RUSTCMS_VERSION=vX.Y.Z-rc.N`.
