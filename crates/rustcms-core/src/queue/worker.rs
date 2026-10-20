//! Workers: claim a job (owner token + lease, committed before it runs), run
//! it with heartbeats and a timeout, then settle it only if still the owner.

use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use sea_orm::sea_query::{Condition, Expr, ExprTrait, LockBehavior, LockType};
use sea_orm::{ColumnTrait, DatabaseConnection, DbBackend, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use super::history::{self, Terminal};
use super::{Handler, JobCtx, Jobs, MAX_PAYLOAD_BYTES, Outcome, QueueConfig, now};
use crate::models::{job, job_receipt};
use crate::{AppResult, AppState};

/// How long a job no worker knows waits before another look.
const UNKNOWN_RETRY: Duration = Duration::from_secs(60);
/// How often a worker deletes old failed jobs and receipts.
const MAINTENANCE_EVERY: Duration = Duration::from_secs(3_600);
/// Longest wait for one bookkeeping statement (settle, release) once a run ended.
const BOOKKEEPING: Duration = Duration::from_secs(10);

/// What proves a claim: the job and the owner token.
#[derive(Clone)]
struct Owner {
    id: uuid::Uuid,
    token: String,
}

/// A claimed job.
struct Claimed {
    row: job::Model,
    owner: Owner,
}

/// A handler task that is stopped when its run is dropped (cancelled).
struct Running(JoinHandle<Outcome>);

impl Drop for Running {
    fn drop(&mut self) {
        self.0.abort();
    }
}

fn after(at: DateTime<Utc>, by: Duration) -> DateTime<Utc> {
    at + chrono::Duration::from_std(by).unwrap_or(chrono::Duration::MAX)
}

fn before(at: DateTime<Utc>, by: Duration) -> DateTime<Utc> {
    at - chrono::Duration::from_std(by).unwrap_or(chrono::Duration::zero())
}

/// Takes the next ready job of `queues`, if any. The claim commits before the
/// job runs: a crash then leaves it to expire and run again.
async fn claim(db: &DatabaseConnection, config: &QueueConfig, queues: &[String]) -> AppResult<Option<Claimed>> {
    let txn = crate::db::begin_write(db).await?;
    // Read the clock once the transaction holds what it waited for.
    let now = now();
    let stale = before(now, Duration::from_secs(config.clock_skew_secs));
    let mut find = job::Entity::find()
        .filter(job::Column::Queue.is_in(queues.iter().cloned()))
        .filter(job::Column::FailedAt.is_null())
        .filter(job::Column::AvailableAt.lte(now))
        .filter(Expr::col(job::Column::Attempts).lt(Expr::col(job::Column::MaxAttempts)))
        .filter(Condition::any().add(job::Column::ReservedUntil.is_null()).add(job::Column::ReservedUntil.lt(stale)))
        .order_by_asc(job::Column::AvailableAt)
        .order_by_asc(job::Column::Id)
        .limit(1);
    // SQLite: `begin_write` holds the database's write lock already.
    if db.get_database_backend() != DbBackend::Sqlite {
        find = find.lock_with_behavior(LockType::Update, LockBehavior::SkipLocked);
    }
    let Some(mut row) = find.one(&txn).await? else {
        txn.commit().await?;
        return Ok(None);
    };
    let token = format!("{:032x}", rand::random::<u128>());
    // The lease starts when the row is held, not when the claim began waiting.
    let until = after(super::now(), Duration::from_secs(config.lease_secs));
    job::Entity::update_many()
        .col_expr(job::Column::ReservedBy, Expr::value(token.clone()))
        .col_expr(job::Column::ReservedUntil, Expr::value(until))
        .col_expr(job::Column::Attempts, Expr::col(job::Column::Attempts).add(1))
        .col_expr(job::Column::StartedAt, Expr::value(now))
        .filter(job::Column::Id.eq(row.id))
        .exec(&txn)
        .await?;
    txn.commit().await?;
    row.started_at = Some(now);
    row.attempts += 1;
    row.reserved_by = Some(token.clone());
    row.reserved_until = Some(until);
    Ok(Some(Claimed { owner: Owner { id: row.id, token }, row }))
}

/// The rows this claim still owns (its token, its lease not expired).
fn owned(owner: &Owner, at: DateTime<Utc>) -> Condition {
    Condition::all()
        .add(job::Column::Id.eq(owner.id))
        .add(job::Column::ReservedBy.eq(owner.token.clone()))
        .add(job::Column::ReservedUntil.gte(at))
}

/// Extends the lease; `false` when the claim is no longer the owner.
async fn heartbeat(db: DatabaseConnection, owner: Owner, lease: Duration) -> AppResult<bool> {
    let now = now();
    let result = job::Entity::update_many()
        .col_expr(job::Column::ReservedUntil, Expr::value(after(now, lease)))
        .filter(owned(&owner, now))
        .exec(&db)
        .await?;
    Ok(result.rows_affected == 1)
}

/// Settles a run as `outcome`, if the claim still owns the job. A terminal
/// outcome is recorded (history, schedule run) in the same transaction, only
/// when the transition changed the job.
async fn settle(
    db: &DatabaseConnection,
    config: &QueueConfig,
    handler: Option<&Arc<dyn Handler>>,
    claim: &Claimed,
    outcome: Outcome,
) -> AppResult<()> {
    let txn = crate::db::begin_write(db).await?;
    let now = now();
    let row = &claim.row;
    let release = |update: sea_orm::UpdateMany<job::Entity>| {
        update
            .col_expr(job::Column::ReservedBy, Expr::value(Option::<String>::None))
            .col_expr(job::Column::ReservedUntil, Expr::value(Option::<DateTime<Utc>>::None))
            .filter(owned(&claim.owner, now))
    };
    let fail = |message: String| {
        release(job::Entity::update_many())
            .col_expr(job::Column::FailedAt, Expr::value(now))
            .col_expr(job::Column::LastError, Expr::value(message))
    };
    let (affected, terminal) = match outcome {
        Outcome::Done(summary) => {
            let deleted = job::Entity::delete_many().filter(owned(&claim.owner, now)).exec(&txn).await?.rows_affected;
            (deleted, Some((Terminal::Done, None, summary)))
        }
        Outcome::Retry(message) if row.attempts < row.max_attempts => {
            let wait = handler.map_or(Duration::from_secs(60), |h| h.backoff(row.attempts as u32));
            let released = release(job::Entity::update_many())
                .col_expr(job::Column::AvailableAt, Expr::value(after(now, wait)))
                .col_expr(job::Column::LastError, Expr::value(message))
                .exec(&txn)
                .await?
                .rows_affected;
            (released, None)
        }
        Outcome::Retry(message) | Outcome::Fail(message) => {
            let failed = fail(message.clone()).exec(&txn).await?.rows_affected;
            (failed, Some((Terminal::Failed, Some(message), None)))
        }
        Outcome::Unknown => {
            let grace = Duration::from_secs(config.unknown_kind_grace_secs);
            if row.created_at < before(now, grace) {
                let message = format!("no worker handles `{}` (version {})", row.kind, row.version);
                let failed = fail(message.clone()).exec(&txn).await?.rows_affected;
                (failed, Some((Terminal::Failed, Some(message), None)))
            } else {
                // Not a run: the attempt is given back.
                let released = release(job::Entity::update_many())
                    .col_expr(job::Column::Attempts, Expr::col(job::Column::Attempts).sub(1))
                    .col_expr(job::Column::AvailableAt, Expr::value(after(now, UNKNOWN_RETRY)))
                    .exec(&txn)
                    .await?
                    .rows_affected;
                (released, None)
            }
        }
    };
    if affected == 0 {
        txn.rollback().await?;
        tracing::warn!(kind = row.kind, id = %row.id, "job lost its lease before it was settled; it may run again");
        return Ok(());
    }
    if let Some((terminal, error, summary)) = terminal {
        let keep = config.history_retention_days > 0;
        history::record(&txn, row, terminal, error.as_deref(), summary.as_deref(), now, keep).await?;
    }
    txn.commit().await?;
    Ok(())
}

/// Gives a job back at once, and the interrupted attempt with it (shutdown is
/// not the job's failure): it runs again, even on its last attempt.
async fn give_back(db: &DatabaseConnection, owner: &Owner) -> AppResult<()> {
    let now = now();
    job::Entity::update_many()
        .col_expr(job::Column::ReservedBy, Expr::value(Option::<String>::None))
        .col_expr(job::Column::ReservedUntil, Expr::value(Option::<DateTime<Utc>>::None))
        .col_expr(job::Column::AvailableAt, Expr::value(now))
        .col_expr(job::Column::Attempts, Expr::col(job::Column::Attempts).sub(1))
        .filter(owned(owner, now))
        .exec(db)
        .await?;
    Ok(())
}

/// Ids of up to 1,000 rows matching `select`, read without locking: the
/// bookkeeping below then writes by primary key and never waits for rows
/// that running claims hold.
async fn ids(db: &DatabaseConnection, select: sea_orm::Select<job::Entity>) -> AppResult<Vec<uuid::Uuid>> {
    Ok(select.select_only().column(job::Column::Id).limit(1_000).into_tuple().all(db).await?)
}

/// Fails jobs whose lease expired on their last attempt (their worker died),
/// each in its own transaction with its history.
async fn reap(db: &DatabaseConnection, config: &QueueConfig) -> AppResult<u64> {
    let now = now();
    let stale = before(now, Duration::from_secs(config.clock_skew_secs));
    let expired = || {
        Condition::all()
            .add(job::Column::FailedAt.is_null())
            .add(job::Column::ReservedUntil.lt(stale))
            .add(Expr::col(job::Column::Attempts).gte(Expr::col(job::Column::MaxAttempts)))
    };
    let candidates = ids(db, job::Entity::find().filter(expired())).await?;
    let message = "the lease expired during the last attempt";
    let mut reaped = 0;
    for id in candidates {
        let txn = crate::db::begin_write(db).await?;
        let Some(row) = job::Entity::find_by_id(id).filter(expired()).one(&txn).await? else {
            txn.rollback().await?;
            continue;
        };
        let failed = job::Entity::update_many()
            .col_expr(job::Column::FailedAt, Expr::value(now))
            .col_expr(job::Column::LastError, Expr::value(message))
            .col_expr(job::Column::ReservedBy, Expr::value(Option::<String>::None))
            .col_expr(job::Column::ReservedUntil, Expr::value(Option::<DateTime<Utc>>::None))
            .filter(job::Column::Id.eq(id))
            .filter(expired())
            .exec(&txn)
            .await?
            .rows_affected;
        if failed == 1 {
            let keep = config.history_retention_days > 0;
            history::record(&txn, &row, Terminal::Failed, Some(message), None, now, keep).await?;
            txn.commit().await?;
            reaped += 1;
        } else {
            txn.rollback().await?;
        }
    }
    Ok(reaped)
}

/// Deletes failed jobs, receipts, the job history and schedule runs past
/// their retention (schedule runs too: Run now records them where no
/// scheduler runs).
async fn maintain(state: &AppState, config: &QueueConfig) -> AppResult<()> {
    let db = state.db();
    if let Ok(schedule) = crate::schedule::ScheduleConfig::of(state.config()) {
        let keep = chrono::Duration::days(i64::from(schedule.history_retention_days));
        crate::schedule::prune_runs(db, now() - keep).await?;
    }
    let cutoff = now() - chrono::Duration::days(i64::from(config.failed_retention_days));
    let old = ids(db, job::Entity::find().filter(job::Column::FailedAt.lt(cutoff))).await?;
    if !old.is_empty() {
        job::Entity::delete_many()
            .filter(job::Column::Id.is_in(old))
            .filter(job::Column::FailedAt.lt(cutoff))
            .exec(db)
            .await?;
    }
    let receipts: Vec<String> = job_receipt::Entity::find()
        .select_only()
        .column(job_receipt::Column::Key)
        .filter(job_receipt::Column::CreatedAt.lt(cutoff))
        .limit(1_000)
        .into_tuple()
        .all(db)
        .await?;
    if !receipts.is_empty() {
        job_receipt::Entity::delete_many().filter(job_receipt::Column::Key.is_in(receipts)).exec(db).await?;
    }
    history::prune(db, config.history_retention_days).await?;
    Ok(())
}

/// Waits for `future` at most [`BOOKKEEPING`], logging a failure.
async fn bookkeeping(what: &str, future: impl std::future::Future<Output = AppResult<()>>) {
    match tokio::time::timeout(BOOKKEEPING, future).await {
        Ok(Ok(())) => {}
        Ok(Err(err)) => tracing::warn!(%err, "queue bookkeeping failed: {what}"),
        Err(_) => tracing::warn!("queue bookkeeping timed out: {what}"),
    }
}

/// How a run ended for the worker.
enum Ending {
    /// The handler finished (or was timed out): settle it.
    Settle(Outcome),
    /// The claim is lost: someone else may own the job now.
    Lost,
    /// Shutdown: give the job back.
    GiveBack,
}

/// Runs one claimed job to its end; `abort` stops it (shutdown deadline).
async fn run(state: &AppState, jobs: &Jobs, config: &QueueConfig, claim: Claimed, abort: &mut watch::Receiver<bool>) {
    let db = state.db();
    let row = &claim.row;
    let Some(handler) = jobs.get(&row.kind).cloned() else {
        bookkeeping("give back an unknown job", settle(db, config, None, &claim, Outcome::Unknown)).await;
        return;
    };
    // A claim that waited so long its lease is gone does not run.
    if row.reserved_until.is_none_or(|until| until <= now()) {
        tracing::warn!(kind = row.kind, id = %row.id, "job claim expired before it ran");
        return;
    }
    let size = serde_json::to_vec(&row.payload).map_or(usize::MAX, |b| b.len());
    if size > MAX_PAYLOAD_BYTES {
        let outcome = Outcome::Fail("the payload is too large".into());
        bookkeeping("fail an oversized job", settle(db, config, Some(&handler), &claim, outcome)).await;
        return;
    }
    let ctx = JobCtx {
        state: state.clone(),
        id: row.id,
        attempt: row.attempts as u32,
        initiated_by: row.initiated_by,
        kind: row.kind.clone(),
    };
    let timeout = handler.timeout().min(Duration::from_secs(config.max_timeout_secs));
    let lease = Duration::from_secs(config.lease_secs);
    let (version, payload) = (row.version as u32, row.payload.clone());
    let runner = handler.clone();
    let mut task = Running(tokio::spawn(async move { runner.run(version, payload, ctx).await }));
    let deadline = tokio::time::Instant::now() + timeout;
    let mut beat = tokio::time::interval(lease / 3);
    beat.tick().await;
    // The last renewal; the run stops before the lease could lapse unrenewed.
    let mut renewed = tokio::time::Instant::now();
    let mut pending: Option<JoinHandle<AppResult<bool>>> = None;
    let ending = loop {
        let lapse = renewed + lease * 2 / 3;
        tokio::select! {
            result = &mut task.0 => break match result {
                Ok(outcome) => Ending::Settle(outcome),
                Err(err) if err.is_panic() => {
                    tracing::warn!(kind = row.kind, id = %row.id, "job panicked");
                    Ending::Settle(Outcome::Retry("panicked".into()))
                }
                Err(_) => Ending::Lost,
            },
            _ = beat.tick(), if pending.is_none() => {
                // In its own task: a slow database never blocks the timeout or a shutdown.
                let (db, owner) = (db.clone(), claim.owner.clone());
                pending = Some(tokio::spawn(async move {
                    tokio::time::timeout(lease / 3, heartbeat(db, owner, lease))
                        .await
                        .unwrap_or_else(|_| Err(crate::AppError::internal("heartbeat timed out")))
                }));
            }
            renewal = async { pending.as_mut().expect("guarded").await }, if pending.is_some() => {
                pending = None;
                match renewal {
                    Ok(Ok(true)) => renewed = tokio::time::Instant::now(),
                    // Someone else owns it now: this run's result does not count.
                    Ok(Ok(false)) => {
                        tracing::warn!(kind = row.kind, id = %row.id, "job lost its lease; stopped");
                        break Ending::Lost;
                    }
                    Ok(Err(err)) => tracing::warn!(kind = row.kind, id = %row.id, %err, "job heartbeat failed"),
                    Err(err) => tracing::warn!(kind = row.kind, id = %row.id, %err, "job heartbeat task failed"),
                }
            }
            () = tokio::time::sleep_until(lapse) => {
                tracing::warn!(kind = row.kind, id = %row.id, "job lease could not be renewed; stopped");
                break Ending::Lost;
            }
            () = tokio::time::sleep_until(deadline) => {
                break Ending::Settle(Outcome::Retry(format!("timed out after {} s", timeout.as_secs())));
            }
            changed = abort.changed() => if changed.is_err() || *abort.borrow() {
                break Ending::GiveBack;
            },
        }
    };
    if let Some(pending) = pending {
        pending.abort();
    }
    // Stopped for good before anyone else may take the job.
    if !task.0.is_finished() {
        task.0.abort();
        let _ = (&mut task.0).await;
    }
    match ending {
        Ending::Settle(outcome) => {
            bookkeeping("settle a job", settle(db, config, Some(&handler), &claim, outcome)).await
        }
        // Never run twice: the interrupted run fails instead of being given back.
        Ending::GiveBack if handler.at_most_once() => {
            let outcome = Outcome::Fail("interrupted by shutdown".into());
            bookkeeping("fail an interrupted job", settle(db, config, Some(&handler), &claim, outcome)).await
        }
        Ending::GiveBack => bookkeeping("give a job back", give_back(db, &claim.owner)).await,
        Ending::Lost => {}
    }
}

fn jobs_of(state: &AppState) -> Arc<Jobs> {
    state.service::<Jobs>().unwrap_or_default()
}

/// Runs ready jobs of `queues` (default: `queue.queues`) one after another
/// until none is ready or `max` ran; returns how many ran (`queue:work --once`,
/// cron, tests). Also deletes failed jobs and receipts past their retention.
pub async fn run_until_idle(state: &AppState, queues: Option<Vec<String>>, max: usize) -> AppResult<usize> {
    let config = QueueConfig::of(state.config()).map_err(crate::AppError::internal)?;
    let queues = queues.unwrap_or_else(|| config.queues.clone());
    let jobs = jobs_of(state);
    let (_keep, mut abort) = watch::channel(false);
    let mut ran = 0;
    // Bookkeeping never keeps the jobs from running.
    if let Err(err) = maintain(state, &config).await {
        tracing::warn!(%err, "queue maintenance failed");
    }
    if let Err(err) = reap(state.db(), &config).await {
        tracing::warn!(%err, "queue reaping failed");
    }
    while ran < max {
        let Some(claim) = claim(state.db(), &config, &queues).await? else { break };
        run(state, &jobs, &config, claim, &mut abort).await;
        ran += 1;
    }
    Ok(ran)
}

/// Running workers; [`Workers::shutdown`] stops them. Dropping them stops
/// them too (without waiting).
pub struct Workers {
    stop: watch::Sender<bool>,
    abort: watch::Sender<bool>,
    tasks: Vec<JoinHandle<()>>,
    shutdown: Duration,
}

impl Workers {
    /// Starts `count` workers on `queues` (default: `queue.queues`).
    pub fn start(state: &AppState, config: &QueueConfig, queues: Option<Vec<String>>, count: usize) -> Self {
        let (stop, _) = watch::channel(false);
        let (abort, _) = watch::channel(false);
        let queues = queues.unwrap_or_else(|| config.queues.clone());
        let jobs = jobs_of(state);
        let tasks = (0..count)
            .map(|_| {
                let (state, config, queues, jobs) = (state.clone(), config.clone(), queues.clone(), jobs.clone());
                let (mut stopped, mut aborted) = (stop.subscribe(), abort.subscribe());
                tokio::spawn(async move {
                    let poll = Duration::from_millis(config.poll_ms);
                    let mut maintained = Instant::now().checked_sub(MAINTENANCE_EVERY).unwrap_or_else(Instant::now);
                    while !*stopped.borrow() {
                        if maintained.elapsed() >= MAINTENANCE_EVERY {
                            maintained = Instant::now();
                            if let Err(err) = maintain(&state, &config).await {
                                tracing::warn!(%err, "queue maintenance failed");
                            }
                        }
                        if let Err(err) = reap(state.db(), &config).await {
                            tracing::warn!(%err, "queue reaping failed");
                        }
                        let claimed = match claim(state.db(), &config, &queues).await {
                            Ok(claimed) => claimed,
                            Err(err) => {
                                tracing::warn!(%err, "claiming a job failed");
                                None
                            }
                        };
                        match claimed {
                            Some(claim) => run(&state, &jobs, &config, claim, &mut aborted).await,
                            None => {
                                tokio::select! {
                                    () = tokio::time::sleep(poll) => {}
                                    // The supervisor is gone: stop.
                                    changed = stopped.changed() => if changed.is_err() { break },
                                }
                            }
                        }
                    }
                })
            })
            .collect();
        Self { stop, abort, tasks, shutdown: Duration::from_secs(config.shutdown_secs) }
    }

    /// Stops claiming (also what [`Workers::shutdown`] starts with).
    pub fn stop_claiming(&self) {
        let _ = self.stop.send(true);
    }

    /// Stops claiming, lets running jobs finish for `queue.shutdown_secs`,
    /// then stops them (they are given back, their attempt with them) and
    /// waits for every worker.
    pub async fn shutdown(mut self) {
        self.stop_claiming();
        let mut tasks = std::mem::take(&mut self.tasks);
        let drained = tokio::time::timeout(self.shutdown, async {
            for task in &mut tasks {
                let _ = task.await;
            }
        })
        .await;
        if drained.is_err() {
            let _ = self.abort.send(true);
            // Each run stops its handler and gives the job back (bounded).
            let joined = tokio::time::timeout(BOOKKEEPING * 2, async {
                for task in tasks.iter_mut().filter(|t| !t.is_finished()) {
                    let _ = task.await;
                }
            })
            .await;
            if joined.is_err() {
                tracing::warn!("queue workers did not stop in time");
                tasks.iter().for_each(JoinHandle::abort);
            }
        }
    }
}

impl Drop for Workers {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
        let _ = self.abort.send(true);
    }
}
