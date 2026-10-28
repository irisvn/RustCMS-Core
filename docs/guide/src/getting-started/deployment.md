# Deployment

An application builds into **one binary** holding the HTTP server, every
console command, the admin panel, queue workers, the scheduler and your
translations. The server needs no Rust, Node or source code: copy the binary
and `config/`, set the environment, run `migrate`, then `serve`.

In this page the binary is called `blog`.

## Quick start

```sh
cargo build --release                      # → target/release/blog
# on the server, in /opt/blog with config/ and the binary:
./blog key:generate --show                 # once: store it as APP_KEY
./blog migrate
./blog permission:sync
./blog serve                               # app.listen, 127.0.0.1:8000 by default
```

In another terminal:

```sh
curl http://127.0.0.1:8000/health          # {"status":"ok","database":"ok"}
```

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
rustcms = { git = "https://github.com/irisvn/RustCMS-Core", tag = "v0.10.3", features = ["redis", "s3"] }
```

Keep the tag your application already uses (`rustcms upgrade` sets it) when
adding features.

Build for the server's platform: on a Linux CI runner, or in Docker (below).

## What the binary reads

Everything is relative to the working directory (`.env` is also looked for
in its parents):

| What | Where | Notes |
|---|---|---|
| Configuration | `config/*.toml` | optional: missing files use the defaults |
| Environment | `.env`, or the process environment | the environment always wins over `.env` |
| Uploads (`local` disk) | `storage/app` (`storage.root`) | keep it across deploys, and back it up |
| SQLite database | `DATABASE_URL` | use an absolute path |

The page templates, scripts, styles and translations of the panel are compiled
into the binary: `resources/` and `lang/` are not needed on the server.

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
| `DATABASE_URL` | yes | `postgres://user:pass@db.example.com:5432/blog`, `mysql://…`, `sqlite:///opt/blog/data/app.sqlite?mode=rwc` |
| `APP_URL` | yes | `https://blog.example.com` — the origin allowed to sign in and write in the admin |
| `APP_ENV` | recommended | `production` (also the default) |
| `REDIS_URL` | several instances | `redis://redis.example.com:6379/0` |
| `RUST_LOG` | no | `info` (overrides `log.filter`) |
| `RUSTCMS_<SECTION>__<KEY>` | no | `RUSTCMS_APP__LISTEN=0.0.0.0:8000` |

Every one of them can come from a file instead (`APP_KEY_FILE=/run/secrets/app_key`):
see [secrets from files](configuration.md#from-files-docker-and-kubernetes-secrets).
`APP_DEBUG` changes nothing in the framework
([The environment](configuration.md#the-environment)); keep it `false`.

::: warning `APP_KEY`
Never commit it, and keep it stable. Replacing it signs every admin out, ends
every JWT sign-in (API tokens stay valid) and needs the old key in
`APP_PREVIOUS_KEYS` until your encrypted values are moved to the new one: see
[Rotating `APP_KEY`](../operations/app-key.md).
:::

With MySQL, `migrate` and `serve` refuse a database whose character set,
collation, engine, `sql_mode` or connection time zone would make RustCMS
behave unlike on PostgreSQL; the error lists the SQL that fixes it
([MySQL](../basics/database.md#mysql)).

## First run

```sh
cd /opt/blog
./blog key:generate --show        # put the value in .env or your secret store: APP_KEY=base64:…
./blog migrate
./blog permission:sync
./blog make:user --email you@example.com --name You --super-admin   # prints a generated password
./blog serve                      # app.listen, 127.0.0.1:8000 by default; --listen 0.0.0.0:8000 overrides
```

Do not pass `--password` to `make:user` on a server: it would stay in the
shell history.

Check `curl http://127.0.0.1:8000/health` → `{"status":"ok","database":"ok"}`,
then sign in at `APP_URL/admin`. When the database does not answer, `/health`
returns 503 with `{"status":"degraded","database":"unavailable"}`: use it as
the load balancer's health check.

In production (`APP_ENV=production`, the default), commands that drop or
overwrite data refuse to run without `--force`: `migrate:fresh`,
`migrate:refresh` and `db:seed`.

## What `serve` runs

One `serve` process is the whole application:

- the HTTP server on `app.listen`;
- **queue workers**: `queue.workers` of them (2) when `queue.enabled`
  (`true`); set `queue.enabled = false` and run `./blog queue:work` as a
  separate service to scale them apart ([Queues](../digging-deeper/queues.md));
- **the scheduler**, when `schedule.enabled` (`true`) and the application has
  schedules; with several instances, each slot runs once
  ([Scheduling](../digging-deeper/scheduling.md)).

Other commands (`migrate`, `db:seed`, your own commands) do not start
workers or the scheduler automatically. Two commands do that work on
purpose: `queue:work` runs workers, and `schedule:run <name>` runs one
schedule by hand (a task runs in the command, a job is queued). See [Request lifecycle](../architecture/lifecycle.md) for what
each command loads.

### Stopping

`serve` stops on Ctrl-C or SIGTERM:

1. it stops accepting connections, and the workers stop claiming jobs;
2. open event streams and WebSockets are told to close and get
   `realtime.shutdown_grace_secs` (5 s), then are aborted; the server waits
   the same grace for requests in progress, then stops waiting (a request
   still running ends when it finishes or when the process exits);
3. running jobs get up to `queue.shutdown_secs` (30 s) to finish; a job still
   running then is stopped and given back to the queue to run again, except
   a job with `AT_MOST_ONCE`, which is marked failed instead. Scheduled runs
   get `realtime.shutdown_grace_secs`.

Stopping a job and recording it can take up to 20 s more. Give the service
manager a stop timeout above both `queue.shutdown_secs` + 20 s and
`realtime.shutdown_grace_secs` + 1 s, with a margin: 60 s with the defaults
(`TimeoutStopSec=`, `stop_grace_period:`, `terminationGracePeriodSeconds`).

## Upgrading

1. Build the new binary and copy it next to the old one (`blog.new`).
2. Back up the database (and `storage/app` on the local disk).
3. `./blog.new migrate`, then `./blog.new permission:sync`.
4. Replace the binary and restart the service.
5. Check `/health` and sign in.

Read the version's section of the [upgrade guide](../upgrade.md) first.

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
TimeoutStopSec=60
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

Keep the build context small and free of secrets with a `.dockerignore`:

```text
target
.env
*.sqlite
*.sqlite-*
storage
```

```sh
docker build -t blog .
docker run --rm --env-file .env -v blog-storage:/app/storage blog /app/blog migrate
docker run -d -p 8000:8000 --env-file .env -v blog-storage:/app/storage blog
```

In `.env` for Docker, leave `DATABASE_URL` out (to use the image's SQLite in
the volume) or point it to your database server. `docker stop` waits 10 s by
default: pass `--stop-timeout 60` (or `stop_grace_period: 60s` in Compose) to
let jobs finish.

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
  address and its limits ([Client IP](../architecture/lifecycle.md#client-ip-and-proxies)).
- Keep `http.body_limit` and the upload limit at or below the proxy's.

## Pitfalls

- **Signed out after every deploy**: `APP_KEY` changed. Generate it once and
  keep it in your secret store.
- **Sign-in answers 403**: `APP_URL` is not the address in the browser (or
  `auth.admin_origins` does not list it).
- **Every client is rate-limited together**: the proxy is not in
  `http.trusted_proxies`.
- **Heavy jobs slow down requests**: every `serve` runs workers. Set
  `queue.enabled = false` on the web instances and run `queue:work` as its own
  service.
- **Several instances** need Redis for shared rate limits, and one `migrate`
  per deployment: see [Several instances](../digging-deeper/instances.md).

Before going live, work through the [production checklist](../security/production.md).

## Related pages

- [Configuration](configuration.md)
- [Production checklist](../security/production.md)
- [Backup and restore](../operations/backup-restore.md)
- [Running the queue](../operations/queue.md)
