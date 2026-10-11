# Developer commands. Install: `brew install just` (or `cargo install just`).
pg_url := env_var_or_default("RUSTCMS_TEST_PG", "postgres://postgres:postgres@localhost:55439/postgres")

default:
    @just --list

# Everything CI runs, except the slow scaffold test (`just test-cli`).
ci: lint features test-matrix doc generated

lint:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings

# Every supported feature combination compiles.
features:
    cargo check -p rustcms-core --no-default-features
    cargo check -p rustcms-core --no-default-features --features sqlite
    cargo check -p rustcms-core --no-default-features --features postgres
    cargo check -p rustcms-admin --no-default-features
    cargo check -p rustcms-admin --no-default-features --features sqlite
    cargo check -p rustcms-admin --no-default-features --features postgres
    cargo check -p rustcms --no-default-features --features sqlite
    cargo check -p rustcms --no-default-features --features postgres

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

# Generated files are up to date.
generated:
    cargo run -q -p rustcms-cli -- docs:cli > docs/src/cli.md
    git diff --exit-code -- docs/src/cli.md
