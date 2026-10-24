# Queues

Work that should not hold up a request (sending mail, building an export,
calling another service) goes into a job. Jobs are stored in the database
(the `jobs` table, created by `rustcms migrate`) and run by workers: inside
`serve`, or in their own process with `rustcms queue:work`. No Redis or
other service is needed.

## Writing a job

```rust,ignore
use rustcms::async_trait;
use rustcms::queue::{Job, JobCtx};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct SendWelcome {
    pub learner_id: i64,
}

#[async_trait]
impl Job for SendWelcome {
    const KIND: &'static str = "lms.send_welcome";

    async fn handle(self, ctx: JobCtx) -> anyhow::Result<()> {
        let learner = load_learner(ctx.state.db(), self.learner_id).await?;
        send_mail(&learner, ctx.id).await
    }
}
```

Jobs live in `src/jobs/`, one per file. Register each in
`src/providers/app_service_provider.rs` (`rustcms make:job SendWelcome` writes
the file and this line):

```rust,ignore
app.job::<SendWelcome>();
```

| Item | Default | |
|---|---|---|
| `KIND` | | `[a-z0-9_.]{1,64}`, unique; it is stored with each job |
| `VERSION` | `1` | raise it when the struct changes (see [versions](#changing-a-job)) |
| `QUEUE` | `"default"` | which workers take it |
| `max_attempts()` | `5` | 1 to 100 |
| `backoff(attempt)` | 10 s, 20 s, 40 s… up to 1 hour | wait before the next attempt |
| `timeout()` | 5 minutes | a longer run is stopped and counts as failed |
| `describe(&self)` | `None` | a short text the Queue page shows ("deploy demo/web #12"): at most 200 characters, control characters removed; `queue.view` holders see it, so never put secrets or personal data in it |
| `AT_MOST_ONCE` | `false` | never run twice: a run stopped by a shutdown fails ("interrupted by shutdown") instead of being given back; use it with `max_attempts() == 1` for effects that must not repeat |

Keep the payload small (at most 64 KiB) and free of secrets: store ids and
load the data when the job runs.

## Dispatching

Add a job **in the transaction of the change it follows from**. It then
exists only if the change commits: an enrolment that rolls back sends no
welcome mail.

```rust,ignore
let txn = rustcms::db::begin_write(state.db()).await?;
// … the change …
rustcms::queue::dispatch(&txn, &SendWelcome { learner_id }).await?;
txn.commit().await?;
```

In a resource hook or an action, `ctx.dispatch(txn, &job)` does the same and
records the user who asked for it (see [Audit log](audit-log.md)):

```rust,ignore
async fn after_save(ctx: &Ctx, txn: &DatabaseTransaction, enrolment: &enrolment::Model) -> AppResult<()> {
    ctx.dispatch(txn, &SendWelcome { learner_id: enrolment.learner_id }).await?;
    Ok(())
}
```

`queue::dispatch_with(&txn, &job, Some(when), initiated_by)` delays a job;
`queue::enqueue(db, &job)` adds one outside any change.

## Running

`serve` runs `queue.workers` workers (2) beside the HTTP server. To run them
elsewhere, set `queue.enabled = false` (or `workers = 0`) on the web
instances and run `rustcms queue:work` in another process; `queue:work
--once` runs the jobs ready now and exits (a cron job, a test), deleting old
failed jobs and receipts first.

On Ctrl-C or SIGTERM, workers stop taking jobs at once (while the server
finishes its requests), let running ones finish for `queue.shutdown_secs`
(30 s), then stop them. A stopped job is given back, and its interrupted
attempt with it: it runs again, even if that was its last attempt.

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

```rust,ignore
return Err(Failure::new("smtp_rejected", "the mail server refused the message").into());
```

It is kept as `code: message` (`smtp_rejected: the mail server refused the
message`, at most 500 characters). Any other error is kept as "failed (see the
logs)", and its full
text goes to the application log only, so a secret in an error message never
reaches the table. Do not put secrets in errors anyway: logs are kept too.
A `Failure` is logged by its code only.

When retrying cannot help (a refused address, a missing template), fail at
once instead of using the remaining runs:

```rust,ignore
return Err(Failure::new("smtp_rejected", "the mail server refused the message").permanent().into());
```

The job is then failed like one that used all its runs (`queue:retry` still
runs it again).

## At least once

A job runs **at least once**: a worker that crashes after the work but
before the job is marked done leaves it to run again. Make effects
idempotent:

- database effects: `queue::once(&txn, key)` records `key` in the effect's
  own transaction and returns `false` if it was recorded before (also when
  another run records it at the same time: one waits for the other);

  ```rust,ignore
  let txn = rustcms::db::begin_write(ctx.state.db()).await?;
  if !rustcms::queue::once(&txn, &format!("welcome:{}", self.learner_id)).await? {
      return Ok(()); // already done
  }
  // … the change …
  txn.commit().await?;
  ```

- other services: send `ctx.id` (the job id) as their idempotency key.

Jobs of one queue start roughly in the order they became ready, but with
several workers they may finish in any order. Give work that must not wait
behind slow jobs its own queue and its own workers (`queue:work --queue
mail`).

## Changing a job

Deploy workers that know a job before code that dispatches it. A worker that
does not know a job's kind, or sees a newer `VERSION`, gives it back for
another worker (it does not count as an attempt); after
`queue.unknown_kind_grace_secs` (1 hour) the job fails. To read payloads of
an older version, implement `upgrade`:

```rust,ignore
const VERSION: u32 = 2;

fn upgrade(version: u32, payload: serde_json::Value) -> anyhow::Result<serde_json::Value> {
    match version {
        1 => Ok(serde_json::json!({ "learner_id": payload["user_id"] })),
        _ => anyhow::bail!("unknown version {version}"),
    }
}
```

## The Queue page

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
- Each action re-reads the user's permissions under the RBAC lock, needs both
  permissions, and is recorded in the audit log (`queue.retried`,
  `queue.forgotten`, `queue.cancelled`, subject `job`, with its kind and
  queue). A job in the wrong state answers 409: `not_failed` for retry and
  forget of any job that has not failed (running ones too), `running` or
  `not_waiting` for cancel; a job that is gone, or a malformed id, 404 (after
  the permission checks). While an action's outcome is unknown (the
  connection dropped), the page pauses further actions until it is resolved.
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

The same statuses and actions are available in Rust, for your own tools:
`rustcms::queue::manage::{counts, retry, forget, cancel}` with a
`rustcms::queue::Clock`. The commands `queue:failed`, `queue:retry` and
`queue:forget` remain for scripts and cron.

## Permissions and the audit log

A job does not carry the permissions of the user who asked for it. When the
work depends on them, load the user and their grants again in `handle`.
Changes a job records in the audit log appear as actor `job`, on behalf of
its initiator, with credential `queue:<kind>`
(`ctx.audit_context()`).

## Several instances

Every instance may run workers: a job is claimed by one worker at a time
(an owner token and a lease of `queue.lease_secs`, renewed while it runs).
Keep the instances' clocks synchronized (NTP); `queue.clock_skew_secs`
(5 s) is the difference tolerated before a lease is taken over.

## Configuration

`[queue]` (`config/queue.toml`):

| Key | Default | |
|---|---|---|
| `enabled` | `true` | workers in `serve` |
| `workers` | `2` | per process (`serve`, `queue:work`); 0 to 64 |
| `queues` | `["default"]` | queues the workers take jobs from |
| `lease_secs` | `60` | 10 to 3,600 |
| `poll_ms` | `1000` | wait when no job is ready; 100 to 60,000 |
| `shutdown_secs` | `30` | 0 to 3,600 |
| `max_timeout_secs` | `3600` | caps every job's `timeout()` |
| `history_retention_days` | `7` | finished jobs kept in the history; 0 to 3,650 (0: no history) |
| `clock_skew_secs` | `5` | 0 to 60 |
| `unknown_kind_grace_secs` | `3600` | |
| `failed_retention_days` | `30` | failed jobs and `once` receipts |

Boot fails on a value out of range.

Code that blocks the thread (no `.await`) cannot be stopped by the timeout:
run it with `tokio::task::spawn_blocking`.
