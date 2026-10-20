#!/bin/sh
# Starts the demo app on a fresh, seeded SQLite database for the E2E suite.
# RUSTCMS_DEMO_BIN: demo binary (default: debug build of the workspace).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
bin=${RUSTCMS_DEMO_BIN:-$here/../../target/debug/demo}
db_dir=$(mktemp -d)
export DATABASE_URL="sqlite://$db_dir/e2e.sqlite?mode=rwc"
export APP_URL=http://localhost:8123
export RUSTCMS_APP__LISTEN=127.0.0.1:8123
export APP_ENV=testing
export APP_KEY=${APP_KEY:-base64:ZTJlLXRlc3Qta2V5LW5vdC1zZWNyZXQtMzItYnl0ZXM=}
export RUSTCMS_AUTH__SESSION_LIFETIME_MINUTES=120
# Runtime branding (the site's name) shows within a second of a change.
export RUSTCMS_ADMIN__BRANDING_TTL_SECS=1
# The suite signs the same accounts in many times a minute (throttling has its own Rust tests).
export RUSTCMS_AUTH__ATTEMPTS_PER_IDENTIFIER=100
export RUSTCMS_AUTH__ATTEMPTS_PER_IP=300
export RUSTCMS_AUTH__ANONYMOUS_SESSIONS_PER_IP=1000
# Password resets and invitations, mailed to Mailpit (docker compose: SMTP 51025, API 58025).
export RUSTCMS_AUTH__PASSWORD_RESET=true
export RUSTCMS_AUTH__RESET_REQUESTS_PER_EMAIL=100
export RUSTCMS_AUTH__RESET_REQUESTS_PER_IP=100
export RUSTCMS_MAIL__DRIVER=smtp
export RUSTCMS_MAIL__SOURCE=config
export RUSTCMS_MAIL__ALLOW_INSECURE=true
export RUSTCMS_MAIL__ALLOWED_PORTS="[${RUSTCMS_SMTP_PORT:-51025}]"
export RUSTCMS_MAIL__SMTP__HOST=127.0.0.1
export RUSTCMS_MAIL__SMTP__PORT=${RUSTCMS_SMTP_PORT:-51025}
export RUSTCMS_MAIL__SMTP__ENCRYPTION=none
export RUSTCMS_MAIL__SMTP__FROM_ADDRESS=no-reply@demo.test
export RUSTCMS_MAIL__SMTP__FROM_NAME="Demo" 
# Run outside the checkout: the binary must not depend on source paths.
cd "$db_dir"
"$bin" migrate
"$bin" db:seed
exec "$bin" serve
