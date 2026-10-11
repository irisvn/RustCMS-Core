# Developer commands. Install: `brew install just` (or `cargo install just`).
pg_url := env_var_or_default("RUSTCMS_TEST_PG", "postgres://postgres:postgres@localhost:55439/postgres")

default:
    @just --list

# Format, lint, test on SQLite + PostgreSQL, docs: what CI runs.
ci: lint features test-matrix doc

lint:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets --features rustcms-admin/postgres -- -D warnings

# Every supported feature combination compiles.
features:
    cargo check -p rustcms-admin --no-default-features
    cargo check -p rustcms-admin --no-default-features --features sqlite
    cargo check -p rustcms-admin --no-default-features --features postgres
    cargo check -p rustcms-admin --features postgres

# Tests on SQLite (temp files) and PostgreSQL (docker compose).
test-matrix:
    docker compose up -d --wait postgres
    RUSTCMS_TEST_PG={{pg_url}} cargo test --workspace --features rustcms-admin/postgres

test:
    cargo test --workspace

doc:
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
    cargo test --workspace --doc
    mdbook build docs
