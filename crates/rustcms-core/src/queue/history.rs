//! Run history (ADR 0016): a job's terminal transition records it in
//! `job_runs` and settles the schedule run it belongs to, in the transaction
//! of the transition itself (which must have changed exactly one row).

use chrono::{DateTime, Utc};
use sea_orm::sea_query::Expr;
use sea_orm::{ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, Set};

use crate::AppResult;
use crate::models::{job, job_run, schedule_run};

/// How a job ended for good.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Terminal {
    Done,
    Failed,
    Cancelled,
}

impl Terminal {
    fn job_status(self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    fn run_status(self) -> &'static str {
        match self {
            Self::Done => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

/// Records `row`'s end in `conn` (its transition's transaction): a history row
/// when `keep`, and the outcome of its schedule run, if any.
pub(crate) async fn record<C: ConnectionTrait>(
    conn: &C,
    row: &job::Model,
    terminal: Terminal,
    error: Option<&str>,
    summary: Option<&str>,
    at: DateTime<Utc>,
    keep: bool,
) -> AppResult<()> {
    let duration = row.started_at.map(|started| (at - started).num_milliseconds().max(0));
    let error = error.map(|e| super::cut(e, 500));
    if keep {
        job_run::ActiveModel {
            id: Set(uuid::Uuid::now_v7()),
            job_id: Set(row.id),
            queue: Set(row.queue.clone()),
            kind: Set(row.kind.clone()),
            description: Set(row.description.clone()),
            status: Set(terminal.job_status().to_owned()),
            attempts: Set(row.attempts),
            created_at: Set(row.created_at),
            started_at: Set(row.started_at),
            finished_at: Set(at),
            duration_ms: Set(duration),
            last_error: Set(error.clone()),
            initiated_by: Set(row.initiated_by),
        }
        .insert(conn)
        .await?;
    }
    schedule_run::Entity::update_many()
        .col_expr(schedule_run::Column::Status, Expr::value(terminal.run_status()))
        .col_expr(schedule_run::Column::FinishedAt, Expr::value(at))
        .col_expr(schedule_run::Column::DurationMs, Expr::value(duration))
        .col_expr(schedule_run::Column::Error, Expr::value(error))
        .col_expr(schedule_run::Column::Summary, Expr::value(summary.and_then(|s| super::clean(s, 200))))
        .filter(schedule_run::Column::JobId.eq(row.id))
        .filter(schedule_run::Column::Status.is_in(["dispatched", "running"]))
        .exec(conn)
        .await?;
    Ok(())
}

/// Deletes history rows older than `days` (up to 1,000 per call).
pub(crate) async fn prune<C: ConnectionTrait>(conn: &C, days: u32) -> AppResult<u64> {
    use sea_orm::{QueryOrder, QuerySelect};
    let cutoff = super::now() - chrono::Duration::days(i64::from(days));
    let old: Vec<uuid::Uuid> = job_run::Entity::find()
        .select_only()
        .column(job_run::Column::Id)
        .filter(job_run::Column::FinishedAt.lt(cutoff))
        .order_by_asc(job_run::Column::FinishedAt)
        .limit(1_000)
        .into_tuple()
        .all(conn)
        .await?;
    if old.is_empty() {
        return Ok(0);
    }
    Ok(job_run::Entity::delete_many().filter(job_run::Column::Id.is_in(old)).exec(conn).await?.rows_affected)
}
