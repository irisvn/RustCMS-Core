# Queues

Work that should not hold up a request (sending mail, building an export,
calling another service) goes into a job. Jobs are stored in the database
(the `jobs` table, created by `rustcms migrate`) and run by workers: inside
`serve`, or in their own process with `rustcms queue:work`. No Redis or
other service is needed.

## Quick start

```sh
rustcms make:job SendWelcome
```

This writes `src/jobs/send_welcome.rs` (a job of kind `send_welcome`) and
registers it in `src/providers/app_service_provider.rs`, at the
`// rustcms:jobs` marker:

```rust
app.job::<crate::jobs::send_welcome::SendWelcome>();
```

The generated struct is empty: add the ids the job needs
(`pub learner_id: i64`) and fill in `handle`. Then queue the job in the
transaction of the change it follows from:

```rust
let txn = rustcms::db::begin_write(state.db()).await?;
// … the change …
rustcms::queue::dispatch(&txn, &SendWelcome { learner_id }).await?;
txn.commit().await?;
```

`serve` runs it a moment after the commit.

## Writing a job

A job is a `Serialize + Deserialize` struct that implements `Job`. The LMS
example mails a learner who took a seat in a course:

<<< @/../../../examples/lms/src/jobs/send_welcome.rs#job [src/jobs/send_welcome.rs]

- The payload holds ids only. The job reads the rows again when it runs, so
  it sees current data and the `jobs` table holds no personal data or
  secrets.
- A row deleted since is not an error: there is nothing to do.
- Mail errors become a [`Failure`](#failures): permanent ones (a refused
  address) fail at once, others are retried.
- A receipt written with [`queue::once`](#at-least-once) after the send stops
  later runs once it commits. A crash between the send and that commit, or
  two runs at the same time, can still mail twice: delivery is at least
  once.

Jobs live in `src/jobs/`, one per file, and each is registered once:

<<< @/../../../examples/lms/src/providers/app_service_provider.rs#jobs [src/providers/app_service_provider.rs]

`app.job` panics at boot on an invalid kind, a kind registered by two
types, a `rustcms.` kind (reserved), an invalid `QUEUE`, `max_attempts` out
of range, `VERSION` 0, or a zero timeout. Registering the same type twice is
fine (a [schedule](scheduling.md)'s `.job` registers its job too).

### The `Job` trait

| Item | Default | |
|---|---|---|
| `KIND` | | 1 to 64 of `a-z`, `0-9`, `_`, `.` (no `-`); unique; stored with each job |
| `VERSION` | `1` | raise it when the struct changes (see [Changing a job](#changing-a-job)) |
| `QUEUE` | `"default"` | which workers take it: 1 to 64 of `a-z`, `0-9`, `_`, `.`, `-` |
| `AT_MOST_ONCE` | `false` | never run twice: a run stopped by a shutdown fails ("interrupted by shutdown") instead of being given back; use it with `max_attempts() == 1` for effects that must not repeat |
| `max_attempts()` | `5` | 1 to 100 |
| `backoff(attempt)` | 10 s, 20 s, 40 s… up to 1 hour | wait before the next attempt |
| `timeout()` | 5 minutes | a longer run is stopped and counts as failed; capped by `queue.max_timeout_secs` |
| `describe(&self)` | `None` | a short text the Queue page shows ("deploy demo/web #12"): at most 200 characters, control characters removed; `queue.view` holders see it, so never put secrets or personal data in it |
| `upgrade(version, payload)` | refuses | reads the payload of an older `VERSION` |
| `handle(self, ctx)` | | does the work; an error, a panic or the timeout retries it |

The demo's job that the [scheduler](scheduling.md) runs every hour, with a
description:

<<< @/../../../examples/demo/src/jobs/post_digest.rs#job [src/jobs/post_digest.rs]

`JobCtx`, what `handle` receives:

| Field / method | |
|---|---|
| `state` | the `AppState` (database, configuration, services) |
| `id` | the job's id (a `Uuid`); also a good idempotency key for other services |
| `attempt` | this run, 1 for the first |
| `initiated_by` | the user who asked for it, if any (their permissions are not carried) |
| `audit_context()` | the [audit](audit-log.md) actor: the job, on behalf of its initiator |

Keep the payload small (at most 64 KiB of JSON, `queue::MAX_PAYLOAD_BYTES`):
a larger one fails the dispatch.

## Dispatching

Add a job **in the transaction of the change it follows from**. It then
exists only if the change commits: an enrollment that rolls back sends no
welcome mail.

| Function | |
|---|---|
| `queue::dispatch(&txn, &job)` | in the change's transaction |
| `queue::dispatch_with(&txn, &job, at, initiated_by)` | the same, run no earlier than `at: Option<DateTime<Utc>>`, on behalf of a user id |
| `queue::enqueue(db, &job)` | on its own, outside any change |
| `ctx.dispatch(txn, &job)` | in a resource hook or an action: `dispatch_with` with the signed-in user as initiator |

Each returns the job's id (`Uuid`).

In the LMS, a new active enrollment queues its welcome mail from the
resource's `mutate_before_create` hook:

```rust
if status == enrollment::ACTIVE {
    // Queued with the enrollment: no enrollment, no job.
    ctx.dispatch(txn, &SendWelcome { course_id, learner_id }).await?;
}
```

## Running

`serve` runs `queue.workers` workers (2) beside the HTTP server. To run them
elsewhere, set `queue.enabled = false` (or `workers = 0`) on the web
instances and run `rustcms queue:work` in another process.

| `queue:work` option | |
|---|---|
| `--queue <name>` | a queue to take jobs from; repeat it for several (default: `queue.queues`) |
| `--workers <n>` | workers in this process, 1 to 64 (default: `queue.workers`, at least 1) |
| `--once` | run the jobs ready now (at most 1,000), then exit: a cron job, a test. It deletes old failed jobs, receipts and history first |

On Ctrl-C or SIGTERM, workers stop taking jobs at once (while the server
finishes its requests), let running ones finish for `queue.shutdown_secs`
(30 s), then stop them. A stopped job is given back, and its interrupted
attempt with it: it runs again, even if that was its last attempt (unless
`AT_MOST_ONCE`).

Operations (where to run workers, deploys) are on
[Running the queue](../operations/queue.md).

## Failures

A run fails when `handle` returns an error, panics or times out. The job
waits for its backoff and runs again, up to `max_attempts`; then it stays
as failed:

```sh
rustcms queue:failed          # list failed jobs
rustcms queue:retry <id>      # or --all: run again, attempts start over
rustcms queue:forget <id>     # or --all: delete
```

Failed jobs are deleted after `queue.failed_retention_days` (30).

The table keeps a safe message only. Return a `queue::Failure` to choose it:

```rust
return Err(Failure::new("smtp_rejected", "the mail server refused the message").into());
```

It is kept as `code: message` (`smtp_rejected: the mail server refused the
message`, at most 500 characters). Any other error is kept as `failed (see the
logs for job <id>)`, and its full text goes to the application log only, so a
secret in an error message never reaches the table. Do not put secrets in
errors anyway: logs are kept too. A `Failure` is logged by its code only.

When retrying cannot help (a refused address, a missing template), fail at
once instead of using the remaining runs:

```rust
return Err(Failure::new("smtp_rejected", "the mail server refused the message").permanent().into());
```

The job is then failed like one that used all its runs (`queue:retry` still
runs it again). `Failure` has public `code` and `message` fields;
`permanent()` wraps it in a `Permanent`. A mail error converts with
`Failure::from(err)` (see [Mail](mail.md#sending-from-your-own-job)).

## At least once

A job runs **at least once**: a worker that crashes after the work but
before the job is marked done leaves it to run again. Make effects
idempotent:

- database effects: `queue::once(&txn, key)` records `key` (1 to 191 bytes)
  in the effect's own transaction and returns `false` if it was recorded
  before (also when another run records it at the same time: one waits for
  the other);

  ```rust
  let txn = rustcms::db::begin_write(ctx.state.db()).await?;
  if !rustcms::queue::once(&txn, &format!("welcome:{}", self.learner_id)).await? {
      return Ok(()); // already done
  }
  // … the change …
  txn.commit().await?;
  ```

- other services: send `ctx.id` (the job id) as their idempotency key.

Receipts are deleted with failed jobs, after `queue.failed_retention_days`.

Jobs of one queue start roughly in the order they became ready, but with
several workers they may finish in any order. Give work that must not wait
behind slow jobs its own queue and its own workers (`queue:work --queue
mail`).

## Changing a job

Deploy workers that know a job before code that dispatches it. A worker that
does not know a job's kind, or sees a newer `VERSION`, gives it back for
another worker (it does not count as an attempt). The grace
`queue.unknown_kind_grace_secs` (1 hour) counts from when the job was
created, not from when a worker first met it: once it has passed, the job
fails (a delayed job older than that fails on the first such worker). To read payloads of
an older version, implement `upgrade`:

```rust
const VERSION: u32 = 2;

fn upgrade(version: u32, payload: serde_json::Value) -> anyhow::Result<serde_json::Value> {
    match version {
        1 => Ok(serde_json::json!({ "learner_id": payload["user_id"] })),
        _ => anyhow::bail!("unknown version {version}"),
    }
}
```

Without `upgrade`, a payload of an older version fails the job ("payload
version 1 cannot be read by version 2").

## The Queue page

![The Queue page: counts per queue, the jobs list, and a failed job with Retry and Forget](/img/digging-deeper/queue-page.webp)
*The demo application: System → Queue.*

The admin panel shows jobs under **System → Queue** to users with
`queue.view`, and lets those who also hold `queue.manage` act on them. Both
permissions are declared by the admin panel (group `queue`); grant them
through roles.

Statuses are the workers' own (application time, and the lease counts until
`queue.clock_skew_secs` after it ends, as when a worker reclaims a job):

| Status | Meaning | Actions |
|---|---|---|
| pending | runs as soon as a worker is free (also a job whose worker vanished, with attempts left) | cancel |
| scheduled (shown as **Delayed**) | waits for its `available_at` (a backoff, a delayed dispatch); not to be confused with [schedules](scheduling.md#the-schedules-page) | cancel |
| running | a worker owns it | none |
| stalled | its last attempt's worker vanished; the next worker pass marks it failed | cancel |
| failed | will not run again | retry, forget |

- **Retry** runs a failed job again: attempts start over, it is available now.
  It keeps its id, so `once` receipts and idempotency keys built from
  `ctx.id` still recognise work an earlier attempt finished; effects guarded
  by neither run again (and receipts older than
  `queue.failed_retention_days` are gone).
- **Forget** deletes a failed job; its entry in the history stays.
- **Cancel** deletes a job waiting to run, in one statement with the
  workers' claim condition: if a worker took it first, the cancel answers
  409 (`running`). What earlier attempts committed stays.
- Each action re-reads the user's permissions, needs both permissions, and is
  recorded in the audit log (`queue.retried`, `queue.forgotten`,
  `queue.cancelled`, subject `job`, with its kind and queue). A job in the
  wrong state answers 409: `not_failed` for retry and forget of any job that
  has not failed (running ones too), `running` or `not_waiting` for cancel; a
  job that is gone, or a malformed id, 404 (after the permission checks).
  While an action's outcome is unknown (the connection dropped), the page
  pauses further actions until it is resolved.
- The page shows kind, description, queue, status, attempts, times (a
  running job's start and how long it has run), the dispatching user (named
  for those who may list users) and the safe error message. It never shows
  payloads or worker tokens. Clicking a job's kind opens its details: its
  times, error and earlier runs.
- **History**: a second tab lists finished jobs (done, failed, cancelled)
  with when they finished and how long their last attempt ran, filtered by
  status, queue, kind and time. One row is written per terminal outcome, in
  the transaction of the transition itself (a job retried and finished again
  has several); intermediate retries and shutdown give-backs write none.
  Rows are kept `queue.history_retention_days` (7 by default; 0 keeps none).
- Counts are per queue, in one SQL query (a job changing status meanwhile is
  counted once), plus the jobs done and failed in the last 24 hours; the
  lists page through jobs newest first.
- **Auto refresh**: the page reloads every 5 seconds while the tab is visible
  and no dialog is open, unless the user switches it off (kept in that
  browser). It pauses after "Load more", until *Back to latest*.

## Permissions and the audit log

A job does not carry the permissions of the user who asked for it. When the
work depends on them, load the user and their grants again in `handle`.
Changes a job records in the audit log appear as actor `job`, on behalf of
its initiator, with credential `queue:<kind>`: record them with
`rustcms::audit::record(&txn, &ctx.audit_context(), entry)`.

## Several instances

Every instance may run workers: a job is claimed by one worker at a time
(an owner token and a lease of `queue.lease_secs`, renewed while it runs).
Keep the instances' clocks synchronized (NTP); `queue.clock_skew_secs`
(5 s) is the difference tolerated before a lease is taken over.

## Configuration

`[queue]` (`config/queue.toml`, or `RUSTCMS_QUEUE__<KEY>`):

| Key | Default | Range | |
|---|---|---|---|
| `enabled` | `true` | | workers in `serve` |
| `workers` | `2` | 0 to 64 | per process (`serve`, `queue:work`); 0: none in `serve` |
| `queues` | `["default"]` | not empty | queues the workers take jobs from; jobs of all of them are taken in one order (by when they became ready, then id), not queue by queue |
| `lease_secs` | `60` | 10 to 3,600 | how long a run owns its job, renewed by heartbeats |
| `poll_ms` | `1000` | 100 to 60,000 | wait when no job is ready |
| `shutdown_secs` | `30` | 0 to 3,600 | how long running jobs may finish on shutdown |
| `max_timeout_secs` | `3600` | 1 to 86,400 | caps every job's `timeout()` |
| `clock_skew_secs` | `5` | 0 to 60 | clock difference tolerated between instances |
| `unknown_kind_grace_secs` | `3600` | 0 to 604,800 | how long after its creation a job no worker knows may still wait before it fails |
| `failed_retention_days` | `30` | 1 to 3,650 | failed jobs and `once` receipts |
| `history_retention_days` | `7` | 0 to 3,650 | finished jobs kept in the history (0: no history) |

Boot fails on a value out of range or an invalid queue name.

## In Rust: tests and tools

| Item | |
|---|---|
| `queue::run_until_idle(&state, queues, max)` | runs ready jobs one after another until none is ready or `max` ran; returns how many ran (what `--once` does; handy in tests) |
| `queue::Workers::start(&state, &config, queues, count)` | starts workers; `.shutdown().await` stops them |
| `queue::manage::{counts, retry, forget, cancel}` | what the Queue page does, with a `queue::Clock` (`Clock::of(&config)`); `retry`/`forget`/`cancel` return `AppResult<Result<(), Refusal>>`: `Ok(Err(refusal))` for a job in the wrong state, `Err(AppError)` for a database error |
| `queue::JobStatus`, `queue::QueueCounts` | the statuses above, and the counts per queue |

`queue::QueueConfig::of(state.config())` reads the validated `[queue]`
section. The commands `queue:failed`, `queue:retry` and `queue:forget` remain
for scripts and cron.

## Pitfalls

- Code that blocks the thread (no `.await`) cannot be stopped by the
  timeout. `tokio::task::spawn_blocking` keeps the worker responsive, but the
  blocking work goes on after the timeout stops the job, and can overlap its
  retry: make long blocking work check a stop flag of its own.
- A job dispatched with `enqueue` outside the change's transaction can run
  for a change that later rolls back. Prefer `dispatch`.
- `describe` is visible to every `queue.view` holder.
- Without a worker somewhere, nothing runs: requests still answer and the jobs
  wait.

## Related

- [Running the queue](../operations/queue.md)
- [Scheduling](scheduling.md)
- [Mail](mail.md)
- [Audit log](audit-log.md)
- [Testing](../basics/testing.md)
