# Changelog

All notable changes are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- M3 resource engine: Filament-style builders (`TextInput`, `SelectInput`,
  `Toggle`, …, `Section`, `Grid`, table columns and filters), action metadata
  (`label`, `bulk`, `requires_confirmation`, …); versioned `ResourceSchema`
  filtered by field permissions per operation; `AdminProvider` with the admin
  API (`panel`, `schema`, records CRUD, actions, relationship options) behind
  the admin guard; resource permissions declared for `permission:sync`;
  `Field::max_length` / `label`; case-insensitive escaped search; typed
  query-string filters; action ability required before per-record checks;
  `make:test`; contract fixture `fixtures/contract/posts.schema.json`;
  authorization oracle `fixtures/authz/admin.toml`.
- New apps register `AdminProvider`.

### Fixed (Codex review of M3)
- Relationship options are available for every operation the user may perform
  with the field (create/update, not only `view_any`); edit forms pass
  `record=<key>` so record-level policies (ownership) decide on that record.
- `max_length` is re-checked on the values hooks produce, before writing.
- Search folds case with the database's `LOWER` on both sides (exact non-ASCII
  text matches on SQLite) and uses table-qualified columns (safe with joined scopes).
- Action names may not reuse CRUD ability names (`view`, `update`, …).
- Record-checked lists and option lookups read all batches from one snapshot
  (`db::begin_snapshot`); option lookups that exhaust their scan budget return a
  422 instead of an incomplete list; search casts columns to text (numeric/UUID
  titles on PostgreSQL).
- Record-checked scans read only keys (bounded) in a snapshot, release it, then
  load records 100 at a time in that order: memory does not grow with row size,
  and policies or label lookups never wait on the scan's connection (works with
  one connection and database-backed policies).
  Key scans keep the query's own projection (100 rows per batch, only keys
  retained), so `DISTINCT`/`ORDER BY` from `scope()` stay valid on PostgreSQL.
  Reloaded records are re-checked against the full query (scope, search,
  filters), so concurrent edits cannot slip non-matching records into results.
- Search on UUID columns matches the displayed (hyphenated) form on SQLite.
- A committed create/update is always reported as a success: relationship
  labels are best effort, and a failing or slow envelope falls back to the key
  alone, within 2 s and within the request's own deadline
  (`http::request_deadline`).
  Optional sliding session renewal is bounded the same way (skipped if it would
  outlast the request).
- Writes lock only target rows found in scope and allowed by the policy
  (PostgreSQL `FOR UPDATE`), then re-check scope and policy under the lock, so
  ownership changed concurrently is seen and hidden records are never locked; policies
  and hooks query `Ctx::conn()`, which is the write's own transaction during
  writes (no second connection needed, no deadlock with small pools).
- Edit schemas keep write-only fields (writable, not readable) as empty,
  `writeOnly` inputs; their values are never returned.
- Envelopes carry data only when the user may view the record (blind updates
  return key and abilities); page offsets are checked for overflow (422).
- Lists exclude (and do not count) records a record-level policy denies `view`;
  `Policy::view_depends_on_record` keeps SQL pagination for scope-only policies.
- `permission:sync` declares action admission permissions; relationship options
  skip policy-denied candidates instead of returning an empty first batch.
- Actions are authorized per record; `Action::requires_permission` declares an
  explicit admission permission instead of inferring it from `allows(…, None)`.
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
