# Changelog

All notable changes are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- M1 core: layered configuration (`config/*.toml`, `.env`, env vars) with required
  `APP_KEY`; `AppBuilder`, `ServiceProvider` (`register`/`boot`), `Inject<T>`;
  HTTP stack (request id, tracing, trusted-proxy client IP, timeout, body limit,
  JSON 404/405, `/health`); `rustcms::Json`; `Console` with `serve`, `migrate*`,
  `key:generate`.
- `rustcms-migration`: `users` and `sessions` tables.
- `rustcms-testing`: per-test SQLite/PostgreSQL databases (PostgreSQL templates),
  `db_test!`, `TestApp`, `Factory`/`UserFactory`.
- `rustcms` facade crate and `rustcms` CLI: `new`, `make:model`, `make:migration`,
  `docs:cli`, forwarding of app commands.
- `examples/demo` generated with `rustcms new`.

### Fixed (Codex review)
- Migrations (framework and generated) are transactional on SQLite too, so a failed
  run leaves no partial schema and can be retried.
- `key:generate` replaces every dotenv form of `APP_KEY` (`export`, spaces, quotes,
  duplicates) instead of appending a definition that the old one shadows.
- The body limit wraps every request body (Content-Length and chunked), not only
  extractors; all 413 responses use the JSON error format.
- Docs: getting started, configuration, basics, generated CLI reference, ADR 0004.
- M0: workspace, CI (lint, features, tests on SQLite + PostgreSQL, docs, cargo-deny).
- `rustcms-core`: `AppError` with Laravel-compatible JSON errors, `Principal` with
  super-admin bypass bounded by token abilities.
- `rustcms-admin`: strict wire format codec, `Resource` trait, type-erased generic CRUD
  engine (key strategies, field permissions, scope, relationships, actions, hooks in
  transactions, constraint mapping, request limits).
- Docs: mdBook skeleton, admin protocol, ADR 0001–0003.
