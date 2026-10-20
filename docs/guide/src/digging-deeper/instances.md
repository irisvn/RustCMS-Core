# Several instances

One instance needs nothing more. To run several behind a load balancer:

1. Build with the `redis` feature and set `REDIS_URL` (`[redis] url`): one
   standalone Redis (Redis Cluster is not supported).
2. Give every instance the same `APP_KEY`, `DATABASE_URL` and `redis.prefix`;
   two applications sharing a Redis need different prefixes (default
   `rustcms:<app.name>`).
3. Share uploads: S3 (feature `s3`), or one volume mounted by every instance.
4. Run `migrate` once per deploy, before the new instances start.

## What Redis is used for

- **Rate limits** (sign-in, `Throttle<N>`, uploads, …) are counted in Redis
  for all instances, atomically across the keys of one check. If Redis does
  not answer, checks fail closed (500).
- **Periodic jobs** (pruning sessions, media deletions, idempotency receipts)
  take a lease, which avoids most duplicate runs. It is best effort: a lease
  expires (it is not renewed during a long run), and when Redis does not
  answer the job runs anyway. Every job is safe to run twice, also at the same
  time.

Permissions need no Redis: a change made on one instance applies on every
instance from the next request (the permission cache follows a counter kept
in the database).

[Runtime branding](../admin/customizing.md#runtime-branding) is cached per
instance: `AdminBranding::invalidate()` clears this instance only; the others
load it again once their `[admin] branding_ttl_secs` has passed. Redis does not
broadcast it.

Without Redis, several instances still work, but each keeps its own rate
limits: run one instance unless Redis is configured. Password-hashing and
upload concurrency limits are always per instance (each bounds its own CPU
and disk).

[Realtime events](../admin/realtime.md) are per instance too. Each instance
runs its own hub producers, and the `[realtime]` connection limits apply to
each one. A change on one instance still reaches every open stream within a
recheck, because rechecks read the database.

## Your own periodic jobs

Use a [schedule](scheduling.md), declared in a provider's `register`:

```rust,ignore
app.schedule(Schedule::every("invoices-sync", Duration::from_secs(60)).job(|| SyncInvoices {}));
```

Schedules run only in `serve`. Each slot is claimed once in the database
across instances, with or without Redis, and enqueues the job; the job must
still be safe to retry.

`rustcms::jobs::exclusive(&state, name, ttl, f)` is a separate, best-effort
helper. It runs `f` unless another instance holds a Redis lease (without
Redis it always runs). If Redis does not answer, `f` runs anyway, and a lease
can expire during a long run: write work that is correct when two runs
overlap (claim rows in the database), the lease only saves work.
