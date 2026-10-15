//! Looking at jobs and acting on them one at a time, as `queue:*` commands
//! and the admin panel's Queue page do. Statuses and conditions follow the
//! worker's own: a lease counts until `clock_skew_secs` after it ends.

use chrono::{DateTime, Utc};
use sea_orm::sea_query::{Condition, Expr, ExprTrait};
use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, PaginatorTrait, QueryFilter, QuerySelect};
use serde::Serialize;

use crate::AppResult;
use crate::models::job;

/// Where a job stands, as the workers see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    /// Runs as soon as a worker is free.
    Pending,
    /// Waits for its `available_at` (a backoff, a delayed dispatch).
    Scheduled,
    /// Owned by a worker (its lease, plus the clock skew, has not ended).
    Running,
    /// Its last attempt's lease ended without an answer: the next worker pass
    /// marks it failed.
    Stalled,
    /// Will not run again unless retried.
    Failed,
}

impl JobStatus {
    /// Every status, in display order.
    pub const ALL: [Self; 5] = [Self::Pending, Self::Scheduled, Self::Running, Self::Stalled, Self::Failed];

    /// `pending`, `scheduled`, …
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Scheduled => "scheduled",
            Self::Running => "running",
            Self::Stalled => "stalled",
            Self::Failed => "failed",
        }
    }

    /// The status named `name`.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == name)
    }

    /// The status of `job` at `clock`.
    pub fn of(job: &job::Model, clock: &Clock) -> Self {
        if job.failed_at.is_some() {
            Self::Failed
        } else if job.reserved_until.is_some_and(|until| until >= clock.cutoff) {
            Self::Running
        } else if job.reserved_until.is_some() && job.attempts >= job.max_attempts {
            Self::Stalled
        } else if job.available_at > clock.now {
            Self::Scheduled
        } else {
            Self::Pending
        }
    }

    /// The rows with this status at `clock`, as a SQL condition.
    pub fn condition(self, clock: &Clock) -> Condition {
        let failed = job::Column::FailedAt.is_not_null();
        let unfailed = job::Column::FailedAt.is_null();
        let running = job::Column::ReservedUntil.gte(clock.cutoff);
        let expired = job::Column::ReservedUntil.lt(clock.cutoff);
        let exhausted = Expr::col(job::Column::Attempts).gte(Expr::col(job::Column::MaxAttempts));
        let free = Condition::any().add(job::Column::ReservedUntil.is_null()).add(
            Condition::all()
                .add(expired.clone())
                .add(Expr::col(job::Column::Attempts).lt(Expr::col(job::Column::MaxAttempts))),
        );
        match self {
            Self::Failed => Condition::all().add(failed),
            Self::Running => Condition::all().add(unfailed).add(running),
            Self::Stalled => Condition::all().add(unfailed).add(expired).add(exhausted),
            Self::Scheduled => Condition::all().add(unfailed).add(free).add(job::Column::AvailableAt.gt(clock.now)),
            Self::Pending => Condition::all().add(unfailed).add(free).add(job::Column::AvailableAt.lte(clock.now)),
        }
    }
}

/// The instant a decision is made at, and the lease cutoff the workers use.
#[derive(Debug, Clone, Copy)]
pub struct Clock {
    /// Now (application time, microseconds).
    pub now: DateTime<Utc>,
    /// Leases ending before this are over (`now - clock_skew_secs`).
    pub cutoff: DateTime<Utc>,
}

impl Clock {
    /// Now, with the configured clock skew.
    pub fn new(clock_skew_secs: u64) -> Self {
        let now = super::now();
        let skew = chrono::Duration::seconds(i64::try_from(clock_skew_secs).unwrap_or(i64::MAX / 1_000));
        Self { now, cutoff: now - skew }
    }
}

/// How many jobs of `queue` are in each status.
#[derive(Debug, Clone, Default, Serialize)]
pub struct QueueCounts {
    /// [`JobStatus::Pending`].
    pub pending: u64,
    /// [`JobStatus::Scheduled`].
    pub scheduled: u64,
    /// [`JobStatus::Running`].
    pub running: u64,
    /// [`JobStatus::Stalled`].
    pub stalled: u64,
    /// [`JobStatus::Failed`].
    pub failed: u64,
}

impl QueueCounts {
    fn add(&mut self, status: JobStatus, count: u64) {
        match status {
            JobStatus::Pending => self.pending += count,
            JobStatus::Scheduled => self.scheduled += count,
            JobStatus::Running => self.running += count,
            JobStatus::Stalled => self.stalled += count,
            JobStatus::Failed => self.failed += count,
        }
    }
}

/// Counts per queue (queues with jobs only), by name.
pub async fn counts(db: &impl ConnectionTrait, clock: &Clock) -> AppResult<Vec<(String, QueueCounts)>> {
    let mut out: std::collections::BTreeMap<String, QueueCounts> = Default::default();
    for status in JobStatus::ALL {
        let rows: Vec<(String, i64)> = job::Entity::find()
            .select_only()
            .column(job::Column::Queue)
            .column_as(Expr::col(job::Column::Id).count(), "n")
            .filter(status.condition(clock))
            .group_by(job::Column::Queue)
            .into_tuple()
            .all(db)
            .await?;
        for (queue, n) in rows {
            out.entry(queue).or_default().add(status, u64::try_from(n).unwrap_or(0));
        }
    }
    Ok(out.into_iter().collect())
}

/// Why an action does not apply to a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// No such job.
    Missing,
    /// Retry and forget take failed jobs only.
    NotFailed,
    /// A worker owns it.
    Running,
    /// Cancel takes jobs waiting to run (pending, scheduled, stalled) only.
    NotWaiting,
}

/// After `0` rows were written: why.
async fn refusal(db: &impl ConnectionTrait, id: uuid::Uuid, clock: &Clock, failed_wanted: bool) -> AppResult<Refusal> {
    let Some(row) = job::Entity::find_by_id(id).one(db).await? else {
        return Ok(Refusal::Missing);
    };
    Ok(match JobStatus::of(&row, clock) {
        JobStatus::Running => Refusal::Running,
        _ if failed_wanted => Refusal::NotFailed,
        _ => Refusal::NotWaiting,
    })
}

/// A failed job runs again: attempts start over, it is available now. Its
/// id (and so `once` receipts and idempotency keys built from it) is kept.
pub async fn retry(db: &impl ConnectionTrait, id: uuid::Uuid, clock: &Clock) -> AppResult<Result<(), Refusal>> {
    let updated = job::Entity::update_many()
        .col_expr(job::Column::FailedAt, Expr::value(Option::<DateTime<Utc>>::None))
        .col_expr(job::Column::Attempts, Expr::value(0))
        .col_expr(job::Column::AvailableAt, Expr::value(clock.now))
        .col_expr(job::Column::ReservedBy, Expr::value(Option::<String>::None))
        .col_expr(job::Column::ReservedUntil, Expr::value(Option::<DateTime<Utc>>::None))
        .filter(job::Column::Id.eq(id))
        .filter(job::Column::FailedAt.is_not_null())
        .exec(db)
        .await?;
    if updated.rows_affected == 1 { Ok(Ok(())) } else { Ok(Err(refusal(db, id, clock, true).await?)) }
}

/// A failed job is deleted.
pub async fn forget(db: &impl ConnectionTrait, id: uuid::Uuid, clock: &Clock) -> AppResult<Result<(), Refusal>> {
    let deleted = job::Entity::delete_many()
        .filter(job::Column::Id.eq(id))
        .filter(job::Column::FailedAt.is_not_null())
        .exec(db)
        .await?;
    if deleted.rows_affected == 1 { Ok(Ok(())) } else { Ok(Err(refusal(db, id, clock, true).await?)) }
}

/// A job waiting to run (pending, scheduled or stalled) is deleted, in one
/// statement with the workers' claim condition: a worker that claimed it
/// first keeps it. What earlier attempts committed stays.
pub async fn cancel(db: &impl ConnectionTrait, id: uuid::Uuid, clock: &Clock) -> AppResult<Result<(), Refusal>> {
    let deleted = job::Entity::delete_many()
        .filter(job::Column::Id.eq(id))
        .filter(job::Column::FailedAt.is_null())
        .filter(
            Condition::any().add(job::Column::ReservedUntil.is_null()).add(job::Column::ReservedUntil.lt(clock.cutoff)),
        )
        .exec(db)
        .await?;
    if deleted.rows_affected == 1 { Ok(Ok(())) } else { Ok(Err(refusal(db, id, clock, false).await?)) }
}

/// Number of jobs matching `condition` (for tests and tools).
pub async fn count(db: &impl ConnectionTrait, condition: Condition) -> AppResult<u64> {
    Ok(job::Entity::find().filter(condition).count(db).await?)
}
