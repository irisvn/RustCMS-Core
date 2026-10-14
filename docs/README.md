# Documentation

| Folder | Audience | Published |
|---|---|---|
| [`guide/`](guide/) | developers building applications on RustCMS (English, mdBook) | **yes**: `mdbook build docs/guide` |
| [`core/`](core/) | the RustCMS team: architecture, protocol, decisions, reports, changelog | **no** (private) |

Rules:

- Every change updates the guide page of the behaviour it changes, in the same
  change. Internals (milestones, reviews, design trade-offs) go to `core/`,
  never to `guide/`.
- `guide/src/reference/cli.md` is generated:
  `cargo run -q -p rustcms-cli -- docs:cli > docs/guide/src/reference/cli.md`.
- `guide/src/reference/error-codes.md` must list every code of
  `rustcms_core::i18n::TEMPLATES` (checked by `crates/rustcms-admin/tests/issues.rs`).
