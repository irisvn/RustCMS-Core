# Developer commands. Install: `brew install just` (or `cargo install just`).
pg_url := env_var_or_default("RUSTCMS_TEST_PG", "postgres://postgres:postgres@localhost:55439/postgres")

default:
    @just --list

# Most of CI. Not included: the slow scaffold test (`just test-cli`), `just e2e`,
# `just msrv` (needs the 1.94 toolchain) and the security audit (cargo-deny, npm audit).
ci: lint features test-matrix doc generated test-ui

# Compiles with the declared minimum Rust version (`rustup toolchain install 1.94`).
msrv:
    cargo +1.94 check --workspace --all-targets

lint:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cd admin-ui && npm run lint && npm run typecheck

# Every supported feature combination compiles.
features:
    cargo check -p rustcms-core --no-default-features
    cargo check -p rustcms-core --no-default-features --features sqlite
    cargo check -p rustcms-core --no-default-features --features postgres
    cargo check -p rustcms-admin --no-default-features
    cargo check -p rustcms-admin --no-default-features --features sqlite
    cargo check -p rustcms-admin --no-default-features --features postgres
    cargo check -p rustcms-auth
    cargo check -p rustcms --no-default-features --features sqlite
    cargo check -p rustcms --no-default-features --features postgres
    cargo check -p rustcms --features s3

# Tests on SQLite (temp files) and PostgreSQL (docker compose).
test-matrix:
    docker compose up -d --wait postgres
    RUSTCMS_TEST_PG={{pg_url}} cargo nextest run --workspace

test:
    cargo nextest run --workspace

# `rustcms new` + generators produce an app that builds and passes its tests (slow).
test-cli:
    cargo test -p rustcms-cli --test scaffold -- --include-ignored

# Drop PostgreSQL template databases (after editing an unreleased migration in place).
test-reset-db:
    docker compose exec -T postgres psql -U postgres -tAc "select 'drop database ' || datname || ';' from pg_database where datname like 'rustcms_t%'" | docker compose exec -T postgres psql -U postgres

doc:
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
    cargo test --workspace --doc
    mdbook build docs

# Generated files are up to date (CLI docs, TypeScript types, built admin assets).
generated:
    cargo run -q -p rustcms-cli -- docs:cli > docs/src/cli.md
    UPDATE_TYPES=1 cargo test -q -p rustcms-admin --features ts --test typescript
    cd admin-ui && npm run build
    test -z "$(git status --porcelain -- docs/src/cli.md admin-ui/src/api/types.gen.ts crates/rustcms-admin/assets fixtures)"

# Admin SPA: install dependencies (Node 20+).
admin-install:
    cd admin-ui && npm ci

# Build the admin SPA into crates/rustcms-admin/assets (commit the result).
admin-build:
    cd admin-ui && npm run build

# Admin SPA unit tests (Vitest).
test-ui:
    cd admin-ui && npm test

# Browser tests against the demo app (needs `npx playwright install chromium` once).
e2e:
    cargo build -p demo
    cd admin-ui && npm run e2e

# Rust server on :8000 and Vite on :5173 (open http://localhost:5173/admin/).
dev:
    #!/usr/bin/env sh
    trap 'kill 0' EXIT
    (cd examples/demo && RUSTCMS_AUTH__ADMIN_ORIGINS='["http://localhost:8000","http://localhost:5173"]' cargo run -- serve) &
    (cd admin-ui && npm run dev) &
    wait

# Every crate packages and verifies for crates.io (no upload).
publish-check:
    cargo publish --workspace --exclude demo --dry-run
