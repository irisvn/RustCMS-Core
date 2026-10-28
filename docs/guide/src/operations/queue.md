# Running the queue

Mails, exports, imports, invitations, scheduled jobs and your own jobs run on
the database queue ([Queues](../digging-deeper/queues.md)). This page is for
operators: where workers run, what to do with failed jobs, and how to deploy.

## Where workers run

- `serve` runs `queue.workers` (2) workers next to the web server. Set
  `queue.enabled = false` (or `queue.workers = 0`) on web instances that
  should not run jobs.
- `rustcms queue:work` runs workers alone (a separate process or container).
  Ctrl-C / SIGTERM lets running jobs finish within `queue.shutdown_secs`
  (30 s).
- Run at least one worker somewhere: without one, nothing is mailed, exported
  or imported, and the panel's Run now does nothing (requests still answer,
  the jobs wait).

```sh
rustcms queue:work                              # queue.queues, queue.workers workers
rustcms queue:work --queue mail --workers 4     # only the `mail` queue, 4 workers
rustcms queue:work --queue mail --queue default # several queues (jobs taken by readiness, not by queue)
rustcms queue:work --once                       # the jobs ready now (at most 1,000), then exit
```

| Option | |
|---|---|
| `--queue <name>` | repeatable; default `queue.queues` |
| `--workers <n>` | 1 to 64; default `queue.workers` (at least 1) |
| `--once` | for cron or a one-off: deletes old failed jobs, receipts and history first, runs what is ready (at most 1,000 jobs), exits |

## Built-in jobs

What you see on **System → Queue** besides your own jobs:

| Kind | Runs | Timeout | |
|---|---|---|---|
| `mail.send` | 8 | 2 min | a mail queued with `send_later` |
| `auth.password_reset` | 5 | 2 min | makes the reset link and mails it; a request older than 15 minutes fails as `expired_before_sent` |
| `auth.invitation` | 8 | 2 min | makes the invitation link and mails it |
| `admin.export` | 3 | 10 min | a background export |
| `admin.import` | 3 | 30 min | an import run |
| `rustcms.schedule.run` | 1 | the schedule's, at most `queue.max_timeout_secs` | a `.run` or `.task` schedule's Run now; never run twice (a run cut by a shutdown fails) |

Job schedules enqueue their own job kinds ([Scheduling](../digging-deeper/scheduling.md)).

## Failed jobs

![The Queue page filtered on failed jobs, with Retry and Forget](/img/digging-deeper/queue-page.webp)
*The demo application: System → Queue, failed jobs.*

A job fails after its last attempt, at once on a permanent failure (a mail
the server refused, a template error), or when no worker knows its kind for
`queue.unknown_kind_grace_secs`. Look at them on **System → Queue** (users
with `queue.view`; acting needs `queue.manage` too) or:

```sh
rustcms queue:failed          # id, kind, attempts, when, the safe error message
rustcms queue:retry <id>      # or --all: run again (attempts start over)
rustcms queue:forget <id>     # or --all: delete
```

Fix the cause first (the SMTP server, the template, a missing permission),
then retry. On the Queue page, **Cancel** deletes a job that is still
waiting (pending, delayed or stalled).

Failed jobs and `once` receipts are deleted after
`queue.failed_retention_days` (30). Finished jobs are listed on the
**History** tab for `queue.history_retention_days` (7; 0 keeps none).
Workers delete both about once an hour.

## Deploying

Workers must know a job before code dispatches it: deploy workers first (or
together). A job of an unknown kind is given back to other workers, and fails
once `queue.unknown_kind_grace_secs` (1 hour) has passed since the job was
created — long enough for a rolling deploy of fresh jobs; a delayed job
older than that fails at once on a worker that does not know it.

## Delivery

Jobs run **at least once**: a run cut after its work but before the queue
records it runs again. Built-in jobs are written for it (import rows have
receipts, export objects are unique per run, mails carry their run's
`Message-ID`); write yours with `queue::once` or idempotency keys.

Keep the clocks of every instance synchronized (NTP): a lease counts until
`queue.clock_skew_secs` (5 s) after it ends.

## Related

- [Queues](../digging-deeper/queues.md#configuration) (every `[queue]` key)
- [Running mail](mail.md)
- [Data retention](retention.md)
- [Several instances](../digging-deeper/instances.md)
