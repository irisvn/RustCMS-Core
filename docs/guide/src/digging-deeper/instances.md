# Several instances

One instance needs nothing more. This page is for running several behind a
load balancer: what they must share, what Redis adds, and what stays per
instance.

## Quick start

1. Build with the `redis` feature and set `REDIS_URL` (`[redis] url`): one
   standalone Redis (Redis Cluster is not supported).
2. Give every instance the same `APP_KEY`, `DATABASE_URL` and `redis.prefix`;
   two applications sharing a Redis need different prefixes (default
   `rustcms:<app.name>`).
3. Share uploads: S3 (feature `s3`), or one volume mounted by every instance
   ([Storage](storage.md)).
4. Run `migrate` once per deploy, before the new instances start.
5. Keep the clocks synchronized (NTP): the queue and the scheduler compare
   times written by different instances.

```toml
# config/redis.toml (or REDIS_URL / RUSTCMS_REDIS__PREFIX); the file is the [redis] section
url = "redis://redis.example.com:6379/0"
prefix = "rustcms:blog"
```

## Redis configuration

| Key | Default | |
|---|---|---|
| `url` (`REDIS_URL`) | unset: no Redis | `redis://host:port/db`; empty counts as unset |
| `prefix` | `rustcms:<app.name>` | starts every key of this deployment; may not be empty or contain whitespace |

- Boot fails when Redis cannot be reached, when the prefix is invalid, or
  when `url` is set but the application was built without the `redis`
  feature.
- Commands time out after a second, connecting after two.

## What Redis is used for

- **Rate limits** (sign-in, `Throttle<N>`, uploads, …) are counted in Redis
  for all instances, atomically across the keys of one check. If Redis does
  not answer, checks fail closed (500).
- **The media and exports cleanups** take a lease, which avoids most
  duplicate runs. It is best effort: a lease expires (it is not renewed
  during a long run), and when Redis does not answer the cleanup runs anyway.
  Every cleanup is safe to run twice, also at the same time. Their lease
  names show in the logs:

  | Lease | Every | Deletes |
  |---|---|---|
  | `media-prune:<disk>` | minute | files of deleted media, abandoned uploads |
  | `exports-prune:<disk>` | 10 minutes | expired exports and imports |

  These cleanups start when the application boots, in every process that
  boots it: `serve`, `queue:work`, and also application commands for as long
  as they run. The auth and idempotency cleanups need no lease: they are
  [framework schedules](scheduling.md#framework-schedules)
  (`rustcms.auth-prune`, `rustcms.idempotency-prune`), each slot claimed once
  in the database, run only by `serve`. See
  [Data retention](../operations/retention.md).

Permissions need no Redis: a change made on one instance applies on every
instance from the next request (the permission cache follows a counter kept
in the database).

The queue and the scheduler need no Redis either: they coordinate through
the database (see [below](#your-own-periodic-jobs)).

## What stays per instance

- Without Redis, several instances still work, but each keeps its own rate
  limits: run one instance unless Redis is configured.
- Password-hashing and upload concurrency limits are always per instance
  (each bounds its own CPU and disk).
- [Runtime branding](../admin/customizing.md#runtime-branding) is cached per
  instance: `AdminBranding::invalidate()` clears this instance only; the
  others load it again once their `[admin] branding_ttl_secs` has passed.
  Redis does not broadcast it.
- [Realtime events](../admin/realtime.md) are per instance too. Each instance
  runs its own hub producers, and the `[realtime]` connection limits apply to
  each one. A change on one instance still reaches every open stream within a
  recheck, because rechecks read the database.

## The queue on several instances

Every instance may run [queue workers](queues.md#several-instances): a job
is claimed by one worker at a time, with a lease of `queue.lease_secs` (60)
renewed while it runs. A lease counts until `queue.clock_skew_secs` (5) after
it ends, so keep clock differences below that. Run workers in their own
processes with `rustcms queue:work` and set `queue.enabled = false` on web
instances if you want to scale them apart ([Running the queue](../operations/queue.md)).

## Your own periodic jobs

Use a [schedule](scheduling.md). The demo declares its own from the
bootstrap:

<<< @/../../../examples/demo/src/jobs/post_digest.rs#schedule [src/jobs/post_digest.rs]

Schedules run only in `serve`. Each slot is claimed once in the database
across instances, with or without Redis, and enqueues the job (or runs the
task); the job must still be safe to retry. Turn the scheduler off on
instances that should not run it with `[schedule] enabled = false`.

`rustcms::jobs::exclusive(&state, name, ttl, f)` is a separate, best-effort
helper. It runs `f` unless another instance holds a Redis lease named `name`
(held at most `ttl`), and returns whether this instance ran it. Without
Redis it always runs. If Redis does not answer, `f` runs anyway, and a lease
can expire during a long run: write work that is correct when two runs
overlap (claim rows in the database), the lease only saves work. Prefer a
schedule for anything periodic.

## Related

- [Scheduling](scheduling.md)
- [Queues](queues.md)
- [Storage](storage.md)
- [Production checklist](../security/production.md)
- [Configuration](../getting-started/configuration.md)
