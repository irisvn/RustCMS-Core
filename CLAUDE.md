# rustcms — notes for AI agents

- Docs: `docs/guide/` is the PUBLIC developer guide (English mdBook); `docs/core/` is PRIVATE (architecture,
  protocol, ADRs in `docs/core/adr/`, reports, CHANGELOG). Every change updates the matching guide page.
- Commands: `just test-fast` (every test on SQLite, the inner loop), `just ci`, `just test-matrix` (starts PostgreSQL,
  MySQL, Redis, S3Mock and Mailpit with docker compose, in memory without fsync), `just test-cli` (slow scaffold test),
  `just generated` (CLI docs, TS types, admin build, SDK copies: must leave git clean), `just e2e`.
- Keep the machine usable: `CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0
  CARGO_PROFILE_TEST_DEBUG=0`, 4 test threads, long runs in the background; check `df -h` before big builds and
  `cargo clean` when space is low.
- E2E (`admin-ui/e2e`) runs on Chrome only (`PLAYWRIGHT_CHANNEL=chrome`), one worker as configured (specs share the
  demo's users); every spec uses the `watch` fixture (CSP violations and console errors fail it). Leave nothing
  behind (a failed spec's records break count-based specs). In editors, reach a paragraph next to a block node
  (image, embed, table) with the keyboard: a click can select the node on CI's Chromium.
- Rust toolchain via Homebrew rustup: `export PATH=/opt/homebrew/opt/rustup/bin:$PATH`.
- Postgres for tests runs on port 55439 (55432 belongs to another project); MySQL 8.4 on 53306
  (`RUSTCMS_TEST_MYSQL=mysql://root:mysql@localhost:53306/mysql`); Redis on 56379
  (`RUSTCMS_TEST_REDIS=redis://localhost:56379`, tests need `--features rustcms/redis`, as `just test-matrix` does);
  S3Mock on 59000 (`RUSTCMS_TEST_S3=http://localhost:59000/rustcms-test`, `AWS_ACCESS_KEY_ID`/`AWS_SECRET_ACCESS_KEY`
  any value, `--features rustcms/s3`); Mailpit SMTP on 51025 and 51026 (TLS; `just test-tls-cert`). MinIO no longer
  publishes images.
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
- Secrets in app tables use `rustcms::crypto::Encrypted<T>` (ADR 0030): explicit `seal`/`open` with the keyring;
  never put plaintext or envelopes in `record()`, the wire, logs or the audit log; admin fields use
  `Field::encrypted`/`encrypted_map`.
- Periodic work uses `AppBuilder::schedule` (ADR 0031: slots claimed once in the database, run only by `serve`);
  never add new `tokio::spawn` loops in providers.
- Wire format (ADR 0003): i64/decimal as strings, decimals rescaled to column scale.
- Auth: all RBAC writes go through `RbacService` (crates/rustcms-auth/src/rbac/); never write role/permission tables directly.
- New auth routes must be added to `rustcms_auth::routes::routes` and to `fixtures/authz/auth.toml` (the oracle test fails otherwise).
- Admin API routes are listed in `rustcms_admin::http::routes`; each needs a case in `fixtures/authz/admin.toml`.
- Application API routes use `ApiRoutes` with an explicit `Access` per route (checked before extractors); never
  authorize inside a handler after reading the body. JSON routes for panel pages use `AdminRoutes` with an
  explicit `AdminAccess` (ADR 0026); `{admin}/api/x` is theirs, nothing built in may live there. Streams exist only
  as `AdminRoutes::events` with `LiveSession` first (ADR 0027): read-only, rechecked, bounded; sockets only as
  `AdminRoutes::socket` with `LiveSocket` (ADR 0029): commands call `AdminSocket::authorize` when they run; never add
  raw SSE or WebSocket routes.
- v2 extensions (ADR 0032): rendered by the host, never `createRoot`; host modules only through the ids the SDK
  preset maps (other dependencies and local modules are bundled; framework subpaths and dynamic imports are refused);
  built `resources/admin/{js,css}` are committed; every extension path goes through `admin-ui/src/lib/paths.ts`.
  `admin-ui/sdk` is canonical: copy it to `crates/rustcms-cli/templates/sdk` and `examples/demo/resources/admin/sdk`
  (`just generated` checks).
- Admin UI text goes through `admin-ui/src/i18n` (`t()`, lint `i18n/no-literal-text`); new server validation
  messages need a template in `rustcms_core::i18n::TEMPLATES` (checked by `crates/rustcms-admin/tests/issues.rs`).
- `fixtures/contract/*.json` is the server↔SPA contract: regenerate with `UPDATE_CONTRACT=1 cargo test -p rustcms-admin` and review the diff.
  Changing the schema vocabulary (input/column/filter kinds, new schema fields the SPA must understand) bumps
  `VOCABULARY_VERSION` (`crates/rustcms-admin/src/contract/mod.rs`); TS types: `UPDATE_TYPES=1 cargo test -p
  rustcms-admin --features ts --test typescript`.
- The built admin SPA in `crates/rustcms-admin/assets` is committed (embedded in the binary): rebuild it
  (`just admin-build`) after any `admin-ui/src` change and after merges (a merge can keep a stale bundle), and
  commit it separately ("rebuilt admin assets").
- Rich text (ADR 0008): the document is a closed, versioned schema validated and bounded on the server
  (`features/rich_text`); never store HTML, remote images or iframes; the editor writes the lowest version a
  document needs, so older readers keep working.
- Lock order across resources: parents before children and belongs-to-many targets (`Registry::lock_rank`, ADR
  0018/0020/0035); a write that locks rows of several resources locks them in that order, keys in key order.
  Media references (resource fields, rich text, settings) are checked by `media::check_references`.
- Dependencies (crates and npm) must be at least two weeks old, unless the owner approves an exception (e.g. a
  security fix flagged by the `security` CI job).
- Releases: bump the workspace version (`Cargo.toml` version + `[workspace.dependencies]`, examples, the three SDK
  `VERSION` files, install/deploy pins, release-notes "current version"), date the CHANGELOG section, add a
  release-notes entry and an upgrade.md "From X to Y" section (migrations to run, rollback notes, breaking changes).
  The tag must point at a commit whose CI on `main` passed (the Release workflow checks).
- Before review, check changes against `docs/core/review-checklist.md`; reviews are exhaustive and batched.
