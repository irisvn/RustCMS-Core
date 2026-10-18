# Scheduling

Periodic work (reconcile every minute, prune every night) is declared on the
application and run by `rustcms serve`:

```rust,ignore
// src/providers/app_service_provider.rs (register)
use rustcms::schedule::Schedule;

app.schedule(Schedule::every("reconcile", Duration::from_secs(60)).job(|| Reconcile {}).without_overlapping());
app.schedule(Schedule::cron("docker-prune", "0 3 * * *")?.job(|| Prune { until: "24h".into() }));
app.schedule(Schedule::every("warm-cache", Duration::from_secs(300)).run(|state| async move { warm(&state).await }));
```

## When it fires

- `every(name, period)`: 10 s to 7 days, whole seconds, on the multiples of
  the period since the Unix epoch. Every instance agrees on the times (a
  minute schedule fires at :00 of every minute).
- `cron(name, "m h dom mon dow")`: five fields in **UTC**, so there are no
  time zones and no daylight saving time.
  - Numbers, `*`, ranges, lists, steps and 3-letter names (`MON`, `JAN`) are
    accepted.
  - Day-of-month and day-of-week combine with OR, as in Vixie cron.
  - No seconds, years, `L`, `W`, `#` or `@daily`. An expression that never
    fires (`0 0 30 2 *`) is an error.
- A **missed slot is skipped, never caught up**: after a restart or a pause
  only the next slot runs. A slot is still taken up to
  `schedule.grace_secs` (30) late.
- Names are 1–64 of `a-z 0-9 _ . -`, unique; `rustcms.` is reserved.

## What it does

- **`.job(make)`** (recommended) enqueues a [queue job](queues.md) for each
  slot. The job's retries, timeouts and leases apply, and the Queue page
  shows it.
  - It registers the job type; `app.job::<J>()` as well is fine.
  - Workers must serve its queue (`serve`, or `queue:work`).
- **`.run(f)`** runs in the process. It is best effort: a crash during the
  run loses it.
- **Once per slot, whatever the number of instances.** Each slot is claimed in
  the database, in the same transaction as its job, so there is at most one
  job per slot (no Redis needed). Jobs are still at-least-once: make them
  idempotent.

## Overlapping runs

- A `.job` schedule enqueues a job per slot. If one can take longer than the
  period, add **`.without_overlapping()`**: a slot is skipped while the
  previous slot's job is still pending or running. The queue's lease is the
  lock, so a dead worker's job is retried, then the schedule resumes.
- A `.run` schedule never overlaps itself on one instance (slots are
  skipped). On several instances two long runs can overlap: use `.job` +
  `.without_overlapping()` instead.
- Manual runs (`schedule:run`) and `queue:retry` are not held back.

```rust,ignore
/// Converges containers to the declared state: safe to repeat.
#[derive(Serialize, Deserialize)]
pub struct Reconcile {}

#[async_trait]
impl Job for Reconcile {
    const KIND: &'static str = "reconcile";
    fn timeout() -> Duration { Duration::from_secs(50) }
    async fn handle(self, ctx: JobCtx) -> anyhow::Result<()> {
        let docker = ctx.state.service::<Docker>().unwrap();
        docker.converge().await // idempotent: a retry repeats part of it harmlessly
    }
}
```

## Where it runs

- Only `serve` runs the schedules. Console commands, `queue:work` and tests
  (`TestApp`) never do.
- `[schedule] enabled = false` turns it off on an instance (for example on
  instances that only serve requests).
- Instances' clocks must agree within about a second (NTP). A clock running
  fast can start a slot that much early; it never runs a slot twice.
- On shutdown nothing new starts, and `.run` actions get
  `[realtime] shutdown_grace_secs`.

## Commands

```sh
rustcms schedule:list            # each schedule, its next slot, its last claimed slot (and job)
rustcms schedule:run reconcile   # run it now: a job is enqueued, a run runs in the command
```

A manual run is not a slot: there is no claim, no grace and no
`without_overlapping`, and `enabled` is ignored.

The framework's own cleanups (media, exports, idempotency keys) still run as
provider loops; they will move to schedules later.
