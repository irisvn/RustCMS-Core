# Documentation

| Folder | Audience | Published |
|---|---|---|
| [`guide/`](guide/) | developers building applications on RustCMS (English, VitePress) | **yes**: https://irisvn.github.io/RustCMS/ |
| [`core/`](core/) | the RustCMS team: architecture, protocol, decisions, reports, changelog | **no** (private) |

Rules:

- Every change updates the guide page of the behaviour it changes, in the same
  change. Internals (milestones, reviews, design trade-offs) go to `core/`,
  never to `guide/`.
- `guide/src/reference/cli.md` is generated:
  `cargo run -q -p rustcms-cli -- docs:cli > docs/guide/src/reference/cli.md`.
- `guide/src/reference/error-codes.md` must list every code of
  `rustcms_core::i18n::TEMPLATES` (checked by `crates/rustcms-admin/tests/issues.rs`).

## The guide (VitePress)

- `just docs-dev` serves it with live reload; `just docs` checks npm package ages, installs, builds with every
  check; `just docs-prefixes` builds it for the three published prefixes and serves them like GitHub Pages.
- Pages live in `guide/src`; the navigation is `guide/.vitepress/sidebar.ts` (a new page is added there).
- Heading ids are mdBook's (`page.html#from-0100-to-0101`); page paths keep `.html`. Do not rename a page or a
  linked heading; the build fails on a dead link or anchor (`scripts/check-site.mjs`).
- Code from a checked file: mark a region in it (`// #region name` … `// #endregion name`) and import it with
  `<<< @/../../../examples/demo/src/....rs#name [title]`. A region is required (`scripts/check-snippets.mjs`).
- Callouts: `::: tip`, `::: warning`, `::: danger`, `::: details`; code groups: `::: code-group`.
- Literal `{{ … }}` outside a code block is a Vue template: write it as `<code v-pre>{{ … }}</code>`.
- npm dependencies: at least 14 days old (`scripts/check-deps-age.mjs` runs before `npm ci`); install with
  `npm install --before=<date 14 days ago>`.
- Published: a release publishes `/v/X.Y.Z/` and (stable) the root; the `Docs` workflow publishes `main` to
  `/dev/` or a doc fix of the latest release to the root ([releasing](core/guides/releasing.md)).
