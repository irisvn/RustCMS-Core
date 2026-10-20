//! Runs of schedules (ADR 0031): their history (`schedule_runs`), and Run now,
//! which enqueues the schedule's job, or the internal job that runs a closure
//! once in a worker (never retried, never given back).

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseTransaction, DbBackend, EntityTrait, QueryFilter,
    QueryOrder, QuerySelect, Set,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Action, Exec, Schedules, TaskCtx, Trigger};
use crate::models::{job, schedule_run, schedule_state};
use crate::queue::{Handler, Job, JobCtx, Jobs, Outcome};
use crate::{AppError, AppResult, AppState};

/// The internal job kind that runs a closure schedule for Run now.
pub(crate) const KIND: &str = "rustcms.schedule.run";

/// The payload of [`KIND`].
#[derive(Serialize, Deserialize)]
struct RunJob {
    name: String,
    run_id: uuid::Uuid,
}

#[async_trait]
impl Job for RunJob {
    const KIND: &'static str = KIND;
    const AT_MOST_ONCE: bool = true;

    fn max_attempts() -> u32 {
        1
    }

    fn describe(&self) -> Option<String> {
        Some(format!("run schedule {}", self.name))
    }

    async fn handle(self, _: JobCtx) -> anyhow::Result<()> {
        unreachable!("run by its own handler")
    }
}

struct RunHandler;

#[async_trait]
impl Handler for RunHandler {
    fn backoff(&self, _: u32) -> Duration {
        Duration::from_secs(60)
    }

    fn timeout(&self) -> Duration {
        // Bounded by `[queue] max_timeout_secs`; each schedule's own timeout applies inside.
        Duration::from_secs(86_400)
    }

    fn at_most_once(&self) -> bool {
        true
    }

    async fn run(&self, _version: u32, payload: Value, ctx: JobCtx) -> Outcome {
        let Ok(job) = serde_json::from_value::<RunJob>(payload) else {
            return Outcome::Fail("the payload does not match the job".into());
        };
        let schedules = ctx.state.service::<Schedules>();
        let Some(schedule) = schedules.as_ref().and_then(|s| s.get(&job.name)) else {
            return Outcome::Fail(format!("schedule `{}` is not declared", job.name));
        };
        let Some(Action::Run(exec)) = schedule.action.clone() else {
            return Outcome::Fail(format!("schedule `{}` does not run a closure", job.name));
        };
        mark_running(ctx.state.db(), job.run_id).await;
        let task = TaskCtx {
            state: ctx.state.clone(),
            run_id: job.run_id,
            trigger: Trigger::Manual,
            initiated_by: ctx.initiated_by,
        };
        match execute(&exec, task, schedule.timeout).await {
            Ok(summary) => Outcome::Done(summary),
            Err(message) => Outcome::Fail(message),
        }
    }
}

/// Registers [`KIND`] (closure schedules can be run now).
pub(crate) fn register(jobs: &mut Jobs) {
    jobs.register_internal(KIND, Arc::new(RunHandler));
}

/// Runs `exec` within `timeout`, catching a panic: its summary, or a safe message.
pub(crate) async fn execute(exec: &Exec, ctx: TaskCtx, timeout: Duration) -> Result<Option<String>, String> {
    let run = futures_util::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(exec(ctx)));
    match tokio::time::timeout(timeout, run).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => Err("panicked".to_owned()),
        Err(_) => Err(format!("timed out after {} s", timeout.as_secs())),
    }
}

async fn mark_running<C: ConnectionTrait>(db: &C, id: uuid::Uuid) {
    let result = schedule_run::Entity::update_many()
        .col_expr(schedule_run::Column::Status, Expr::value("running"))
        // `started_at` stays the dispatch time: it orders the history (and its cursors).
        .filter(schedule_run::Column::Id.eq(id))
        .filter(schedule_run::Column::Status.eq("dispatched"))
        .exec(db)
        .await;
    if let Err(err) = result {
        tracing::warn!(%err, run = %id, "schedule run not marked running");
    }
}

/// A new run row.
pub(crate) fn new_run(
    name: &str,
    slot: Option<DateTime<Utc>>,
    trigger: Trigger,
    status: &str,
    job_id: Option<uuid::Uuid>,
    initiated_by: Option<i64>,
) -> schedule_run::ActiveModel {
    schedule_run::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        name: Set(name.to_owned()),
        slot_at: Set(slot),
        trigger: Set(trigger.as_str().to_owned()),
        started_at: Set(crate::queue::now()),
        finished_at: Set(None),
        status: Set(status.to_owned()),
        job_id: Set(job_id),
        duration_ms: Set(None),
        summary: Set(None),
        error: Set(None),
        initiated_by: Set(initiated_by),
    }
}

/// Records how an in-process run ended (`succeeded`, `failed`, `unknown`).
pub(crate) async fn finish<C: ConnectionTrait>(
    db: &C,
    id: uuid::Uuid,
    status: &str,
    summary: Option<&str>,
    error: Option<&str>,
) -> AppResult<()> {
    let now = crate::queue::now();
    let Some(row) = schedule_run::Entity::find_by_id(id).one(db).await? else { return Ok(()) };
    schedule_run::Entity::update_many()
        .col_expr(schedule_run::Column::Status, Expr::value(status))
        .col_expr(schedule_run::Column::FinishedAt, Expr::value(now))
        .col_expr(schedule_run::Column::DurationMs, Expr::value((now - row.started_at).num_milliseconds().max(0)))
        .col_expr(schedule_run::Column::Summary, Expr::value(summary.and_then(|s| crate::queue::clean(s, 200))))
        .col_expr(schedule_run::Column::Error, Expr::value(error.map(|e| crate::queue::cut(e, 500))))
        .filter(schedule_run::Column::Id.eq(id))
        .filter(schedule_run::Column::Status.is_in(["running", "dispatched"]))
        .exec(db)
        .await?;
    Ok(())
}

/// Why Run now refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunNowError {
    /// No schedule has this name.
    Unknown,
    /// Its previous run is not finished.
    Running,
}

/// Runs schedule `name` now, in `txn` (the caller's, after its authorization):
/// locks the schedule's state row as slot claims do, refuses while its
/// previous run is unfinished, dispatches its job (or the internal job of a
/// closure) on behalf of `actor`, and records a manual run. `last_slot_at` is
/// left as it is.
pub async fn run_now(
    txn: &DatabaseTransaction,
    state: &AppState,
    name: &str,
    actor: Option<i64>,
) -> AppResult<Result<schedule_run::Model, RunNowError>> {
    let schedules = state.service::<Schedules>().unwrap_or_default();
    let Some(schedule) = schedules.get(name) else { return Ok(Err(RunNowError::Unknown)) };
    super::claim::ensure_state(txn, name).await?;
    let mut find = schedule_state::Entity::find_by_id(name.to_owned());
    if txn.get_database_backend() != DbBackend::Sqlite {
        find = find.lock_exclusive();
    }
    let Some(state_row) = find.one(txn).await? else {
        return Err(AppError::internal(format!("schedule `{name}` has no state row")));
    };
    let now = crate::queue::now();
    let (job_id, overlapping) = match &schedule.action {
        Some(Action::Job { dispatch, .. }) => {
            if schedule.without_overlapping
                && let Some(active) = state_row.active_job_id
                && job::Entity::find_by_id(active).filter(job::Column::FailedAt.is_null()).one(txn).await?.is_some()
            {
                return Ok(Err(RunNowError::Running));
            }
            (dispatch(txn, actor).await?, schedule.without_overlapping)
        }
        Some(Action::Run(_)) => {
            // A run of this schedule not finished and within its time is still going.
            let grace = super::ScheduleConfig::of(state.config()).map_or(30, |c| c.grace_secs);
            let window = chrono::Duration::from_std(schedule.timeout + Duration::from_secs(grace))
                .unwrap_or(chrono::Duration::MAX);
            if closure_busy(txn, name, now - window).await? {
                return Ok(Err(RunNowError::Running));
            }
            let run_id = uuid::Uuid::now_v7();
            let id = crate::queue::dispatch_with(txn, &RunJob { name: name.to_owned(), run_id }, None, actor).await?;
            let mut run = new_run(name, None, Trigger::Manual, "dispatched", Some(id), actor);
            run.id = Set(run_id);
            let model = run.insert(txn).await?;
            return Ok(Ok(model));
        }
        None => return Ok(Err(RunNowError::Unknown)),
    };
    if overlapping {
        schedule_state::Entity::update_many()
            .col_expr(schedule_state::Column::ActiveJobId, Expr::value(job_id))
            .col_expr(schedule_state::Column::UpdatedAt, Expr::value(now))
            .filter(schedule_state::Column::Name.eq(name))
            .exec(txn)
            .await?;
    }
    Ok(Ok(new_run(name, None, Trigger::Manual, "dispatched", Some(job_id), actor).insert(txn).await?))
}

/// Whether a closure run of `name` is unfinished: its job still waits or runs,
/// or (in process, no job) it started after `since`.
pub(crate) async fn closure_busy<C: ConnectionTrait>(db: &C, name: &str, since: DateTime<Utc>) -> AppResult<bool> {
    let open = schedule_run::Entity::find()
        .filter(schedule_run::Column::Name.eq(name))
        .filter(schedule_run::Column::Status.is_in(["running", "dispatched"]))
        .order_by_desc(schedule_run::Column::StartedAt)
        .limit(50)
        .all(db)
        .await?;
    for run in open {
        match run.job_id {
            Some(id) => {
                if job::Entity::find_by_id(id).filter(job::Column::FailedAt.is_null()).one(db).await?.is_some() {
                    return Ok(true);
                }
            }
            None if run.started_at > since => return Ok(true),
            None => {}
        }
    }
    Ok(false)
}

/// The state row of each schedule.
pub async fn states<C: ConnectionTrait>(db: &C) -> AppResult<Vec<schedule_state::Model>> {
    super::claim::states(db).await
}

/// Runs of `name`, newest first, older than `before` (a cursor `(started_at,
/// id)`), at most `limit`.
pub async fn runs<C: ConnectionTrait>(
    db: &C,
    name: &str,
    before: Option<(DateTime<Utc>, uuid::Uuid)>,
    limit: u64,
) -> AppResult<Vec<schedule_run::Model>> {
    let mut find = schedule_run::Entity::find().filter(schedule_run::Column::Name.eq(name));
    if let Some((at, id)) = before {
        find = find.filter(sea_orm::Condition::any().add(schedule_run::Column::StartedAt.lt(at)).add(
            sea_orm::Condition::all().add(schedule_run::Column::StartedAt.eq(at)).add(schedule_run::Column::Id.lt(id)),
        ));
    }
    Ok(find
        .order_by_desc(schedule_run::Column::StartedAt)
        .order_by_desc(schedule_run::Column::Id)
        .limit(limit)
        .all(db)
        .await?)
}

/// Deletes runs started before `before` (up to 1,000 per call).
pub(crate) async fn prune<C: ConnectionTrait>(db: &C, before: DateTime<Utc>) -> AppResult<u64> {
    let ids: Vec<uuid::Uuid> = schedule_run::Entity::find()
        .select_only()
        .column(schedule_run::Column::Id)
        .filter(schedule_run::Column::StartedAt.lt(before))
        .order_by_asc(schedule_run::Column::StartedAt)
        .limit(1_000)
        .into_tuple()
        .all(db)
        .await?;
    if ids.is_empty() {
        return Ok(0);
    }
    Ok(schedule_run::Entity::delete_many().filter(schedule_run::Column::Id.is_in(ids)).exec(db).await?.rows_affected)
}
