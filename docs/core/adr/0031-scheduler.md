# ADR 0031: The scheduler

- Status: accepted (0.4.1, W4)
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
