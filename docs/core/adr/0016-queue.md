# 0016 — Job queue in the database

- Status: accepted
- Date: 2026-10-19

## Context

Mail (M19), imports and exports (M17) need work outside requests, surviving
crashes, on one or several instances, without adding a broker. The
database is already shared and transactional.

## Decision

- **A `jobs` table** (kind, version, payload ≤ 64 KiB, attempts,
  `available_at`, `reserved_by` + `reserved_until`, safe `last_error`,
  `initiated_by`, `failed_at`). Done jobs are deleted.
- **Outbox**: `dispatch` takes the caller's transaction, so a job exists
  only if its change commits; an idempotent replay runs no hook, hence no
  second job. `enqueue` is the separate, explicit standalone form.
- **Claim**: in its own write transaction committed before the job runs
  (`FOR UPDATE SKIP LOCKED` on PostgreSQL and MySQL 8.4; SQLite's
  `BEGIN IMMEDIATE` serializes the select and update), a random owner token
  and a lease; `attempts` grows at claim only. Jobs out of attempts are not
  claimed; one whose lease expired on its last attempt is failed ("reaped").
- **Ownership**: heartbeats every lease/3 and every settle (delete, retry,
  fail, give back) require the token **and** an unexpired lease; a run that
  loses its lease, or cannot renew it in time, is stopped and its result
  discarded. A late ack changes nothing.
- **Time**: application clock, microseconds (as stored everywhere), with a
  tolerated skew before a lease is taken over (SQLite stores instants as
  text, so comparing with database time is not reliable).
- **Runs**: in a task with a timeout (abort), panics caught; errors keep a
  safe message only (`queue::Failure`, else a generic one), the chain goes
  to the log. Unknown kinds and newer payload versions are given back
  without counting an attempt (rolling deploys), failing after a grace;
  older versions go through `Job::upgrade`; undecodable payloads fail.
- **At least once**: documented; `queue::once` records a database effect in
  its transaction (`job_receipts`); external services get the job id.
- **Lifecycle**: providers only register; `serve` and `queue:work` start
  workers; `serve` stops claiming on the signal, while HTTP drains. Shutdown
  drains for `shutdown_secs`, then aborts each handler, **waits for it to
  stop**, and gives the job back with its attempt (a shutdown is not the
  job's failure; on the last attempt the job would otherwise be stranded).
  Dropped supervisors and cancelled runs stop their tasks. Workers and
  `--once` delete old failed jobs and receipts.
- **Bookkeeping never blocks a run**: heartbeats run in their own task,
  bounded by lease/3; the run stops if its lease could lapse unrenewed;
  settle and give-back are bounded. Reaping and retention read ids without
  locks, then write by primary key (MySQL's `DELETE` would otherwise wait for
  rows running claims hold). The lease is computed once the row is held.
- **Receipts** insert atomically (`ON CONFLICT DO NOTHING`; MySQL `INSERT
  IGNORE`): concurrent runs with one key get one `true`.
- **Actor**: `job`, user id of the initiator, credential `queue:<kind>`;
  permissions are not serialized (handlers reload them).

## Admin page (M15d)

`System → Queue`: statuses computed with the workers' own conditions
(`queue::manage::JobStatus`: lease cutoff `now - clock_skew_secs`; `stalled`
for an expired last attempt awaiting the reaper). Actions are single
conditional statements (retry `WHERE failed_at IS NOT NULL`, cancel with the
claim condition), run under `rbac::begin_as` (fresh grants, both
`queue.view` and `queue.manage`) and audited in the same transaction. No
bulk actions in the panel (the commands keep `--all`); payloads and worker
tokens are never sent.

## Consequences

- Polling (default 1 s per worker) costs one small query per idle worker.
- Clocks must be synchronized across instances (NTP).
- Blocking code cannot be stopped by the timeout (`spawn_blocking`).
- Ordering is best effort.

## Amendment (M19, 2026-10-24)

`Failure::permanent()` (`queue::Permanent`) fails a job at once instead of
retrying it (an error retrying cannot fix: a refused address, a missing
template). `queue:retry` still runs it again. A `Failure` (permanent or not)
is logged by its code only; other errors are logged in full as before.
