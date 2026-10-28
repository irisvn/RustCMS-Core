# ADR 0031: The scheduler

- Status: accepted (0.4.1, W4); amended in 0.8.0 (runs, Run now)
- Date: 2026-10-18

## Context

Apps need periodic work: RDockPanel reconciles every minute and prunes Docker
nightly. Until now, providers spawned ad-hoc `tokio` loops in `boot`. Those
loops also ran in console commands and tests, and every instance ran them
unless Redis was configured.

## Decision

- **Declaring:** `AppBuilder::schedule(Schedule::every(name, d) |
  Schedule::cron(name, expr)?)` with an action:
  - `.job(make)`, recommended: it enqueues a queue job and registers the job
    type; registering the same type again is a no-op, two types sharing a
    `KIND` panic.
  - `.run(f)`: runs in the process, best effort.
  - `.without_overlapping()`: only for `.job`.
  - Names: `[a-z0-9_.-]{1,64}`, unique; the prefix `rustcms.` is reserved.
- **Slots:**
  - Instants shared by every instance: the multiples of the period since the
    Unix epoch (10 s to 7 days, whole seconds), or the fire times of a
    five-field cron in **UTC**.
  - Cron day-of-month and day-of-week combine with OR (Vixie); there are no
    seconds, years or extensions. An expression with no fire time in five
    years is refused.
- **Clock and grace:**
  - Clocks are assumed to agree within about a second (NTP).
  - A slot may be claimed until `schedule.grace_secs` (30) after its instant.
  - Missed slots are skipped, never caught up: on waking only the latest
    elapsed slot runs, if it is within the grace.
  - Sleeps are at most 60 s and recomputed from wall time, so long waits and
    clock jumps are noticed.
  - A fast clock can start a slot early: the claim stops a second run of the
    slot, not an early one.
- **Claim, in `db::begin_write`, bounded to 5 s:**
  1. Lock the schedule's row in `schedule_states`. This serializes one
     schedule's claims across instances, adjacent slots included.
  2. Insert `(name, slot_at)` into `schedule_slots`. The unique key
     arbitrates: `ON CONFLICT DO NOTHING`, or on MySQL a plain insert whose
     error 1062 means another instance won (never `INSERT IGNORE`).
  3. Recheck the grace.
  4. With `without_overlapping`, skip the slot while the state row's
     `active_job_id` is a job still pending or running.
  5. For `.job`, dispatch the job in the same transaction and record its id.
  6. Commit.
  - A failed or timed-out attempt is retried with the same slot while within
    the grace and not stopping. Each claim call carries its own token (stored
    in the claim row and kept across its retries), and every failure is first
    reconciled from the row: only a row with this call's token counts as its
    own commit. A revisited slot, after the clock went back, therefore never
    runs twice.
- **Guarantee:** at most one committed enqueue (or `.run` invocation) per slot
  while its claim is kept. Jobs still run at least once, as queue jobs do. A
  crash after the commit leaves the job, which is durable; a `.run` can be
  lost or truncated.
- **Overlap:**
  - `.job`: slots enqueue independently unless `without_overlapping`, whose
    lock is the queue job's lease. Manual runs and `queue:retry` bypass it.
  - `.run`: never concurrent on one instance; across instances, long runs can
    overlap.
- **Only `serve` runs it.** `App::serve_on` owns the scheduler; console
  commands, `queue:work` and `TestApp` never start it. `[schedule] enabled =
  false` turns it off.
  - One task per schedule, each with at most one claim in flight, plus a
    pruning task.
  - Dropping the owner (a server error, a dropped future) cancels the
    schedules and aborts their tasks, actions included.
  - On the stop signal nothing new is claimed, and `.run` actions get
    `[realtime] shutdown_grace_secs`.
- **Pruning:** claims older than 7 days are deleted hourly, in batches of
  1,000. State rows are never pruned, so overlap protection outlives
  retention.
- **Commands:**
  - `schedule:list`;
  - `schedule:run <name>`: a manual run, with no claim, grace or overlap
    check, which ignores `enabled`.

## Consequences

- One supported path for periodic work. The framework's own prune loops
  (media, exports, idempotency) remain provider loops for now; moving them is
  a follow-up.
- No Redis is needed for single execution.
- Capacity: a 10 s schedule keeps about 60,000 claim rows, a minute schedule
  about 10,000.

## Amendment (0.8.0, 2026-10-20): runs and Run now

- **Run history:** every run is recorded in `schedule_runs`, in its claim's
  transaction:
  - `dispatched` (a job), `running` (a closure) or `skipped` (overlap);
  - a skipped slot is now committed with its run, so no other instance runs
    it later, and reconciliation rebuilds `Skipped`.
- **How a run ends:**
  - A job's terminal transition writes the outcome on the run (`succeeded`,
    `failed`, `cancelled`), never inferred from the job's absence.
  - A closure's result is written by the runner.
  - A shutdown with runs still going writes `unknown`. Crashes leave
    `running`, shown as "completion unknown" after the timeout plus an hour.
- **`.task`:** returns a summary or an error whose `queue::Failure` message
  is kept. `.run` errors are kept as a generic text, so `AppError` text is
  never persisted. `.timeout` bounds closures (1 h).
- **Run now:** `schedule::run_now(txn, …)` runs in the caller's (RBAC)
  transaction and locks the schedule's state row as claims do. It refuses
  while the previous run is unfinished:
  - a job schedule's job with `without_overlapping`;
  - a closure's Run now whose job still lives (whatever its age), or an
    in-process run younger than its timeout plus the grace;

  and a closure's scheduled slot is skipped while one of its Run now jobs
  lives. `started_at` stays the dispatch time (it orders runs and cursors).

  Job schedules dispatch their job. Closures dispatch the internal at-most-once
  job `rustcms.schedule.run {name, run_id}`, so long closures never run in a
  web request and a crash never repeats one. No named lock is added. The
  console's `schedule:run` stays in-process and is recorded as a manual run.

## Amendment (Unreleased, 2026-10-28): framework schedules

- The framework's own periodic work uses this scheduler too. The auth cleanup
  (`rustcms.auth-prune`: sessions, JWT sign-ins, reset and invitation links,
  audit retention) and the idempotency cleanup (`rustcms.idempotency-prune`)
  are hourly `.task` schedules registered in `AuthProvider::register` and
  `AdminProvider::register`; the `tokio::spawn` loops in `boot` and their
  Redis leases are gone (slot claims replace them; both prunes are safe when
  concurrent).
- `Schedule::framework()` (`#[doc(hidden)]`) marks a framework schedule: it
  asserts the name starts with `rustcms.` and waives only the reserved-prefix
  check of `Schedules::add` (characters, length, uniqueness and action checks
  stay). A trusted-code opt-in, not an access boundary.
- Consequences: they run only in `serve` with `[schedule] enabled`, never in
  `queue:work`, commands or tests; cron-only deployments use `schedule:run
  rustcms.auth-prune` or the prune commands (`session:prune`,
  `idempotency:prune`, `audit:prune`); `schedule:run` records a run but does
  not prune the schedule history. Each part of the auth cleanup runs even if
  another fails; the run then fails with `prune_failed`.
- Not moved: the media and exports/imports prunes are per disk and sweep each
  instance's local staging directory; a cluster-wide slot would leave other
  instances' directories unswept (plan `code-findings-fixes.md` B2).
