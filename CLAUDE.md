# rustcms — notes for AI agents

- Plan and milestones: see `docs/adr/` and the approved plan (M0–M6 for 0.1).
- Commands: `just ci`, `just test-matrix` (starts PostgreSQL and MySQL with docker compose), `just test-cli` (slow scaffold test).
- Rust toolchain via Homebrew rustup: `export PATH=/opt/homebrew/opt/rustup/bin:$PATH`.
- Postgres for tests runs on port 55439 (55432 belongs to another project); MySQL 8.4 on 53306
  (`RUSTCMS_TEST_MYSQL=mysql://root:mysql@localhost:53306/mysql`).
- Database tests must use `rustcms_testing::db_test!` so they run on SQLite, PostgreSQL and MySQL;
  tables built from entities go through `rustcms_testing::create_table`.
- Serializing transactions: `rustcms_core::db::xact_lock` (first lock of the transaction), never
  backend-specific advisory locks; migrations use `rustcms::migration` helpers (`create_table`,
  `instant`, …) so they re-run on MySQL (ADR 0010).
- `docs/src/cli.md` is generated: run `cargo run -p rustcms-cli -- docs:cli > docs/src/cli.md` after changing commands.
- App template lives in `crates/rustcms-cli/templates/app`; `examples/demo` started from it and has since diverged (resources in `src/admin.rs`).
- Security invariants: deny by default; undeclared columns never exposed; field rules
  and `scope()` enforced server-side on every path, including relationships and actions.
- Wire format (ADR 0003): i64/decimal as strings, decimals rescaled to column scale.
- Auth: all RBAC writes go through `RbacService` (crates/rustcms-auth/src/rbac.rs); never write role/permission tables directly.
- New auth routes must be added to `rustcms_auth::routes::routes` and to `fixtures/authz/auth.toml` (the oracle test fails otherwise).
- Admin API routes are listed in `rustcms_admin::http::routes`; each needs a case in `fixtures/authz/admin.toml`.
- `fixtures/contract/*.json` is the server↔SPA contract: regenerate with `UPDATE_CONTRACT=1 cargo test -p rustcms-admin` and review the diff.
- Before review, check changes against `docs/review-checklist.md`; reviews are exhaustive and batched.
