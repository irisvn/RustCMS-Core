# Docs v2 — brief for the agent rewriting a batch of guide pages

Repo: RustCMS-Core. Guide: `docs/guide/src` (VitePress 1.6.4, conventions in `docs/README.md`). Plan:
`docs/core/plans/docs-v2-vitepress.md`. Inventory of what is wrong/missing per page:
`docs/core/reports/docs-v2-inventory/*.md` — read the rows of your pages first, then verify every claim in the
source yourself (the inventory can be wrong).

## Audience and tone

Developers building an application on RustCMS (Rust, some Laravel/Filament background). English, direct, short
sentences, present tense, "you". No marketing words. Explain *what it does, when to use it, how*, then the
details. Keep the security invariants explicit (deny by default; undeclared columns never exposed; field rules and
`scope()` enforced server-side on every path).

## Page template

1. `# Title` + 1–3 sentences: what it is and when to use it.
2. Screenshot placeholder where the page has UI: `![alt text](/img/<section>/<id>.webp)` followed by an italic
   caption line saying where it comes from, e.g. `*RBlog, a blog built on RustCMS: the posts table.*` — and add an
   entry to the list `SCREENSHOTS NEEDED` in your final report (id, site rblog|rdp|local, admin path, what to
   show, element to crop if any). Do not create image files; the maintainer captures them.
3. **Quick start**: the smallest working code.
4. **Full example**: imported from compiled code when it exists:
   `<<< @/../../../examples/demo/src/<file>.rs#<region> [src/<file>.rs]`
   Add `// #region <name>` / `// #endregion <name>` comment lines (only comments, no code change) around the item
   in `examples/demo` or `examples/lms`. Region names: lowercase, `-` allowed. Short shell/TOML/env snippets may
   stay inline. Hand-written Rust is allowed only when no example has it; it must use the current API exactly
   (check signatures in the source) and be marked as a sketch if it does not compile on its own.
5. Options / reference: tables of builder methods, config keys (with defaults), CLI flags — names exactly as in
   code.
6. Security notes (link to `admin/authorization.md` / `security/*` instead of repeating).
7. Pitfalls / limits.
8. Related pages (links).

VitePress features to use where they help: `::: tip`, `::: warning`, `::: danger`, `::: details`, `::: code-group`
(e.g. `config/x.toml` vs env vars), tables.

## Hard rules

- Do not rename a page file. Keep every heading that other pages link to (grep `<page>.md#`) with the same text,
  or add `{#old-id}` after a changed heading so its id stays (ids follow mdBook: lowercase, letters/digits/`_`/`-`
  kept, spaces → `-`, other characters dropped).
- Navigation: a new page goes into `docs/guide/.vitepress/sidebar.ts`.
- Literal `{{ … }}` outside code blocks: `<code v-pre>{{ … }}</code>`.
- Never put real credentials, emails or hostnames other than `example.com` in samples.
- Every page you touch must reflect the source at HEAD; cite nothing internal (no ADR numbers, plans, reports) in
  the guide — the guide is public.
- Do not change Rust code except adding region comment lines. Do not change `reference/cli.md` (generated).

## Checks before you finish

```sh
# Other agents build at the same time: use your own output directory.
cd docs/guide && DOCS_OUT=/tmp/guide-$$ npm run build   # snippet regions, dead links, anchors
cd ../.. && export PATH=/opt/homebrew/opt/rustup/bin:$PATH CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
cargo check -q -p demo -p lms --all-targets   # region comments did not break anything
cargo fmt --all --check
```

Do not commit. Final report: pages changed, what was wrong and fixed (with source file:line), new sections,
regions added, SCREENSHOTS NEEDED list, open questions.
