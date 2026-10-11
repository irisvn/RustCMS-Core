# Changelog

All notable changes are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- M2 auth (`rustcms-auth`): admin guard with server-side sessions (hashed ids,
  regeneration, sliding expiry, pruning), synchronizer CSRF + Origin allowlist;
  api guard with personal access tokens (`rcms_…`, hashed, abilities, expiry,
  revocation); deterministic guard selection with no fallback; Argon2id on a
  bounded blocking pool; sign-in throttling per account+IP and per IP;
  credential revocation via `auth_version`; roles/permissions per guard with a
  cached resolver; `RbacService` enforcing anti-escalation rules and the
  last-super-admin invariant under a lock; `Gate`; auth endpoints; commands
  `make:user`, `token:create`, `permission:sync`, `session:prune`.
- Migrations: `personal_access_tokens`, RBAC tables, seeded `super_admin` roles.
- Core: `AppCommand` (provider console commands), `AppBuilder::middleware` and
  `::service`, `AppError::TooManyRequests` with `Retry-After`.
- Testing: `TestClient` with `acting_as`, `login_as`, `with_token`;
  `UserFactory::password`; authorization oracle `fixtures/authz/auth.toml`.
- New apps register `AuthProvider` and ship `config/auth.toml`.
- Docs: security section (authentication, roles & permissions, revocation,
  production checklist), ADR 0005.

### Fixed (Codex review of M2)
- Password change, sign-out everywhere and token issuance re-validate the
  verified credential snapshot under a user-row lock (no revival of revoked
  sessions, no token outliving a concurrent password change).
- Sign-in throttling reserves attempts before Argon2 verification; concurrent
  bursts can no longer exceed the limit.
- Permission cache keyed by RBAC generation: in-flight loads cannot undo eviction.
- A revoked session cookie can obtain a CSRF token and sign in again.
- RBAC changes are serialized by a transaction-scoped lock (`rbac::lock`) and
  authorize with the actor's grants re-read under it: concurrent role edits
  cannot restore revoked permissions, and promotions racing removals cannot
  bypass the last-super-admin rule (PostgreSQL READ COMMITTED).
- A session ended by a concurrent request no longer turns the other request's
  committed response into a 500 (conditional renewal, no revival).
- Argon2 permits move into the blocking work: cancelled requests cannot start
  more concurrent hashes than configured.
- `make:user` is atomic (`RbacService::create_user`): a failed super-admin
  assignment leaves no half-created account.
- Anonymous session creation (`GET csrf`) is throttled per IP and globally, and
  anonymous sessions have a fixed 15-minute lifetime (never extended): storage
  cannot be flooded.
- Credential restamps never re-create a session signed out concurrently:
  sessions carry a `lineage`; re-issue and sign-out take a per-lineage lock, and
  sign-out ends the whole lineage (any number of concurrent re-issues).
- Session renewal failures no longer replace a committed handler response.
- Rate limiter memory is bounded: oversized emails/passwords rejected early,
  keys hashed to a fixed size, expired windows reclaimed, capacity capped.
- SQLite write transactions use `BEGIN IMMEDIATE` (`db::begin_write`): concurrent
  writers wait instead of failing with "database is locked".
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
