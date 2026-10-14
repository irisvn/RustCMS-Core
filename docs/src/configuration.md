# Configuration

Sources, last one wins:

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
| `database.url`              | — (**required**)       | `sqlite://…`, `postgres://…` or `mysql://…` ([MySQL setup](basics/database.md#mysql)) |
| `database.max_connections`  | `10`                   |                                              |
| `database.log_statements`   | `false`                |                                              |
| `http.body_limit`           | `1048576`              | bytes; larger bodies get 413                 |
| `http.timeout_secs`         | `30`                   | slower requests get 408                      |
| `http.trusted_proxies`      | `[]`                   | CIDRs allowed to set `X-Forwarded-For`       |
| `log.filter`                | `info`                 | `RUST_LOG` overrides                         |
| `log.json`                  | `false`                | JSON lines for log shippers                  |

Boot fails with a clear message when `APP_KEY` is missing or malformed.

### `[storage]` (`config/storage.toml`)

Uploads (media library): see [Media, uploads & rich text](admin/media.md).

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
| `auth.jwt.enabled`                   | `true`             | JWT sign-in endpoints and bearer JWTs   |
| `auth.jwt.access_ttl_secs`           | `900`              | access token lifetime (60–3600)         |
| `auth.jwt.refresh_ttl_days`          | `14`               | an unused refresh token expires (1–90)  |
| `auth.jwt.refresh_family_days`       | `30`               | a sign-in ends, however often refreshed (≥ refresh_ttl_days, ≤ 365) |

### `[redis]` (`config/redis.toml`, feature `redis`)

| Key            | Default               |                                                  |
|----------------|-----------------------|--------------------------------------------------|
| `redis.url`    | — (`REDIS_URL`)        | standalone Redis; unset: one instance            |
| `redis.prefix` | `rustcms:<app.name>`  | shared by one deployment's instances, distinct between deployments |

With a URL set, boot fails if Redis does not answer, or if rustcms was built
without the `redis` feature. Commands time out after 1 s; rate limits then
fail closed (500).

## Your own sections

```rust,ignore
#[derive(serde::Deserialize)]
struct Mail { from: String }

let mail: Mail = config.section("mail")?;   // config/mail.toml or RUSTCMS_MAIL__FROM
```
