# Running the queue

Mails, exports, imports, invitations and your own jobs run on the database
queue ([Queues](../digging-deeper/queues.md)).

## Where workers run

- `serve` runs `queue.workers` (2) workers next to the web server. Set
  `queue.enabled = false` on web instances that should not run jobs.
- `rustcms queue:work [--queue mail] [--workers 4]` runs workers alone (a
  separate process or container). Ctrl-C / SIGTERM lets running jobs finish
  within `queue.shutdown_secs`.
- Run at least one worker somewhere: without one, nothing is mailed, exported
  or imported (requests still answer, the jobs wait).

## Failed jobs

A job fails after its last attempt, at once on a permanent failure (a mail
the server refused, a template error), or when no worker knows its kind for
`queue.unknown_kind_grace_secs`. Look at them on **System → Queue** or:

```sh
rustcms queue:failed          # kind, attempts, the safe error code
rustcms queue:retry <id>      # or --all: run again (attempts start over)
rustcms queue:forget <id>     # or --all: delete
```

Fix the cause first (the SMTP server, the template, a missing permission),
then retry. Failed jobs are deleted after `queue.failed_retention_days` (30).

## Deploying

Workers must know a job before code dispatches it: deploy workers first (or
together). A job of an unknown kind is given back to other workers, and fails
only after `queue.unknown_kind_grace_secs` (1 hour) — long enough for a
rolling deploy.

## Delivery

Jobs run **at least once**: a run cut after its work but before the queue
records it runs again. Built-in jobs are written for it (import rows have
receipts, export objects are unique per run, mails carry their run's
`Message-ID`); write yours with `queue::once` or idempotency keys.

Keep the clocks of every instance synchronized (NTP).
