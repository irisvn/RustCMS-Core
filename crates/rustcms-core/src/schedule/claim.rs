//! Claiming a slot: once per slot across instances, in the database (ADR 0031).

use std::time::Duration;

use chrono::{DateTime, Utc};
use sea_orm::sea_query::{Expr, OnConflict};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseTransaction, DbBackend, EntityTrait,
    QueryFilter, QuerySelect, SqlErr,
};

use super::{Action, Clock, Schedule, Trigger};
use crate::models::{job, schedule_run, schedule_slot, schedule_state};
use crate::{AppError, AppResult, AppState};

/// Longest attempt at one claim.
const CLAIM_TIMEOUT: Duration = Duration::from_secs(5);
/// Wait between attempts after a transient failure.
const RETRY_WAIT: Duration = Duration::from_millis(500);

/// What a claim did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Claimed {
    /// This instance claimed the slot (with the job it enqueued).
    Mine(Option<uuid::Uuid>),
    /// Another instance did.
    Taken,
    /// Too late: past the grace.
    Late,
    /// The previous slot's job is still pending or running.
    Overlapping,
}

/// Makes sure `name` has its state row (inserted once, never removed).
pub(crate) async fn ensure_state<C: ConnectionTrait>(db: &C, name: &str) -> AppResult<()> {
    let row = schedule_state::ActiveModel {
        name: Set(name.to_owned()),
        active_job_id: Set(None),
        last_slot_at: Set(None),
        updated_at: Set(crate::queue::now()),
    };
    // sea-query renders MySQL's form as `ON DUPLICATE KEY IGNORE`, which MySQL
    // rejects: a plain insert there, its duplicate-key error meaning "exists".
    let insert = schedule_state::Entity::insert(row);
    let result = if db.get_database_backend() == DbBackend::MySql {
        insert.exec_without_returning(db).await
    } else {
        insert
            .on_conflict(OnConflict::column(schedule_state::Column::Name).do_nothing().to_owned())
            .exec_without_returning(db)
            .await
    };
    match result {
        Ok(_) => Ok(()),
        // MySQL reports the conflict as a duplicate-key error.
        Err(err) if matches!(err.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) => Ok(()),
        Err(err) => Err(err.into()),
    }
}

/// Claims `slot` of `schedule`. A failed or timed-out attempt (a lost
/// connection, a busy database, a commit whose answer was lost) is
/// reconciled from the claim row, by this call's own token, then retried with
/// the same slot while within `grace` and not `cancelled`.
pub(crate) async fn claim(
    state: &AppState,
    schedule: &Schedule,
    slot: DateTime<Utc>,
    grace: Duration,
    clock: &dyn Clock,
    cancelled: &tokio_util::sync::CancellationToken,
) -> AppResult<Claimed> {
    // Unique to this logical claim (kept across its retries): only a row
    // carrying it was committed by this call.
    let token = uuid::Uuid::now_v7().to_string();
    let late = |now: DateTime<Utc>| (now - slot).to_std().is_ok_and(|elapsed| elapsed > grace);
    let mut last_error = None;
    loop {
        if late(clock.now()) {
            return last_error.map_or(Ok(Claimed::Late), Err);
        }
        if cancelled.is_cancelled() {
            return last_error.map_or(Ok(Claimed::Taken), Err);
        }
        match tokio::time::timeout(CLAIM_TIMEOUT, attempt(state, schedule, slot, grace, clock, &token)).await {
            Ok(Ok(claimed)) => return Ok(claimed),
            Ok(Err(err)) => {
                tracing::warn!(schedule = %schedule.name, %err, "slot claim failed; retrying");
                last_error = Some(err);
            }
            Err(_) => tracing::warn!(schedule = %schedule.name, "slot claim timed out; retrying"),
        }
        // The attempt may have committed before failing to answer.
        let row = schedule_slot::Entity::find_by_id((schedule.name.clone(), slot)).one(state.db()).await;
        if let Ok(Some(row)) = row {
            if row.instance != token {
                return Ok(Claimed::Taken);
            }
            // Committed by this call: a skipped slot left a `skipped` run.
            let skipped = schedule_run::Entity::find()
                .filter(schedule_run::Column::Name.eq(schedule.name.clone()))
                .filter(schedule_run::Column::SlotAt.eq(slot))
                .filter(schedule_run::Column::Status.eq("skipped"))
                .one(state.db())
                .await;
            return Ok(if matches!(skipped, Ok(Some(_))) { Claimed::Overlapping } else { Claimed::Mine(row.job_id) });
        }
        tokio::time::sleep(RETRY_WAIT).await;
    }
}

async fn attempt(
    state: &AppState,
    schedule: &Schedule,
    slot: DateTime<Utc>,
    grace: Duration,
    clock: &dyn Clock,
    token: &str,
) -> AppResult<Claimed> {
    let txn = crate::db::begin_write(state.db()).await?;
    let outcome = claim_in(&txn, schedule, slot, grace, clock, token).await;
    match outcome {
        // A skipped slot is spent too: no other instance runs it later.
        Ok(claimed @ (Claimed::Mine(_) | Claimed::Overlapping)) => {
            txn.commit().await?;
            Ok(claimed)
        }
        Ok(other) => {
            txn.rollback().await?;
            Ok(other)
        }
        Err(err) => {
            let _ = txn.rollback().await;
            Err(err)
        }
    }
}

async fn claim_in(
    txn: &DatabaseTransaction,
    schedule: &Schedule,
    slot: DateTime<Utc>,
    grace: Duration,
    clock: &dyn Clock,
    token: &str,
) -> AppResult<Claimed> {
    let name = schedule.name.as_str();
    // The state row serializes the claims of one schedule (adjacent slots
    // included); SQLite's `BEGIN IMMEDIATE` already serializes writers.
    let mut find = schedule_state::Entity::find_by_id(name.to_owned());
    if txn.get_database_backend() != DbBackend::Sqlite {
        find = find.lock_exclusive();
    }
    let Some(state_row) = find.one(txn).await? else {
        return Err(AppError::internal(format!("schedule `{name}` has no state row")));
    };
    // The unique key arbitrates; no existence check first.
    let claim = schedule_slot::ActiveModel {
        name: Set(name.to_owned()),
        slot_at: Set(slot),
        claimed_at: Set(crate::queue::now()),
        instance: Set(token.to_owned()),
        job_id: Set(None),
    };
    let inserted = if txn.get_database_backend() == DbBackend::MySql {
        // A plain insert: a duplicate is error 1062 (the transaction stays
        // usable); `INSERT IGNORE` would hide other errors.
        match schedule_slot::Entity::insert(claim).exec_without_returning(txn).await {
            Ok(n) => n,
            Err(err) if matches!(err.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) => 0,
            Err(err) => return Err(err.into()),
        }
    } else {
        schedule_slot::Entity::insert(claim)
            .on_conflict(
                OnConflict::columns([schedule_slot::Column::Name, schedule_slot::Column::SlotAt])
                    .do_nothing()
                    .to_owned(),
            )
            .exec_without_returning(txn)
            .await?
    };
    if inserted != 1 {
        return Ok(Claimed::Taken);
    }
    // A wait for the lock may have crossed the grace.
    if (clock.now() - slot).to_std().is_ok_and(|elapsed| elapsed > grace) {
        return Ok(Claimed::Late);
    }
    if schedule.without_overlapping
        && let Some(active) = state_row.active_job_id
    {
        let alive = job::Entity::find_by_id(active).filter(job::Column::FailedAt.is_null()).one(txn).await?;
        if alive.is_some() {
            super::runs::new_run(name, Some(slot), Trigger::Schedule, "skipped", None, None).insert(txn).await?;
            return Ok(Claimed::Overlapping);
        }
    }
    // A closure's Run now still going (its job alive): this slot is skipped.
    // Scheduled runs of other slots (no job) are not counted, as before.
    if matches!(schedule.action, Some(Action::Run(_))) {
        let manual = super::runs::closure_busy(txn, name, DateTime::<Utc>::MAX_UTC).await?;
        if manual {
            super::runs::new_run(name, Some(slot), Trigger::Schedule, "skipped", None, None).insert(txn).await?;
            return Ok(Claimed::Overlapping);
        }
    }
    let job_id = match &schedule.action {
        Some(Action::Job { dispatch, .. }) => Some(dispatch(txn, None).await?),
        _ => None,
    };
    // The run is recorded with its claim: dispatched (a job) or running (a closure).
    let status = if job_id.is_some() { "dispatched" } else { "running" };
    super::runs::new_run(name, Some(slot), Trigger::Schedule, status, job_id, None).insert(txn).await?;
    if let Some(id) = job_id {
        schedule_slot::Entity::update_many()
            .col_expr(schedule_slot::Column::JobId, Expr::value(id))
            .filter(schedule_slot::Column::Name.eq(name))
            .filter(schedule_slot::Column::SlotAt.eq(slot))
            .exec(txn)
            .await?;
    }
    let mut update = schedule_state::Entity::update_many()
        .col_expr(schedule_state::Column::LastSlotAt, Expr::value(slot))
        .col_expr(schedule_state::Column::UpdatedAt, Expr::value(crate::queue::now()));
    if job_id.is_some() {
        update = update.col_expr(schedule_state::Column::ActiveJobId, Expr::value(job_id));
    }
    update.filter(schedule_state::Column::Name.eq(name)).exec(txn).await?;
    Ok(Claimed::Mine(job_id))
}

/// Deletes claims older than `keep`, in batches of 1,000 by key.
pub(crate) async fn prune<C: ConnectionTrait>(db: &C, before: DateTime<Utc>) -> AppResult<u64> {
    let mut deleted = 0;
    loop {
        let keys: Vec<(String, DateTime<Utc>)> = schedule_slot::Entity::find()
            .select_only()
            .column(schedule_slot::Column::Name)
            .column(schedule_slot::Column::SlotAt)
            .filter(schedule_slot::Column::ClaimedAt.lt(before))
            .limit(1000)
            .into_tuple()
            .all(db)
            .await?;
        if keys.is_empty() {
            return Ok(deleted);
        }
        // Small OR groups: SQLite limits the depth of an expression (1,000).
        for chunk in keys.chunks(100) {
            let mut any = sea_orm::Condition::any();
            for (name, slot) in chunk {
                any = any.add(
                    sea_orm::Condition::all()
                        .add(schedule_slot::Column::Name.eq(name.clone()))
                        .add(schedule_slot::Column::SlotAt.eq(*slot)),
                );
            }
            deleted += schedule_slot::Entity::delete_many().filter(any).exec(db).await?.rows_affected;
        }
    }
}

/// The last claimed slot of each schedule (`schedule:list`).
pub(crate) async fn states<C: ConnectionTrait>(db: &C) -> AppResult<Vec<schedule_state::Model>> {
    Ok(schedule_state::Entity::find().all(db).await?)
}
