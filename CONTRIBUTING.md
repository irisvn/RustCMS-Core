# Contributing

## Setup

- Rust stable (`rust-toolchain.toml`), [`just`](https://github.com/casey/just), Docker.
- `docker compose up -d --wait` starts PostgreSQL on port `55439`
  (override with `RUSTCMS_PG_PORT`).

## Before opening a PR

`just ci` must pass. It runs fmt, clippy (`-D warnings`), every supported feature
combination, the test matrix (SQLite + PostgreSQL), docs, generated files and the
SPA tests. CI also runs `just test-cli`, `just e2e`, `just msrv` and the security
audit; run them when your change touches those areas.

## Definition of done

- New behaviour has tests, including denied and failing paths.
- Database tests use `db_test!` so they run on SQLite and PostgreSQL.
- Docs and rustdoc are updated; record architecture decisions in `docs/adr/`.
- `CHANGELOG.md` has an entry.
