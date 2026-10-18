# Configuration

Configuration is read when the application starts. Sources, last one wins:

1. built-in defaults,
2. `config/<section>.toml` — each file becomes the section named after it,
3. well-known variables: `APP_NAME`, `APP_ENV`, `APP_DEBUG`, `APP_URL`, `APP_KEY`, `DATABASE_URL`,
4. `RUSTCMS_<SECTION>__<KEY>`, e.g. `RUSTCMS_HTTP__BODY_LIMIT=2097152`.

`.env` is loaded first and never overrides variables already set in the environment.

## Framework sections

| Key                         | Default                | Notes                                        |
|-----------------------------|------------------------|----------------------------------------------|
| `app.name`                  | `rustcms`              |                                              |
| `app.env`                   | `production`           | `local`, `testing`, `production`             |
| `app.debug`                 | `false`                |                                              |
| `app.url`                   | `http://localhost:8000`|                                              |
| `app.key`                   | — (**required**)       | `base64:` + 32 bytes; `rustcms key:generate` |
| `app.listen`                | `127.0.0.1:8000`       |                                              |
| `app.locale`                | `en`                   | admin interface language by default ([Languages](../admin/i18n.md)) |
| `app.locales`               | `["en", "vi"]`         | languages users may choose (the panel ships `en`, `vi`) |
| `database.url`              | — (**required**)       | `sqlite://…`, `postgres://…` or `mysql://…` ([MySQL setup](../basics/database.md#mysql)) |
| `database.max_connections`  | `10`                   |                                              |
| `database.log_statements`   | `false`                |                                              |
| `http.body_limit`           | `1048576`              | bytes; larger bodies get 413                 |
| `http.timeout_secs`         | `30`                   | slower requests get 408                      |
| `http.trusted_proxies`      | `[]`                   | CIDRs allowed to set `X-Forwarded-For`       |
| `log.filter`                | `info`                 | `RUST_LOG` overrides                         |
| `log.json`                  | `false`                | JSON lines for log shippers                  |

Boot fails with a clear message when `APP_KEY` is missing or malformed.

### `[storage]` (`config/storage.toml`)

Uploads and the media library: see [File storage](../digging-deeper/storage.md).

### `[auth]` (`config/auth.toml`)

| Key                                  | Default            |                                         |
|--------------------------------------|--------------------|-----------------------------------------|
| `auth.admin_path`                    | `/admin`           | panel and session guard prefix, cookie path (`/seg(/seg)*`, not `/` or `/api…`) |
| `auth.admin_origins`                 | `[]` → `APP_URL`   | origins allowed to change state         |
| `auth.cookie_name`                   | `rustcms_session`  |                                         |
| `auth.secure_cookies`                | https `APP_URL`    |                                         |
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

### `[redis]` (`config/redis.toml`, feature `redis`)

| Key            | Default               |                                                  |
|----------------|-----------------------|--------------------------------------------------|
| `redis.url`    | — (`REDIS_URL`)        | standalone Redis; unset: one instance            |
| `redis.prefix` | `rustcms:<app.name>`  | shared by one deployment's instances, distinct between deployments |

With a URL set, boot fails if Redis does not answer, or if RustCMS was built
without the `redis` feature. Commands time out after 1 s; rate limits then
fail closed (500). See [Several instances](../digging-deeper/instances.md).

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
| `realtime.shutdown_grace_secs` | `5` | time given to open streams and sockets when the server stops |
| `realtime.socket_revalidate_secs` | `10` | how often an open WebSocket rechecks (at least 1) |
| `realtime.max_message_bytes` | `65536` | largest WebSocket message, either way (1009 beyond) |
| `realtime.send_timeout_secs` | `10` | longest wait of a WebSocket send for a slow client (then `lagging`) |

## Your own sections

```rust,ignore
#[derive(serde::Deserialize)]
struct Mail { from: String }

let mail: Mail = config.section("mail")?;   // config/mail.toml or RUSTCMS_MAIL__FROM
```

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
APP_KEY_FILE=/run/secrets/app_key
DATABASE_URL_FILE=/run/secrets/database_url
RUSTCMS_MAIL__SMTP__PASSWORD_FILE=/run/secrets/smtp_password
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
