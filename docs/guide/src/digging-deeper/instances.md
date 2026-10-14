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

Without Redis, several instances still work, but each keeps its own rate
limits: run one instance unless Redis is configured. Password-hashing and
upload concurrency limits are always per instance (each bounds its own CPU
and disk).

## Your own periodic jobs

```rust,ignore
// In a provider's `boot`: run every minute, on one instance at a time.
let state = state.clone();
tokio::spawn(async move {
    loop {
        rustcms::jobs::exclusive(&state, "invoices-sync", Duration::from_secs(120), || async {
            // the work; must be safe to run twice
        })
        .await;
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
});
```

`jobs::exclusive` runs the job unless another instance holds the lease
(without Redis it always runs). If Redis does not answer, the job runs anyway,
and a lease can expire during a long run: write jobs that are correct when
two run at once (claim rows in the database), the lease only saves work.
