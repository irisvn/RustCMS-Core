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
  run loses it. Its error is recorded as "failed (see the logs)".
- **`.task(f)`** is `.run` with a context and a result the
  [Schedules page](#the-schedules-page) shows:

  ```rust,ignore
  app.schedule(Schedule::cron("database-backups", "0 3 * * *")?.task(|ctx| async move {
      let n = backup_all(&ctx.state).await?;
      Ok(Some(format!("{n} databases backed up")))
  }).timeout(Duration::from_secs(1800)));
  ```

  `Ok(Some(summary))` is kept (at most 200 characters). An error whose chain
  holds a [`queue::Failure`](queues.md#failures) keeps its message; any other
  error is kept as "failed (see the logs)". `schedules.view` holders see
  both: never put secrets in them. `ctx` has `state`, `run_id` (stable, a key
  for your own idempotency), `trigger` (`Schedule` or `Manual`) and
  `initiated_by`. Moving from `.run`: `|state|` becomes `|ctx|`
  (`ctx.state`), `Ok(())` becomes `Ok(None)`.
- **`.timeout(d)`** bounds a `.run` or `.task` (default 1 hour, at most a
  day): a longer run is stopped and recorded as failed.
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
- Manual runs from the console (`schedule:run`) and `queue:retry` are not
  held back. The panel's Run now is (see below).

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
`without_overlapping`, and `enabled` is ignored. It is recorded as a manual
run.

## The Schedules page

**System → Schedules** lists every schedule to users with `schedules.view`:
its rule, what it does (a job kind or a task), its next run, and its last
run (status, when, how long, a task's summary or error). Clicking a name
opens its recent runs. It refreshes like the [Queue page](queues.md#the-queue-page).

Each run is recorded in `schedule_runs` (kept
`[schedule] history_retention_days`, 7 by default; pruned by the scheduler and
by queue workers):

| Status | Meaning |
|---|---|
| dispatched | a job schedule's job (or Run now's) is waiting or running |
| running | a closure is running in `serve` |
| succeeded, failed | it ended; a job schedule's outcome is written by its job's own transition |
| skipped | `without_overlapping` held the slot back |
| cancelled | its job was cancelled on the Queue page |
| unknown | its end was not recorded (a shutdown, a crash: a `running` run older than its timeout plus an hour shows so) |

**Run now** (users who also hold `schedules.manage`) runs a schedule once, as
a manual run, after a confirmation; it is recorded in the audit log
(`schedule.run`). It goes through the queue, so a worker must run somewhere:

- a job schedule enqueues its job on behalf of the user; with
  `without_overlapping` it is refused (409) while the previous job is
  pending or running;
- a `.run` or `.task` schedule enqueues the framework's job
  `rustcms.schedule.run`, which runs the closure once in a worker: never
  retried, and a run stopped by a shutdown or a crash is recorded as failed,
  not run again. It is refused (409) while another run of the schedule is
  unfinished: a Run now whose job is still waiting or running, or a
  scheduled run younger than its timeout plus the grace. While a Run now's
  job is unfinished, the schedule's own slots are skipped (`skipped`). Two
  scheduled runs of adjacent slots may still overlap each other, as before.

The next scheduled run is unchanged. Both permissions are declared by the
admin panel (group `schedules`).

The framework's own cleanups (media, exports, idempotency keys) still run as
provider loops; they will move to schedules later.
