# Scheduling

Periodic work (reconcile every minute, prune every night, a daily digest) is
declared on the application with `app.schedule` and run by `rustcms serve`.
Each slot runs once for the whole deployment, however many instances you
run, without Redis or an external cron.

## Quick start

Declare schedules while the application is assembled: in a provider's
`register`, or in a function the bootstrap calls. `register` returns `()`,
so a `cron` expression is unwrapped with `.expect(..)`, not `?`: a typo then
stops the boot with a clear message.

```rust
use std::time::Duration;
use rustcms::schedule::Schedule;

// Every hour, as a queue job:
app.schedule(Schedule::every("posts-ping", Duration::from_secs(3600)).job(|| PingPosts));
// Every day at 03:00 UTC, in the process:
app.schedule(
    Schedule::cron("cleanup", "0 3 * * *")
        .expect("a valid cron expression")
        .run(|state| async move { cleanup(&state).await }),
);
```

## Full example

The demo keeps its schedules next to their job, in `src/jobs/post_digest.rs`:

<<< @/../../../examples/demo/src/jobs/post_digest.rs#schedule [src/jobs/post_digest.rs]

The bootstrap calls it after the providers:

<<< @/../../../examples/demo/src/bootstrap/mod.rs#build [src/bootstrap/mod.rs]

- `posts-ping` enqueues the `PingPosts` [job](queues.md) every hour, and
  skips a slot while the previous one's job is still pending or running.
- `posts-digest` runs a task every day at 06:00 UTC, stopped after a minute,
  and its summary ("12 posts") shows on the [Schedules page](#the-schedules-page).

The job it enqueues:

<<< @/../../../examples/demo/src/jobs/post_digest.rs#job [src/jobs/post_digest.rs]

::: details In a real application: RDockPanel's backups
[RDockPanel](https://github.com/irisvn/RDockPanel), a hosting panel built on RustCMS, checks every
minute which backups are due and queues them; the summary shows on the Schedules page
([`src/providers/app_service_provider.rs`](https://github.com/irisvn/RDockPanel/blob/v0.9.3/src/providers/app_service_provider.rs#L77-L89),
v0.9.3):

```rust
app.schedule(
    rustcms::schedule::Schedule::every("database-backups", std::time::Duration::from_secs(60))
        .task(|ctx| async move {
            let n = crate::services::backup_service::tick(&ctx.state).await?;
            Ok((n > 0).then(|| format!("{n} backups queued")))
        }),
);
```
:::

## When it fires

- `Schedule::every(name, period)`: 10 s to 7 days, whole seconds, on the
  multiples of the period since the Unix epoch. Every instance agrees on the
  times (a minute schedule fires at :00 of every minute).
- `Schedule::cron(name, "m h dom mon dow")`: five fields in **UTC**, so there
  are no time zones and no daylight saving time.
  - Numbers, `*`, ranges, lists, steps and 3-letter names (`MON`, `JAN`) are
    accepted.
  - Day-of-month and day-of-week combine with OR, as in Vixie cron.
  - No seconds, years, `L`, `W`, `#` or `@daily`.
  - It returns `Result<Schedule, ScheduleError>`: `ScheduleError::Cron` for
    an expression it cannot read, `ScheduleError::Never` for one that never
    fires in the next five years (`0 0 30 2 *`).
- A **missed slot is skipped, never caught up**: after a restart or a pause
  only the next slot runs. A slot is still taken up to
  `schedule.grace_secs` (30) late.
- Names are 1–64 of `a-z 0-9 _ . -`, unique; `rustcms.` is reserved for the
  [framework's schedules](#framework-schedules).

## What it does

Give each schedule exactly one action.

- **`.job(make)`** (recommended) enqueues a [queue job](queues.md) for each
  slot. The job's retries, timeouts and leases apply, and the Queue page
  shows it.
  - It registers the job type; `app.job::<J>()` as well is fine.
  - Workers must serve its queue (`serve`, or `queue:work`).
- **`.run(f)`** runs `f(state)` in the process. It is best effort: a crash
  during the run loses it. Its error is recorded as "failed (see the logs)".
- **`.task(f)`** is `.run` with a context and a result the
  [Schedules page](#the-schedules-page) shows (the demo's `posts-digest`
  above).
  - `Ok(Some(summary))` is kept (at most 200 characters); `Ok(None)` keeps
    none.
  - An error whose chain holds a [`queue::Failure`](queues.md#failures)
    keeps its message; any other error is kept as "failed (see the logs)".
  - `schedules.view` holders see both: never put secrets in them.
  - Moving from `.run`: `|state|` becomes `|ctx|` (`ctx.state`), `Ok(())`
    becomes `Ok(None)`.
- **`.timeout(d)`** bounds a `.run` or `.task` (default 1 hour, at most a
  day): a longer run is stopped and recorded as failed. A `.job` schedule
  uses the job's own `timeout()`.
- **Once per slot, whatever the number of instances.** Each slot is claimed in
  the database, in the same transaction as its job, so there is at most one
  job per slot (no Redis needed). Jobs are still at-least-once: make them
  idempotent.

`TaskCtx`, what a `.task` receives:

| Field | |
|---|---|
| `state` | the `AppState` |
| `run_id` | this run (`schedule_runs.id`): stable, a key for your own idempotency |
| `trigger` | `Trigger::Schedule` (its slot came) or `Trigger::Manual` (Run now, `schedule:run`) |
| `initiated_by` | the user who ran it, for a manual run by a user |

## Overlapping runs

- A `.job` schedule enqueues a job per slot. If one can take longer than the
  period, add **`.without_overlapping()`**: a slot is skipped while the
  previous slot's job is still pending or running. The queue's lease is the
  lock, so a dead worker's job is retried, then the schedule resumes.
- A `.run` or `.task` schedule never overlaps itself on one instance (slots
  are skipped). On several instances two long runs can overlap: use `.job` +
  `.without_overlapping()` instead.
- Manual runs from the console (`schedule:run`) and `queue:retry` are not
  held back. The panel's Run now is (see below).

## Where it runs

- Only `serve` runs the schedules. Console commands, `queue:work` and tests
  (`TestApp`) never do.
- `[schedule] enabled = false` turns it off on an instance (for example on
  instances that only serve requests).
- Instances' clocks must agree within about a second (NTP). A clock running
  fast can start a slot that much early; it never runs a slot twice.
- On shutdown nothing new starts, and `.run` and `.task` actions get
  `[realtime] shutdown_grace_secs`, then are stopped.

## Configuration

`[schedule]` (`config/schedule.toml`, or `RUSTCMS_SCHEDULE__<KEY>`):

| Key | Default | Range | |
|---|---|---|---|
| `enabled` | `true` | | run the schedules in `serve` on this instance |
| `grace_secs` | `30` | 1 to 300 | how late a slot may still be claimed |
| `history_retention_days` | `7` | 1 to 3,650 | how long runs are kept in `schedule_runs` |

Boot fails on a value out of range.

## Commands

```sh
rustcms schedule:list              # each schedule, its next slot, its last claimed slot (and job)
rustcms schedule:run posts-digest  # run it now: a job is enqueued, a task runs in the command
```

A manual run is not a slot: there is no claim, no grace and no
`without_overlapping`, and `enabled` is ignored. It is recorded as a manual
run.

## The Schedules page

![The Schedules page: each schedule's rule, action, next run and last run](/img/digging-deeper/schedules-page.webp)
*The demo application: System → Schedules, with `posts-ping` and `posts-digest`.*

![RDockPanel's Schedules page: backups, deployments, certificate and health checks](/img/digging-deeper/schedules-rdockpanel.webp)
*RDockPanel, a hosting panel built on RustCMS: its periodic work (backups, certificate expiry, health alerts) as schedules.*

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
  `rustcms.schedule.run`, which runs the closure once in a worker, within
  the schedule's timeout and at most `queue.max_timeout_secs`: never
  retried, and a run stopped by a shutdown or a crash is recorded as failed,
  not run again. It is refused (409) while another run of the schedule is
  unfinished: a Run now whose job is still waiting or running, or a
  scheduled run younger than its timeout plus the grace. While a Run now's
  job is unfinished, the schedule's own slots are skipped (`skipped`). Two
  scheduled runs of adjacent slots may still overlap each other, as before.

The next scheduled run is unchanged. Both permissions are declared by the
admin panel (group `schedules`); grant them through roles.

## Reference

| Item | |
|---|---|
| `Schedule::every(name, period)` | a period schedule; panics on a period out of range or with a fraction of a second |
| `Schedule::cron(name, expr)` | `Result<Schedule, ScheduleError>` |
| `.job(make)`, `.run(f)`, `.task(f)` | the action |
| `.timeout(d)` | for `.run`/`.task`; panics on 0 or more than a day |
| `.without_overlapping()` | for `.job` |
| `.name()`, `.describe()`, `.next_after(t)`, `.action()`, `.run_timeout()` | read a schedule (`action()` is `ScheduleAction::Job(kind)` or `Closure`) |
| `schedule::MIN_PERIOD`, `MAX_PERIOD`, `DEFAULT_TIMEOUT` | 10 s, 7 days, 1 hour |

`app.schedule` panics at boot on an invalid, reserved or repeated name, a
schedule without an action, or `without_overlapping` on a `.run` or `.task`
schedule.

## Pitfalls

- `Schedule::cron(..)?` does not compile in `register` (it returns `()`): use
  `.expect("a valid cron expression")`.
- Cron times are UTC: `0 6 * * *` is 06:00 UTC, not local time.
- Run now needs a queue worker even for a `.task` schedule.
- Media, exports and imports cleanups are not schedules yet: they are
  background loops started when the application boots, and do not appear on
  this page. See [Data retention](../operations/retention.md).

## Framework schedules

The framework registers its own schedules, named `rustcms.…` (the prefix is
reserved: an application's `rustcms.x` panics at boot):

| Schedule | Every | Does | Summary |
|---|---|---|---|
| `rustcms.auth-prune` | hour (timeout 30 min) | deletes expired sessions, ended JWT sign-ins, expired reset and invitation links, audit entries past `audit.retention_days` | `3 sessions, 0 JWT sign-ins, 1 links, 0 audit entries` |
| `rustcms.idempotency-prune` | hour (timeout 30 min) | deletes expired idempotency receipts of admin writes | `12 expired keys` |

They behave like yours: only `serve` runs them, once per slot across
instances; they are listed on System → Schedules and by `schedule:list`, and
`schedules.manage` can Run now (audited). Each part of the auth cleanup runs
even when another fails; the run then fails with
`prune_failed: some cleanups failed (see the logs)`. Without `serve` (or with
`[schedule] enabled = false` everywhere), run them from cron with
`schedule:run rustcms.auth-prune` or the prune commands
([Data retention](../operations/retention.md#from-cron)).

## Related

- [Queues](queues.md)
- [Several instances](instances.md)
- [Data retention](../operations/retention.md)
