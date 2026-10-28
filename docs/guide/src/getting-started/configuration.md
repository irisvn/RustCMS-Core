# Configuration

Configuration is read once, when a command starts, from `config/*.toml`,
`.env` and the environment. Values that need a deploy to change go here;
values admins change at run time go in [settings](../admin/settings.md).

## Quick start

A new application has `config/app.toml`, `auth.toml`, `http.toml` and
`log.toml`, and a `.env`. Change a value in the file, or override it from the
environment:

::: code-group

```toml [config/http.toml]
# Maximum request body (bytes) and timeout (seconds).
body_limit = 2097152
timeout_secs = 30
```

```sh [environment]
export RUSTCMS_HTTP__BODY_LIMIT=2097152
rustcms serve
# or for one command: RUSTCMS_HTTP__BODY_LIMIT=2097152 rustcms serve
```

:::

## Sources and precedence

Sources, last one wins:

1. built-in defaults,
2. `config/<section>.toml`: each file becomes the section named after it
   (`config/http.toml` is `[http]`); a missing file keeps the defaults,
3. well-known variables: `APP_NAME`, `APP_ENV`, `APP_DEBUG`, `APP_URL`,
   `APP_KEY`, `APP_PREVIOUS_KEYS`, `DATABASE_URL`, `REDIS_URL`,
4. `RUSTCMS_<SECTION>__<KEY>`, e.g. `RUSTCMS_HTTP__BODY_LIMIT=2097152`.

`.env` is loaded first and never overrides variables already set in the
environment. `config/` is read from the working directory; `.env` is looked
for in the working directory, then in its parents.

## Framework sections

| Key                         | Default                | Notes                                        |
|-----------------------------|------------------------|----------------------------------------------|
| `app.name`                  | `rustcms`              | the default Redis prefix is `rustcms:<app.name>` |
| `app.env`                   | `production`           | `local`, `testing`, `production`: see [The environment](#the-environment) |
| `app.debug`                 | `false`                | read by your code only: see [The environment](#the-environment) |
| `app.url`                   | `http://localhost:8000`| public URL; its origin may change state in the panel by default |
| `app.key`                   | — (**required**)       | `base64:` + 32 bytes; `rustcms key:generate` |
| `app.previous_keys`         | `[]`                   | keys `APP_KEY` replaced, only to open [encrypted values](../digging-deeper/encryption.md) until `key:reencrypt` ran; env `APP_PREVIOUS_KEYS` (comma-separated) |
| `app.listen`                | `127.0.0.1:8000`       | address of `serve` (`serve --listen` overrides) |
| `app.locale`                | `en`                   | admin interface language by default ([Languages](../admin/i18n.md)); must be one of `app.locales` |
| `app.locales`               | `["en", "vi"]`         | languages users may choose (the panel ships `en`, `vi`) |
| `database.url`              | — (**required**)       | `sqlite://…`, `postgres://…` or `mysql://…` ([MySQL setup](../basics/database.md#mysql)) |
| `database.max_connections`  | `10`                   | pool size                                    |
| `database.log_statements`   | `false`                | logs every SQL statement, at info level      |
| `database.allow_newer_migrations` | `false`          | `migrate` accepts migrations a newer version applied (rollbacks; [additive migrations only](deployment.md#upgrading)) |
| `http.body_limit`           | `1048576`              | bytes; larger bodies get 413                 |
| `http.timeout_secs`         | `30`                   | slower requests get 408                      |
| `http.trusted_proxies`      | `[]`                   | CIDRs allowed to set `X-Forwarded-For` ([Client IP](../architecture/lifecycle.md#client-ip-and-proxies)) |
| `log.filter`                | `info`                 | a `tracing` filter (`info,blog=debug`); `RUST_LOG` overrides |
| `log.json`                  | `false`                | JSON lines for log shippers                  |

Loading fails with a message naming the key when `APP_KEY` is missing or
malformed, when a previous key repeats another key, or when `app.locale` is
not one of `app.locales` (tags such as `en`, `vi`).

New applications start from these files (the rest are defaults):

::: details The generated `config/` files

```toml
# config/app.toml (overridden by APP_* variables in .env)
name = "blog"
listen = "127.0.0.1:8000"

# config/auth.toml
admin_path = "/admin"
admin_origins = []
session_lifetime_minutes = 120
password_min_length = 8
revoke_tokens_on_password_change = true

# config/http.toml
body_limit = 1048576
timeout_secs = 30
trusted_proxies = []

# config/log.toml (RUST_LOG overrides it)
filter = "info,blog=debug"
json = false
```

:::

### `[storage]` (`config/storage.toml`)

Uploads and the media library: see [File storage](../digging-deeper/storage.md).

### `[auth]` (`config/auth.toml`)

| Key                                  | Default            |                                         |
|--------------------------------------|--------------------|-----------------------------------------|
| `auth.admin_path`                    | `/admin`           | panel and session guard prefix, cookie path |
| `auth.admin_origins`                 | `[]` → `APP_URL`   | origins allowed to change state         |
| `auth.cookie_name`                   | `rustcms_session`  |                                         |
| `auth.secure_cookies`                | https `APP_URL`    | unset: `Secure` cookies when `APP_URL` is https |
| `auth.session_lifetime_minutes`      | `120`              | idle, sliding                           |
| `auth.anonymous_session_lifetime_minutes` | `15`          | sessions holding only a CSRF token      |
| `auth.anonymous_sessions_per_ip`     | `30`               | new anonymous sessions per IP per minute |
| `auth.anonymous_sessions_per_minute` | `1000`             | new anonymous sessions per minute, total |
| `auth.password_min_length`           | `8`                |                                         |
| `auth.revoke_tokens_on_password_change` | `true`          |                                         |
| `auth.attempts_per_identifier`       | `5`                | failed sign-ins per account+IP per minute |
| `auth.attempts_per_ip`               | `30`               | failed sign-ins per IP per minute       |
| `auth.permission_cache_ttl_secs`     | `30`               | upper bound; changes apply on the next request |
| `auth.remember_days`                 | `30`               | "Remember me" keeps a session this long (0: no such box) |
| `auth.password_reset`                | `false`            | "Forgot password?" (needs mail)         |
| `auth.password_reset_minutes`        | `60`               | how long a reset link works             |
| `auth.invitation_days`               | `7`                | how long an invitation link works (1–30) |
| `auth.reset_requests_per_ip`         | `5`                | reset requests per IP per minute        |
| `auth.reset_requests_per_email`      | `3`                | reset requests per address per minute   |
| `auth.reset_cooldown_secs`           | `60`               | at most one reset mail per user this often |
| `auth.jwt.enabled`                   | `true`             | JWT sign-in endpoints and bearer JWTs   |
| `auth.jwt.access_ttl_secs`           | `900`              | access token lifetime (60–3600)         |
| `auth.jwt.refresh_ttl_days`          | `14`               | an unused refresh token expires (1–90)  |
| `auth.jwt.refresh_family_days`       | `30`               | a sign-in ends, however often refreshed (≥ refresh_ttl_days, ≤ 365) |

- `auth.admin_path` is one or more segments of letters, digits, `_` and `-`
  (`/admin`, `/back-office/panel`); a trailing `/` is removed. `/` and paths
  under `/api` are refused.
- A value out of its range (`invitation_days`, the `jwt` lifetimes) or a bad
  `admin_path` stops the application as soon as it is assembled, with a
  message naming the key: every command that assembles it fails (all but
  `key:generate`, `migrate`, `migrate:rollback`, `migrate:refresh`,
  `migrate:status`, and `migrate:fresh` without `--seed`).
- `auth.password_reset = true` without a mail driver stops the boot.

See [Authentication](../security/authentication.md).

### `[admin]` (`config/admin.toml`)

[Runtime branding](../admin/customizing.md#runtime-branding) of the panel.

| Key | Default | |
|---|---|---|
| `admin.branding` | `"runtime"` | `"static"` ignores the `BrandingSource` (recovery: `RUSTCMS_ADMIN__BRANDING=static`) |
| `admin.branding_ttl_secs` | `30` | how long an instance keeps the loaded branding (1–3600) |
| `admin.branding_timeout_ms` | `2000` | a slower load shows the static theme (1–30000) |

### `[audit]` (`config/audit.toml`)

| Key                                 | Default | |
|-------------------------------------|---------|---|
| `audit.retention_days`              | `365`   | older entries are deleted hourly (1–36,500) |
| `audit.sign_in_failures_per_minute` | `600`   | failed sign-ins recorded one by one, per instance |

See [Audit log](../digging-deeper/audit-log.md).

### `[mail]` (`config/mail.toml`, feature `mail`)

Driver (`disabled`, `smtp`, `log`, `memory`), where the SMTP server comes from
(`settings` or `config`), the hosts and ports mail may go to, timeouts and the
rate: see [Mail](../digging-deeper/mail.md#configuration).

### `[queue]` (`config/queue.toml`)

Workers and job leases: see [Queues](../digging-deeper/queues.md#configuration).
`serve` runs `queue.workers` workers (2) when `queue.enabled` (`true`).

### `[redis]` (`config/redis.toml`, feature `redis`)

| Key            | Default               |                                                  |
|----------------|-----------------------|--------------------------------------------------|
| `redis.url`    | — (`REDIS_URL`)        | standalone Redis; unset: one instance            |
| `redis.prefix` | `rustcms:<app.name>`  | shared by one deployment's instances, distinct between deployments; not empty, no spaces |

With a URL set, boot fails if Redis does not answer within 5 s, or if RustCMS
was built without the `redis` feature. Commands time out after 1 s; rate
limits then fail closed (500). See [Several instances](../digging-deeper/instances.md).

### `[realtime]` (`config/realtime.toml`)

[Realtime](../admin/realtime.md) streams and sockets. Every limit is per instance; streams and sockets share
`max_connections`, `max_per_user` and the open rates.

| Key | Default | |
|---|---|---|
| `realtime.revalidate_secs` | `30` | how often an open stream rechecks the session, user and permissions (at least 1; each check costs about 3 queries) |
| `realtime.max_connections` | `1000` | open streams on this instance (429 beyond) |
| `realtime.max_per_user` | `10` | open streams of one user on this instance |
| `realtime.opens_per_user_per_minute` | `60` | |
| `realtime.opens_per_ip_per_minute` | `300` | shared IPs (offices, proxies) share it |
| `realtime.shutdown_grace_secs` | `5` | time given to open streams, sockets and connections when the server stops ([Stopping](../architecture/lifecycle.md#stopping)) |
| `realtime.socket_revalidate_secs` | `10` | how often an open WebSocket rechecks (at least 1) |
| `realtime.max_message_bytes` | `65536` | largest WebSocket message, either way (1009 beyond; at most 1 MiB) |
| `realtime.send_timeout_secs` | `10` | longest wait of a WebSocket send for a slow client (then `lagging`; at least 1) |

Values below the minimum, or above the maximum, are raised or lowered to it.

### `[schedule]` (`config/schedule.toml`)

[Scheduling](../digging-deeper/scheduling.md). Schedules run automatically in `serve`
only; `schedule:run <name>` runs one by hand.

| Key | Default | |
|---|---|---|
| `schedule.enabled` | `true` | run the schedules on this instance |
| `schedule.grace_secs` | `30` | how late (1–300 s) a slot may still be claimed; older slots are skipped |
| `schedule.history_retention_days` | `7` | runs kept on the Schedules page (1–3,650) |

## The environment

`app.env` (`APP_ENV`) names where the application runs: `local`, `testing` or
`production` (the default, so a server that forgets it gets the safe
behaviour). The framework uses it in a few places:

| Behaviour | `local` | `testing` | `production` |
|---|---|---|---|
| `migrate:fresh`, `migrate:refresh`, `db:seed` | run | run | refuse without `--force` |
| `mail.log_body` (mail bodies, reset links, in the log) | allowed | refused at start | refused at start |
| `mail.allowed_hosts` when admins set the SMTP server (`smtp` driver) | optional | optional | required |
| Page and mail templates re-read from disk on each render | in debug builds (`cargo run`) | no | no |

Your code reads it as `state.config().app.env`, an `Environment`
(`rustcms::config::Environment::{Local, Testing, Production}`), e.g. a seeder
that refuses to create demo users in production.

`app.debug` (`APP_DEBUG`, `true` in a new `.env`) is loaded for your code as
`config.app.debug`. **The framework does not change its behaviour on it**:
error responses never contain internal details, whatever its value, and log
verbosity comes from `log.filter`. Keep it `false` in production anyway, in
case your own code uses it.

## Your own sections

Any `config/<name>.toml` becomes a section your code reads with
`Config::section`. Pick a name the framework does not use (above):

```rust
#[derive(serde::Deserialize)]
struct Billing { currency: String }

// in a provider's register: config/billing.toml or RUSTCMS_BILLING__CURRENCY
let billing: Billing = app.config().section("billing").expect("[billing] settings");
```

`section` deserializes with serde: give fields `#[serde(default)]` to make them
optional. The configuration is available as `app.config()` while the
application is assembled and as `state.config()` afterwards.

## Environment variables

Every key can be set from the environment as `RUSTCMS_<SECTION>__<KEY>`
(nested keys with more `__`): `RUSTCMS_APP__LISTEN=0.0.0.0:8000`,
`RUSTCMS_AUTH__JWT__ACCESS_TTL_SECS=600`. Lists are written in TOML syntax:
`RUSTCMS_HTTP__TRUSTED_PROXIES='["10.0.0.0/8"]'`. With every value in the
environment, the `config/` directory is optional.

### From files (Docker and Kubernetes secrets)

Any of these variables (the well-known ones and every `RUSTCMS_…`) can be read
from a file instead: add `_FILE` to its name and give the path.

```sh
export APP_KEY_FILE=/run/secrets/app_key
export DATABASE_URL_FILE=/run/secrets/database_url
export RUSTCMS_MAIL__SMTP__PASSWORD_FILE=/run/secrets/smtp_password
```

- The value takes its variable's place in the order above: a
  `RUSTCMS_DATABASE__URL` still wins over `DATABASE_URL_FILE`.
- One trailing line break is removed (as `echo` and editors add it); any other
  whitespace is kept. Values are read like environment values (`true`,
  numbers), except that a value with leading or trailing spaces stays exactly
  that string.
- Boot fails when a variable and its `_FILE` are both set, or when the file is
  missing, unreadable, empty, not UTF-8 or larger than 64 KiB. The message
  names the variable and the path, never the content.
- A key whose own name ends in `_file` cannot be set from the environment
  (`RUSTCMS_X__LOG_FILE` means the file of `x.log`); put it in a TOML file.
  A key named `file` itself is fine: `RUSTCMS_LOG__FILE` is `log.file`.

## In tests and tools

| API | Does |
|---|---|
| `Config::load()` | what the console does: `.env`, then `config/` and the environment of the working directory |
| `Config::figment(dir)` | the layered sources of `dir` and the environment, without `.env` |
| `Config::from_figment(figment)` | checks and extracts the framework sections |
| `config.with("app.env", "production")` | consumes the configuration and returns it with one value replaced, checked again (`Result<Config, ConfigError>`) |
| `config.section::<T>("name")` | an application section |

In tests, `rustcms_testing::test_config(url)` builds a configuration for a
test database; change it with `with` ([Testing](../basics/testing.md)).

## Pitfalls

- **A section named like the framework's** (`mail`, `queue`, `storage`, …) is
  the framework's: your fields and its fields must agree, and its checks apply.
- **`.env` does not override** a variable already exported in the shell or by
  systemd: the environment wins.
- **Secrets in `config/*.toml`** end up in the repository. Keep `APP_KEY`,
  database passwords and SMTP passwords in the environment or in `_FILE`
  secrets.

## Related pages

- [Deployment](deployment.md): what the binary reads on a server
- [Production checklist](../security/production.md)
- [The `APP_KEY`](../operations/app-key.md): rotating it
