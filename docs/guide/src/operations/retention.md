# What is kept, and for how long

RustCMS deletes expired data on its own, once per period, in background
passes. This page lists what each table keeps, which pass deletes it, and
which commands do the same from cron when no long-running process is up.

## Background passes

The auth and idempotency cleanups are framework
[schedules](../digging-deeper/scheduling.md) (`rustcms.auth-prune`,
`rustcms.idempotency-prune`): they run only in `serve` with
`[schedule] enabled` (the default), once per slot across instances (on the
hour, UTC), never in `queue:work`, console commands or tests. They show on
System → Schedules and in `schedule:list` with their runs and summaries.

The media and exports cleanups still start when the application boots, in
every process that boots it: `serve`, `queue:work`, and also an application
command for as long as it runs. With Redis, a lease keeps instances from
doing the same pass at the same time in the usual case (runs can still
overlap after a lease expires or when Redis fails); without, every process
runs it. Each deletion is safe to repeat.

| Pass | Every | First run | Deletes |
|---|---|---|---|
| auth cleanup (`rustcms.auth-prune`, `serve` only) | hour | next full hour | expired sessions, ended JWT sign-ins, expired reset and invitation links, audit entries past retention |
| idempotency cleanup (`rustcms.idempotency-prune`, `serve` only) | hour | next full hour | expired idempotency receipts |
| media cleanup | minute | at start | files of deleted media records, abandoned uploads |
| exports and imports cleanup | 10 minutes | at start | expired exports and imports, with their files |
| scheduler (`serve` only, while schedules are registered and `[schedule] enabled`) | hour | after one hour | schedule claims older than 7 days, schedule runs past retention |
| queue workers (`serve` with workers, `queue:work`) | hour | at start | failed jobs and `once` receipts, job history, schedule runs past retention |

## What is kept

| Data | Kept | Deleted by |
|---|---|---|
| Sessions | until idle `session_lifetime_minutes` (remembered: `remember_days`) | auth cleanup, `session:prune` |
| JWT sign-ins | until their family ends (`refresh_family_days`) | auth cleanup, `session:prune` |
| Reset and invitation links | until they expire (marks of credential changes: 30 days) | auth cleanup, `schedule:run rustcms.auth-prune` |
| Audit entries | `audit.retention_days` (365) | auth cleanup, `audit:prune` |
| Idempotency receipts | 24 hours | idempotency cleanup, `idempotency:prune` |
| Schedule claims (`schedule_slots`) | 7 days (`schedule_states` are kept) | the scheduler only |
| Failed jobs, `once` receipts | `queue.failed_retention_days` (30) | the workers, `queue:work --once` |
| Job history (`job_runs`) | `queue.history_retention_days` (7; 0 keeps no history) | the workers, `queue:work --once` |
| Schedule runs (`schedule_runs`) | `schedule.history_retention_days` (7) | the scheduler, the workers, `queue:work --once` |
| Background exports (file and row) | 24 hours, removed an hour later | exports and imports cleanup only |
| Imports (file, row, report, receipts) | 24 hours from upload, or from the start of the run | exports and imports cleanup only |
| Media files of deleted media records, interrupted uploads | removed within minutes (queued deletions), an hour (abandoned uploads) | media cleanup, `media:prune` |
| Uploaded media | until deleted from the Media library (a cover removed from a record, or an abandoned draft's, stays) | by hand |

## From cron

When `serve` is not running all the time (workers only, maintenance
windows), run the commands from cron:

```sh
rustcms session:prune       # sessions and ended JWT sign-ins
rustcms schedule:run rustcms.auth-prune   # or: the whole auth cleanup, reset links included
rustcms audit:prune
rustcms idempotency:prune
rustcms media:prune
rustcms queue:work --once   # failed jobs, receipts, job and schedule history (and the ready jobs)
```

`rustcms schedule:run rustcms.auth-prune` runs the whole auth cleanup once,
reset and invitation links included (no other command deletes those), and
records the run in the schedule history; it does not prune that history, nor
schedule claims (only `serve`'s scheduler and the workers do). No command
deletes exports and imports: they need a process that boots the application
and keeps running (`serve`, or `queue:work`). A command run from cron also
starts the media and exports passes for as long as it runs, so they may run
once at its start.

## Personal data

Queued jobs hold their payload until they are done or, failed, until pruned:
a password reset job holds the address it was asked for (never a link).
[Audit entries](../digging-deeper/audit-log.md#personal-data) hold user ids,
addresses and user agents. Keep backups of the database as protected as the
database itself.

## Related

- [Audit log](../digging-deeper/audit-log.md)
- [Running the queue](queue.md)
- [Scheduling](../digging-deeper/scheduling.md)
- [Several instances](../digging-deeper/instances.md)
- [Backup and restore](backup-restore.md)
