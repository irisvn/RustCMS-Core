# Deployment

An application builds into **one binary** holding the HTTP server, every
console command, the admin panel and your translations. The server needs no
Rust, Node or source code.

In this page the binary is called `blog`.

## Building

```sh
cargo build --release          # → target/release/blog
strip target/release/blog      # optional: about 29 MB → 23 MB
```

Optional features are enabled on the `rustcms` dependency:

| Feature | Default | Enables |
|---|---|---|
| `sqlite`, `postgres`, `mysql` | on | database drivers |
| `redis` | off | [several instances](../digging-deeper/instances.md) |
| `s3` | off | uploads on S3-compatible storage (`storage.disk = "s3"`) |
| `mail` | on | [mail](../digging-deeper/mail.md): SMTP, templates, the Mail settings page |

```toml
rustcms = { git = "https://github.com/irisvn/RustCMS-Core", tag = "v0.10.1", features = ["redis", "s3"] }
```

Keep the tag your application already uses (`rustcms upgrade` sets it) when
adding features.

Build for the server's platform: on a Linux CI runner, or in Docker (below).

## What the binary reads

Everything is relative to the working directory:

| What | Where | Notes |
|---|---|---|
| Configuration | `config/*.toml` | optional: missing files use the defaults |
| Environment | `.env`, or the process environment | the environment always wins over `.env` |
| Uploads (`local` disk) | `storage/app` (`storage.root`) | keep it across deploys, and back it up |
| SQLite database | `DATABASE_URL` | use an absolute path |

A typical layout:

```text
/opt/blog/
├── blog              # the binary
├── config/           # copied from the repository
├── .env              # secrets (chmod 600), or an EnvironmentFile of systemd
└── storage/app/      # uploads (writable by the app's user)
```

Main variables:

| Variable | Required | Example |
|---|---|---|
| `APP_KEY` | yes | `base64:…` — generate once with `./blog key:generate --show`, store as a secret |
| `DATABASE_URL` | yes | `postgres://user:pass@db:5432/blog`, `mysql://…`, `sqlite:///opt/blog/data/app.sqlite?mode=rwc` |
| `APP_URL` | yes | `https://blog.example.com` — the origin allowed to sign in and write in the admin |
| `APP_ENV` | recommended | `production` (also the default) |
| `APP_DEBUG` | no | `false` in production |
| `REDIS_URL` | several instances | `redis://redis:6379/0` |
| `RUST_LOG` | no | `info` (overrides `log.filter`) |
| `RUSTCMS_<SECTION>__<KEY>` | no | `RUSTCMS_APP__LISTEN=0.0.0.0:8000` |

Rotating `APP_KEY` signs every admin out and ends every JWT sign-in (API
tokens stay valid). Never commit it.

## First run

```sh
cd /opt/blog
./blog key:generate --show        # put the value in .env or your secret store: APP_KEY=base64:…
./blog migrate
./blog permission:sync
./blog make:user --email you@example.com --name You --super-admin   # prints a generated password
./blog serve                      # app.listen, 127.0.0.1:8000 by default
```

Do not pass `--password` to `make:user` on a server: it would stay in the
shell history.

Check `curl http://127.0.0.1:8000/health` → `{"status":"ok","database":"ok"}`
(use it as the load balancer's health check), then sign in at `APP_URL/admin`.

## Upgrading

1. Build the new binary and copy it next to the old one (`blog.new`).
2. Back up the database (and `storage/app` on the local disk).
3. `./blog.new migrate`, then `./blog.new permission:sync`.
4. Replace the binary and restart the service.
5. Check `/health` and sign in.

To go back, roll back with the **new** binary first (only it knows its
migrations): `./blog.new migrate:rollback --step N`, then start the old one.
`migrate:rollback` undoes migrations in the migrator's order, last first, not
in the order they were applied: the application's migrations come after the
framework's, so `--step N` undoes the application's newest ones first. When
an upgrade only added framework migrations (a new rustcms version, no new
migrations of yours), do not roll back: keep them and start the old binary
with `migrate --allow-newer` (below). Migrations that drop data cannot
restore it: use the backup.

**Automatic rollbacks** (a Docker Swarm or Kubernetes rollout that returns to
the previous image, whose start runs `migrate`): the older binary finds
migrations it does not know and `migrate` refuses to run. If your migrations
are **additive only** (new tables, new nullable columns or columns with
defaults; never a rename, a drop or a stricter constraint), let it accept
them: `migrate --allow-newer`, or `[database] allow_newer_migrations = true`
(`RUSTCMS_DATABASE__ALLOW_NEWER_MIGRATIONS=true`). It logs the unknown ones,
leaves them as they are and applies its own pending migrations, so the older
version starts on the newer schema. Keeping migrations additive is the
application's rule; the framework's own migrations follow it.
Two things it does not do:
- **Protect a rollback to an image older than 0.8.1**, or one started
  without the flag: that image refuses as before. Roll out an image on 0.8.1
  or later with `--allow-newer` first, and only then add migrations, so the
  image a rollback returns to accepts them.
- **Serialize replicas.** Two replicas running `migrate` at the same start
  may race on the same pending migration, with or without the flag. Run
  migrations once per deployment (a one-off task or job before the rollout,
  or a single replica), not in every replica's entry point.

`migrate:status` lists the unknown ones as `Newer`, and
`rustcms::db::unknown_migrations::<Migrator>(state.db())` returns them (for
a warning in your panel). `migrate:rollback` still needs the binary that knows
a migration.

`serve` stops gracefully on Ctrl-C or SIGTERM: requests in progress finish.

## systemd

```ini
[Unit]
Description=blog (RustCMS)
After=network-online.target

[Service]
User=blog
WorkingDirectory=/opt/blog
EnvironmentFile=/opt/blog/.env
ExecStart=/opt/blog/blog serve
Restart=on-failure
NoNewPrivileges=true
ProtectSystem=strict
ReadWritePaths=/opt/blog/storage /opt/blog/data

[Install]
WantedBy=multi-user.target
```

## Docker

```dockerfile
FROM rust:1.94-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release && strip target/release/blog

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* && useradd --system --home /app blog
WORKDIR /app
COPY --from=build /src/target/release/blog /app/blog
COPY config /app/config
RUN mkdir -p /app/storage/app && chown -R blog /app/storage
USER blog
# SQLite (if used) lives in the volume too; production usually uses PostgreSQL or MySQL.
ENV APP_ENV=production RUSTCMS_APP__LISTEN=0.0.0.0:8000 \
    DATABASE_URL=sqlite:///app/storage/app.sqlite?mode=rwc
EXPOSE 8000
CMD ["/app/blog", "serve"]
```

```sh
docker build -t blog .
docker run --rm --env-file .env -v blog-storage:/app/storage blog /app/blog migrate
docker run -d -p 8000:8000 --env-file .env -v blog-storage:/app/storage blog
```

In `.env` for Docker, leave `DATABASE_URL` out (to use the image's SQLite in
the volume) or point it to your database server.

## Behind a reverse proxy

The application speaks HTTP; terminate TLS at the proxy:

```text
blog.example.com {
    reverse_proxy 127.0.0.1:8000
}
```

- `APP_URL` is the public https URL (cookies get `Secure`).
- List the proxy in `http.trusted_proxies` (`["127.0.0.1/32"]`), so rate
  limits see client addresses. Without it, every client shares the proxy's
  address and its limits.
- Keep `http.body_limit` and the upload limit at or below the proxy's.

Before going live, work through the [production checklist](../security/production.md).
