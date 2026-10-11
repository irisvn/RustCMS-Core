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
| `database.url`              | — (**required**)       | `sqlite://…` or `postgres://…`               |
| `database.max_connections`  | `10`                   |                                              |
| `database.log_statements`   | `false`                |                                              |
| `http.body_limit`           | `1048576`              | bytes; larger bodies get 413                 |
| `http.timeout_secs`         | `30`                   | slower requests get 408                      |
| `http.trusted_proxies`      | `[]`                   | CIDRs allowed to set `X-Forwarded-For`       |
| `log.filter`                | `info`                 | `RUST_LOG` overrides                         |
| `log.json`                  | `false`                | JSON lines for log shippers                  |

Boot fails with a clear message when `APP_KEY` is missing or malformed.

## Your own sections

```rust,ignore
#[derive(serde::Deserialize)]
struct Mail { from: String }

let mail: Mail = config.section("mail")?;   // config/mail.toml or RUSTCMS_MAIL__FROM
```
