# rustcms — notes for AI agents

- Docs: `docs/guide/` is the PUBLIC developer guide (English mdBook); `docs/core/` is PRIVATE (architecture,
  protocol, ADRs in `docs/core/adr/`, reports, CHANGELOG). Every change updates the matching guide page.
- Commands: `just test-fast` (every test on SQLite, the inner loop), `just ci`, `just test-matrix` (starts PostgreSQL,
  MySQL and Redis with docker compose, in memory without fsync), `just test-cli` (slow scaffold test).
- Rust toolchain via Homebrew rustup: `export PATH=/opt/homebrew/opt/rustup/bin:$PATH`.
- Postgres for tests runs on port 55439 (55432 belongs to another project); MySQL 8.4 on 53306
  (`RUSTCMS_TEST_MYSQL=mysql://root:mysql@localhost:53306/mysql`); Redis on 56379
  (`RUSTCMS_TEST_REDIS=redis://localhost:56379`, tests need `--features rustcms/redis`, as `just test-matrix` does);
  S3Mock on 59000 (`RUSTCMS_TEST_S3=http://localhost:59000/rustcms-test`, `AWS_ACCESS_KEY_ID`/`AWS_SECRET_ACCESS_KEY`
  any value, `--features rustcms/s3`). MinIO no longer publishes images.
- Database tests must use `rustcms_testing::db_test!` so they run on SQLite, PostgreSQL and MySQL;
  tables built from entities go through `rustcms_testing::create_table`.
- Serializing transactions: `rustcms_core::db::xact_lock` (first lock of the transaction), never
  backend-specific advisory locks; migrations use `rustcms::migration` helpers (`create_table`,
  `instant`, …) so they re-run on MySQL (ADR 0010).
- `docs/guide/src/reference/cli.md` is generated: run `cargo run -p rustcms-cli -- docs:cli > docs/guide/src/reference/cli.md` after changing commands.
- App template lives in `crates/rustcms-cli/templates/app`; `examples/demo` and `examples/lms` follow its layout
  (ADR 0017: Laravel folder names inside `src/` — bootstrap, providers, routes, http, models, services, policies,
  jobs, settings, admin/{resources,pages,slots}, database/{migrations,seeders,factories}; tests in `tests/feature`;
  `resources/` for page templates, extension scripts/styles, logo).
- Custom admin HTML (ADR 0019): templates are always escaped then sanitized server-side
  (`features/custom/html.rs` allowlist); never add a way to mark template output safe or loosen the CSP.
- Where code lives: `docs/core/architecture.md` (modules grouped by role; old public paths are kept by re-exports,
  checked by `tests/paths.rs` in core, auth and admin — keep them passing).
- Security invariants: deny by default; undeclared columns never exposed; field rules
  and `scope()` enforced server-side on every path, including relationships and actions.
- Wire format (ADR 0003): i64/decimal as strings, decimals rescaled to column scale.
- Auth: all RBAC writes go through `RbacService` (crates/rustcms-auth/src/rbac/); never write role/permission tables directly.
- New auth routes must be added to `rustcms_auth::routes::routes` and to `fixtures/authz/auth.toml` (the oracle test fails otherwise).
- Admin API routes are listed in `rustcms_admin::http::routes`; each needs a case in `fixtures/authz/admin.toml`.
- Application API routes use `ApiRoutes` with an explicit `Access` per route (checked before extractors); never
  authorize inside a handler after reading the body. JSON routes for panel pages use `AdminRoutes` with an
  explicit `AdminAccess` (ADR 0026); `{admin}/api/x` is theirs, nothing built in may live there. Streams exist only
  as `AdminRoutes::events` with `LiveSession` first (ADR 0027): read-only, rechecked, bounded; never add raw SSE or
  WebSocket routes.
- Admin UI text goes through `admin-ui/src/i18n` (`t()`, lint `i18n/no-literal-text`); new server validation
  messages need a template in `rustcms_core::i18n::TEMPLATES` (checked by `crates/rustcms-admin/tests/issues.rs`).
- `fixtures/contract/*.json` is the server↔SPA contract: regenerate with `UPDATE_CONTRACT=1 cargo test -p rustcms-admin` and review the diff.
- Before review, check changes against `docs/core/review-checklist.md`; reviews are exhaustive and batched.
