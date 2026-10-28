# Docs v2: VitePress + full rewrite of the guide

Status: r3; Phase 0 done 2026-10-28 (b5cc436, d43ebae), Codex verify fixed (1 P1, 2 P2, 1 P3) (Codex plan review: round 1 = 4 P1/11 P2/3 P3, round 2 = 3 P1/8 P2/1 P3, all applied).
Owner chose **VitePress** (over Starlight, MkDocs/Zensical, re-themed mdBook).

Goals:
1. Replace mdBook with VitePress for `docs/guide` (better look, search, dark mode, mobile).
2. Rewrite every guide page: re-read the current page **and** the source, add screenshots from the live demos
   (RDockPanel, RBlog) and the slide decks, add runnable code samples; Codex reviews each batch before the next.

Demo logins are **not** in this file (the repo is public). They live in
`~/.claude/plans/docs-v2-credentials.md` and are passed to scripts through env vars only. The owner disables the
accounts when the work is done.

## Decisions

- **The guide is no longer embedded in the binary** (owner, 2026-10-28). Reasons: smaller binary, no
  `file://`/localhost-server problem (VitePress output needs HTTP), docs fixes ship without a CLI release, images
  cost nothing in the binary. Offline readers clone the source or read the Markdown on GitHub.
  Supersedes the "embedded guide" part of ADR 0025 (add a supersession note).
- **Source stays in `RustCMS-Core`** (`docs/guide/src`), next to the code: one commit changes code + guide (the
  sync rule), samples import from `examples/*`, the review checklist sees both. **The built site is published to
  `irisvn/RustCMS` GitHub Pages** (`https://irisvn.github.io/RustCMS/`, same place as `install.sh`), as today.
- **Stable docs describe a release, never `main`** (Codex P1). Layout on gh-pages:
  - `/` = latest stable release, `/v/X.Y.Z/` = every release (built from the tag, as today);
  - `/dev/` = `main`, with a banner "unreleased, for RustCMS-Core main" and `noindex`.
  - Fixing the stable guide without a CLI release: branch `docs/vX.Y.Z` from the tag, cherry-pick doc-only
    commits, publish it with `docs.yml` (the workflow checks that the latest stable tag is an ancestor and that
    the diff from the tag touches only `docs/guide/` and `examples/demo/tests/docs_snippets.rs`; no production
    code).
  - The rewrite is published to `/dev/` while it is in progress and becomes stable with the next release.
- Every build records the source SHA and the documented Core version in the footer.
- **URL compatibility contract**: `cleanUrls: false`; existing page paths stay (`introduction.html`,
  `upgrade.html` — the CLI links to `/upgrade.html`, `crates/rustcms-cli/src/upgrade.rs:62`); heading anchors
  linked from code or other pages keep their IDs (explicit `{#id}` when a heading changes). `print.html` is
  retired: a stub page says the guide has no print view and links to the home page.

## Constraints found in the current setup

- `rustcms docs` (`crates/rustcms-cli/src/guide.rs`) embeds the book via `RUSTCMS_DOCS_DIR` + `include_dir` in
  release builds and extracts it to a cache; `build.rs:20` refuses a release build without it
  (`RUSTCMS_RELEASE`); `release.yml` builds the book before the targets and its smoke test runs
  `test -f "$(rustcms docs --print-path)"` (`release.yml:139`).
- `build.rs` also injects `RUSTCMS_TARGET` (`release.rs:14`): keep it. `include_dir` is still used by scaffold and
  the SDK copy, `tempfile` by self-update: keep both.
- Pages: one clone-modify-push of gh-pages in `release.yml:239` under concurrency group `release`; the stable
  path deletes every root entry except `.git` and `v` (`release.yml:245`); release asset checks expect 3 archives
  and 5 assets in total (`release.yml:165`, `:225`) — docs never become release assets.
- `just doc` (`justfile:83`) and CI job `doc` (`ci.yml:142`) run rustdoc, doctests and `mdbook build`.
  `README.md:82` links `SUMMARY.md`; `docs/README.md:5`, `docs/core/guides/releasing.md`, `OVERVIEW.vi.md`
  mention mdBook.
- Vue interpolation: inline `{{ … }}` in `admin/customizing.md:314`, `admin/i18n.md:56`; `reference/cli.md` is
  generated verbatim from help text (`crates/rustcms-cli/src/docs.rs:29,62`).
- 58 pages, ~8.2k lines, 37 with Rust code; no mdBook-only syntax; fragment links exist
  (e.g. `reference/admin-api.md:9`).
- Dependencies at least two weeks old, **including every transitive npm package** in the lockfile;
  vitepress **1.6.4** (2025-08-05), not 2.0 alpha. CI uses Node 22.

## Phase 0 — VitePress skeleton, no embedding (no content change yet)

1. `docs/guide/package.json` (`"engines"` Node 22 + npm version) + lockfile, vitepress 1.6.4 exact. Every builder
   (just, CI, release, docs.yml) asserts `npm --version` matches before `npm ci`. A script
   `docs/guide/scripts/check-deps-age.mjs` (plain Node, no dependencies) fails if any resolved version in the
   lockfile is younger than 14 days; it runs **before** `npm ci` (rejected packages never run install scripts),
   in CI only when the lockfile changed. Exceptions need the owner's OK.
2. `.vitepress/config.mts`:
   - `srcDir: 'src'`, `base` from `DOCS_BASE` (default `/`), `outDir` from `DOCS_OUT` (one directory per base),
     `cleanUrls: false`, `lastUpdated: true` (every checkout that builds docs uses `fetch-depth: 0`).
   - Sidebar in config, same section order as `SUMMARY.md` (then delete `SUMMARY.md` and `book.toml`).
   - `search: { provider: 'local' }`; `editLink` → `RustCMS-Core/edit/main/docs/guide/src/:path`.
   - `ignoreDeadLinks: false`; Shiki: rust, toml, sql, ts/tsx, bash, json, http, dotenv.
   - Footer: version + short SHA from env (`DOCS_VERSION`, `DOCS_SHA`); `/dev/` build adds the banner and
     `<meta name="robots" content="noindex">`.
3. Theme: extend the default theme; brand colors and logo from the admin panel; home page (`index.md`, layout
   `home`) with hero + feature cards + one screenshot. No custom components in Phase 0 (deferred, see Phase 5);
   any asset URL in theme code uses `withBase`.
4. Vue safety: hand-written pages put literal `{{ … }}` in fenced blocks or a `<span v-pre>` (never `v-pre`
   text inside backticks). `docs:cli` wraps the whole generated body in `<div v-pre>` (blank lines around it so
   Markdown still renders) instead of a Markdown-aware escaper. A generator fixture with `{{ name }}`, raw
   `<name>`, inline code and defaults; the build of the fresh `cli.md` checks rendered headings and tables.
5. Snippet imports: `<<< @/../../../examples/demo/src/...#region` (from `docs/guide/src`, three levels up).
   VitePress may fall back to the whole file when a region is missing, so a pre-build script
   `scripts/check-snippets.mjs` checks each import: file exists, region name present, start/end markers paired,
   extract not empty. A negative fixture proves a missing region fails. Phase 0 adds one real import.
6. Post-build check `docs/guide/scripts/check-site.mjs`: every local `<img src>` exists, every `#fragment` link
   resolves to an `id` in the target page, the stub `print.html` exists. 404: a small static server that mimics
   Pages (root `404.html` fallback) serves an unknown root URL and an unknown `/v/X.Y.Z/` URL; the page renders
   with its assets and the home link works. Documented: a missing URL under any version shows the root guide's
   404. External URLs: a separate, non-blocking `just docs-links` (manual).
7. `rustcms docs`: prints/opens `https://irisvn.github.io/RustCMS/v/X.Y.Z/`; `--print-path` prints that URL.
   Remove `BOOK`, `embedded()`, `extract()`, cache code and their tests (`guide.rs:113…`), the `rustcms_docs` cfg,
   `RUSTCMS_DOCS_DIR` and `RUSTCMS_RELEASE` in `build.rs` and in workflow env, `MDBOOK_VERSION`. Add a CLI test
   for the printed URL. Regenerate `reference/cli.md`; upgrade.md: "offline guide removed, `rustcms docs` opens
   the online guide of your version".
8. Build & publish:
   - `justfile`: `doc` keeps rustdoc + doctests and calls `docs` (= `npm ci && vitepress build` + `check-site`);
     `docs-dev` = `vitepress dev`. CI job `doc` same.
   - `release.yml`: targets stop depending on the docs job. Docs job (no token, `persist-credentials: false`)
     builds `/RustCMS/v/X.Y.Z/` and, for stable, `/RustCMS/` into separate out dirs, uploads them as a `site`
     artifact (not an `asset-*` artifact). Smoke test becomes
     `test "$(rustcms docs --print-path)" = "https://irisvn.github.io/RustCMS/v/${GITHUB_REF_NAME#v}/"`;
     "Check what developers download" also fetches the root, a deep page, its JS/CSS and an image under both bases.
   - New `docs.yml` (`workflow_dispatch`, input `target` = `dev` | `stable`, no `ref` input; jobs guarded by
     `github.ref == 'refs/heads/main'`):
     - **build job** (no secrets, `persist-credentials: false`, `contents: read`): `dev` builds `main`; `stable`
       reads `latest.txt` from gh-pages, builds `docs/vX.Y.Z` after checking the tag is an ancestor and the diff
       only touches the allowed paths. Resolves the ref to a SHA once; uploads `site` (+ the SHA).
     - **publish job**: `environment: pages` (the write token lives in this GitHub Environment, deployment
       branches restricted to `main`; Contents-write on `irisvn/RustCMS` only). It checks out the trusted
       workflow commit (`github.sha`, `persist-credentials: false`) for `scripts/pages-publish.sh`, downloads
       `site` into a staging dir and rejects symlinks and reserved root names (`.git`, `.github`, `v`, `dev`,
       `install.sh`, `latest.txt`, `CNAME`, `.nojekyll`); only the publisher writes those paths. Nothing from the
       artifact is executed.
   - `release.yml` publish job moves its token to the same `pages` environment and uses the same script and
     staging checks.
   - Locking: **job-level** `concurrency: { group: gh-pages, cancel-in-progress: false }` on both publishing jobs
     (release keeps its workflow-level `release` group). GitHub keeps one pending run per group: a displaced
     publication must be re-run (documented in `releasing.md`). On push conflict: fetch, reapply, recheck
     staleness, retry; never force-push.
   - Per-mode contracts (enforced by `pages-publish.sh`):
     - release-stable: writes `/v/X.Y.Z/`, replaces the root, advances `install.sh` and `latest.txt`;
     - release-RC: writes only `/v/X.Y.Z-rc.N/`;
     - docs-dev: writes only `dev/`;
     - docs-stable: refuses unless its version equals `latest.txt`; replaces root docs and `/v/<latest>/`;
       `install.sh` and `latest.txt` stay byte-identical.
     The root cleanup keeps `.git`, `v`, `dev`, `CNAME`, and (for docs-stable) `install.sh`, `latest.txt`.
   - `.gitignore`: `docs/guide/.vitepress/{dist,cache}`, `docs/guide/node_modules`, `docs/guide/out*`.
9. Docs updates: CLAUDE.md, both READMEs, `releasing.md`, `OVERVIEW.vi.md`, ADR 0025 note, CHANGELOG.
10. Migrate the 58 pages as-is (only syntax fixes) so the switch is a separate, reviewable commit.

Tests:
- Rust: `rustcms docs --print-path` URL test; release-profile build without `RUSTCMS_DOCS_DIR`; `just test-fast`
  for rustcms-cli.
- Docs: `vitepress build` + `check-site` + deps-age in CI.
- Pages logic: `scripts/pages-publish.sh` tested locally against a fixture gh-pages checkout for the four
  per-mode contracts, plus: a stale docs-stable is refused, an artifact with a reserved name or a symlink is
  refused, old `/v/*`, `dev/`, `CNAME` survive every mode.
- Docs snippets/generator fixtures: missing region fails; `cli.md` with braces renders.
- Prefixes: build and serve with the same absolute `DOCS_OUT` and `DOCS_BASE` (a static server mounted at
  `/RustCMS/`, `/RustCMS/v/X.Y.Z/`, `/RustCMS/dev/`); reload a deep page, assets, search hit, image; on `/dev/`
  assert the banner and the robots meta on a deep page. Manual: dark mode, 390px.

Codex: one verify on Phase 0 (workflows push to a public repo; token scope; publish script).

## Phase 1 — Inventory (before writing)

For each feature, one agent reads the current page, the code (crate modules from `docs/core/architecture.md`),
tests, `examples/demo`, `examples/lms`, the CHANGELOG and ADRs, and writes a row in
`docs/core/reports/docs-v2-inventory.md`, recording the source SHA it read:
feature · public API (types, builder methods, config keys, CLI commands) · what the page says that is wrong or
missing · examples available in `examples/*` · screenshots needed (site + screen) · anchors linked from elsewhere ·
owner page. Also list features with no page at all (e.g. schedules UI, queue page, branding, record actions on
phones…).

## Phase 2 — Screenshots (per batch, after the batch's text settles)

- Sources, in order of preference:
  1. Existing images: `../rblog-slides/img/*` (20, RBlog) and `../rustcms-slides/img/*` (18, incl. `rdp-ui.png`)
     — reuse when the UI still matches the current version, else recapture.
  2. Live demos, captured with Playwright (Chrome channel):
     - RDockPanel: `https://rdockpanel.ducdoblog.com/admin` (panel pages, realtime, WebSocket, settings, roles).
     - RBlog: `https://cms.ducdoblog.com/admin/` (resources, forms, tables, rich text, media, actions, schedules,
       revisions, import/export, settings, i18n, mobile).
  3. `examples/demo` run locally, for core features neither demo shows and for anything that runs an action or
     writes.
- Script `docs/guide/scripts/shots/` with its **own Playwright config**: no trace, HAR, video or failure
  screenshots, no credential logging; never run in CI, raw captures never uploaded. Credentials only from env
  (`RDP_EMAIL`, `RDP_PASSWORD`, `RBLOG_EMAIL`, `RBLOG_PASSWORD`); auth state kept in memory (one browser context
  per site, no `storageState` file).
- **Read-only enforced on the live demos** (Playwright 1.56.1, as admin-ui): contexts are created with
  `serviceWorkers: 'block'` and `acceptDownloads: false`; **before any page exists**, `context.route('**')`
  aborts every non-GET/HEAD request except the exact login POST (the exception is removed right after login;
  login's session + audit entry is the only write), and `context.routeWebSocket('**')` closes every socket
  (live screens that need a socket are captured on the local demo). Steps may open menus and dialogs, never
  submit.
- Variants: desktop 1440×900 light; phone 390×844 where the page has phone behaviour; dark only for the home page
  and the theming page. deviceScaleFactor 2, cropped to the element, WebP, `docs/guide/src/public/img/<section>/<id>.webp`.
- Privacy: mask emails, IPs, domains other than the demo's, tokens, API keys, user names and free-text content
  that is not demo copy; review every image before commit.
- Captions say which app a screen comes from when it shows app-specific pages (RDockPanel/RBlog are apps built
  on RustCMS); the text makes clear what core gives and what the app added. Note the app's Core version.

## Phase 3 — Rewrite, one batch at a time

Order (each line = one batch = one Codex review = one commit):
1. Introduction + home page + Installation/Configuration/Directory structure/Deployment
2. Architecture (lifecycle, providers & container)
3. Basics: routing, validation & pagination, errors
4. Database & migrations, console, testing
5. Security: authentication, roles & permissions, revocation, production checklist, API authentication
6. Admin overview + dashboard + admin authorization (the shared invariants: deny by default, `scope()`, field
   rules — later pages link here)
7. Resources, forms, tables
8. Actions, relationships
9. Import/export, media/uploads/rich text, custom fields
10. Customizing, branding, React pages (v2 extensions), admin routes, realtime
11. Users/roles/account, i18n, settings
12. Digging deeper: encryption, storage, instances, audit log, queues, scheduling, mail
13. Operations: queue, retention, backup/restore, APP_KEY, mail
14. Examples: LMS walkthrough
15. Reference: CLI (generated), DB schema, admin API, error codes, wire format, limitations; release notes, upgrade

Page template: what it is (2–3 lines) → screenshot → quick start (smallest working code) → full example
(snippet imported from `examples/demo` or `examples/lms`) → options/reference table → security notes (link to
the shared invariants) → pitfalls → related pages.

Code samples: substantial Rust samples are imported from checked files (`examples/*` regions, or
`examples/demo/tests/docs_snippets.rs` — a top-level, auto-discovered test target — with regions); a sample that
claims runtime behaviour gets a focused test there, not only `cargo check`. Short shell/TOML/env snippets may stay
inline (the install and scaffold checks cover them).

Per batch loop:
1. Writer reads inventory rows + source at a recorded SHA, writes pages, then screenshots (Phase 2).
2. `just docs` (build + check-site), `cargo check -p demo -p lms --all-targets` (+ feature sets the samples use).
3. **Codex review** (codex:codex-rescue) of the batch diff, prompt: "start immediately; check every claim against
   the source at SHA <x> (file:line), samples are imported from checked code and use current APIs, security
   invariants stated correctly, no undeclared behaviour, anchors kept, screenshots match the text and show no
   personal data, English clarity; findings ranked, no rewrites".
4. Fix all findings in one pass; one Codex verify of the fixes (review-checklist rule), then commit
   `docs v2: <batch>` and republish `/dev/`.

## Phase 4 — Finish

- Whole-site pass: navigation, search terms, images, dark mode, 390px, under each base.
- `review-checklist.md`: a UI change updates its guide page and screenshot.
- Ship the `rustcms docs` change and the rewritten guide with the next release (stable root + `/v/X.Y.Z/`).
- Tell the owner to disable both demo accounts; delete `~/.claude/plans/docs-v2-credentials.md`.

## Phase 5 — Optional, only after Phase 4 passes

- Components: `<Screenshot>` (light/dark pair, zoom), `<Since version>` badge.
- Dark captures for all screens.
- Case studies: "Building a blog (RBlog)", "A hosting panel (RDockPanel)".

## Definition of done

- mdBook and the embedded guide removed (CI, release, build.rs, guide.rs, justfile, docs mentions); release
  asset counts unchanged.
- Builds for `/`, `/v/X.Y.Z/`, `/dev/` work at their real prefixes (deep reload, assets, search, 404);
  `check-site`, `check-snippets` and deps-age pass in CI.
- `pages-publish.sh` passes the four per-mode contracts on a fixture checkout; both publishing jobs hold the
  job-level `gh-pages` lock; the write token lives only in the `pages` environment (main only); publishers run
  no code from the artifact and reject reserved names and symlinks.
- Every page rewritten, Codex-reviewed against a recorded SHA, screenshots where there is UI, samples imported
  from checked code; existing page paths and linked anchors kept.
- No credentials or personal data in the repo, images, logs or CI artifacts.
- `just generated` clean, CI green, Codex verify of Phase 0 done.

## Risks

- Demo sites run a different Core version than HEAD → caption the version, prefer the local demo for changed
  screens, recapture after upgrades.
- Readers of an old version see its `/v/X.Y.Z/` guide; `docs.yml` only refreshes the latest stable one.
- VitePress 2.0 is alpha → stay on 1.6.x; transitive npm deps must pass the 14-day check.
- Blocking WebSockets leaves live RDockPanel screens empty → capture those on a local demo.
- A dispatch displaced in the `gh-pages` queue is dropped by GitHub → re-run it (documented).
