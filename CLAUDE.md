# rustcms — notes for AI agents

- Plan and milestones: see `docs/adr/` and the approved plan (M0–M6 for 0.1).
- Commands: `just ci`, `just test-matrix` (needs `docker compose up -d --wait`).
- Rust toolchain via Homebrew rustup: `export PATH=/opt/homebrew/opt/rustup/bin:$PATH`.
- Postgres for tests runs on port 55439 (55432 belongs to another project).
- Database tests must use `db_test!` (`crates/rustcms-admin/tests/support`) so they run on both databases.
- Security invariants: deny by default; undeclared columns never exposed; field rules
  and `scope()` enforced server-side on every path, including relationships and actions.
- Wire format (ADR 0003): i64/decimal as strings, decimals rescaled to column scale.
