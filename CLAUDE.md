# rustcms — notes for AI agents

- Plan and milestones: see `docs/adr/` and the approved plan (M0–M6 for 0.1).
- Commands: `just ci`, `just test-matrix` (needs `docker compose up -d --wait`), `just test-cli` (slow scaffold test).
- Rust toolchain via Homebrew rustup: `export PATH=/opt/homebrew/opt/rustup/bin:$PATH`.
- Postgres for tests runs on port 55439 (55432 belongs to another project).
- Database tests must use `rustcms_testing::db_test!` so they run on both databases.
- `docs/src/cli.md` is generated: run `cargo run -p rustcms-cli -- docs:cli > docs/src/cli.md` after changing commands.
- App template lives in `crates/rustcms-cli/templates/app`; `examples/demo` was generated from it.
- Security invariants: deny by default; undeclared columns never exposed; field rules
  and `scope()` enforced server-side on every path, including relationships and actions.
- Wire format (ADR 0003): i64/decimal as strings, decimals rescaled to column scale.
- Auth: all RBAC writes go through `RbacService` (crates/rustcms-auth/src/rbac.rs); never write role/permission tables directly.
- New auth routes must be added to `rustcms_auth::routes::routes` and to `fixtures/authz/auth.toml` (the oracle test fails otherwise).
- Admin API routes are listed in `rustcms_admin::http::routes`; each needs a case in `fixtures/authz/admin.toml`.
- `fixtures/contract/*.json` is the server↔SPA contract: regenerate with `UPDATE_CONTRACT=1 cargo test -p rustcms-admin` and review the diff.
