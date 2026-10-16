# Developer commands. Install: `brew install just` (or `cargo install just`).
pg_url := env_var_or_default("RUSTCMS_TEST_PG", "postgres://postgres:postgres@localhost:55439/postgres")
mysql_url := env_var_or_default("RUSTCMS_TEST_MYSQL", "mysql://root:mysql@localhost:53306/mysql")
redis_url := env_var_or_default("RUSTCMS_TEST_REDIS", "redis://localhost:56379")
s3_url := env_var_or_default("RUSTCMS_TEST_S3", "http://localhost:59000/rustcms-test")

default:
    @just --list

# Most of CI. Not included: the slow scaffold test (`just test-cli`), `just e2e`,
# `just msrv` (needs the 1.94 toolchain) and the security audit (cargo-deny, npm audit).
ci: lint features test-matrix doc generated test-ui

# Compiles with the declared minimum Rust version (`rustup toolchain install 1.94`).
msrv:
    cargo +1.94 check --workspace --all-targets
    cargo +1.94 check -p rustcms --features redis,s3

lint:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo clippy --workspace --all-targets --features rustcms/redis,rustcms/s3 -- -D warnings
    cd admin-ui && npm run lint && npm run typecheck

# Every supported feature combination compiles.
features:
    cargo check -p rustcms-core --no-default-features
    cargo check -p rustcms-core --no-default-features --features sqlite
    cargo check -p rustcms-core --no-default-features --features postgres
    cargo check -p rustcms-core --no-default-features --features mysql
    cargo check -p rustcms-admin --no-default-features
    cargo check -p rustcms-admin --no-default-features --features sqlite
    cargo check -p rustcms-admin --no-default-features --features postgres
    cargo check -p rustcms-admin --no-default-features --features mysql
    cargo check -p rustcms-auth
    cargo check -p rustcms --no-default-features --features sqlite
    cargo check -p rustcms --no-default-features --features postgres
    cargo check -p rustcms --no-default-features --features mysql
    cargo check -p rustcms --features s3
    cargo check -p rustcms --features redis
    cargo check -p rustcms-auth --features redis
    cargo check -p rustcms-core --no-default-features --features redis
    cargo check -p rustcms-core --no-default-features --features mail
    cargo check -p rustcms-admin --no-default-features --features sqlite,mail
    cargo check -p rustcms --no-default-features --features sqlite

# Tests on SQLite (temp files), PostgreSQL and MySQL, with Redis and an S3-compatible bucket (docker compose).
# A test CA and a certificate it signs for `localhost` (the TLS mail test server), made once.
test-tls-cert:
    #!/usr/bin/env sh
    set -eu
    test -f .test-tls/ca.pem && exit 0
    mkdir -p .test-tls && cd .test-tls
    openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -subj "/CN=rustcms test CA" -keyout ca-key.pem -out ca.pem 2>/dev/null
    openssl req -newkey rsa:2048 -nodes -subj "/CN=localhost" -keyout key.pem -out server.csr 2>/dev/null
    printf "subjectAltName=DNS:localhost\nbasicConstraints=CA:FALSE\nextendedKeyUsage=serverAuth\n" > server.ext
    openssl x509 -req -in server.csr -CA ca.pem -CAkey ca-key.pem -CAcreateserial -days 3650 -extfile server.ext -out cert.pem 2>/dev/null
    chmod 644 key.pem

test-matrix: test-tls-cert
    docker compose up -d --wait postgres mysql redis s3 mailpit mailpit-tls
    @for i in $(seq 1 60); do curl -sf --connect-timeout 2 -m 5 -o /dev/null {{s3_url}} && exit 0; sleep 1; done; echo "the S3 bucket is not ready: {{s3_url}}"; exit 1
    RUSTCMS_TEST_PG={{pg_url}} RUSTCMS_TEST_MYSQL={{mysql_url}} RUSTCMS_TEST_REDIS={{redis_url}} RUSTCMS_TEST_S3={{s3_url}} RUSTCMS_TEST_SMTP=127.0.0.1:51025 RUSTCMS_TEST_SMTP_TLS=localhost:51026 RUSTCMS_TEST_SMTP_TLS_CA={{justfile_directory()}}/.test-tls/ca.pem AWS_ACCESS_KEY_ID=test AWS_SECRET_ACCESS_KEY=test RUSTCMS_TEST_REQUIRE=postgres,mysql,redis,s3,smtp,smtp_tls cargo nextest run --workspace --features rustcms/redis,rustcms/s3

test:
    cargo nextest run --workspace

# `rustcms new` + generators produce an app that builds and passes its tests (slow).
test-cli:
    cargo test -p rustcms-cli --test scaffold -- --include-ignored

# Drop PostgreSQL and MySQL template databases (after editing an unreleased migration in place).
test-reset-db:
    docker compose exec -T postgres psql -U postgres -tAc "select 'drop database ' || datname || ';' from pg_database where datname like 'rustcms_t%'" | docker compose exec -T postgres psql -U postgres
    docker compose exec -T mysql mysql -uroot -pmysql -N -e "select concat('drop database ', schema_name, ';') from information_schema.schemata where schema_name like 'rustcms\_t%'" | docker compose exec -T mysql mysql -uroot -pmysql

doc:
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
    cargo test --workspace --doc
    mdbook build docs/guide

# Generated files are up to date (CLI docs, TypeScript types, built admin assets).
generated:
    cargo run -q -p rustcms-cli -- docs:cli > docs/guide/src/reference/cli.md
    UPDATE_TYPES=1 cargo test -q -p rustcms-admin --features ts --test typescript
    cd admin-ui && npm run build
    test -z "$(git status --porcelain -- docs/guide/src/reference/cli.md admin-ui/src/api/types.gen.ts crates/rustcms-admin/assets fixtures)"

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

# Every version of the workspace (X.Y.Z or X.Y.Z-rc.N) and Cargo.lock; then tag `vX.Y.Z`.
release-bump version:
    python3 scripts/release_version.py bump {{version}}

# The tag matches every version (run by the release workflow).
release-check tag:
    python3 scripts/release_version.py check {{tag}}
