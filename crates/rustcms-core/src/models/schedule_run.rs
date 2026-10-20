//! `schedule_runs` table: one row per run of a schedule, kept for
//! `[schedule] history_retention_days` (see [`crate::schedule`]).

use sea_orm::entity::prelude::*;

/// A run of a schedule.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "schedule_runs")]
pub struct Model {
    /// UUID v7 (also the `TaskCtx::run_id`).
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// The schedule's name.
    pub name: String,
    /// The slot it ran for (`None` for a manual run).
    pub slot_at: Option<DateTimeUtc>,
    /// `schedule` or `manual`.
    pub trigger: String,
    /// When it started (or was dispatched).
    pub started_at: DateTimeUtc,
    /// When it finished.
    pub finished_at: Option<DateTimeUtc>,
    /// `running`, `dispatched`, `succeeded`, `failed`, `skipped`, `cancelled` or `unknown`.
    pub status: String,
    /// The job that runs it.
    pub job_id: Option<Uuid>,
    /// How long it ran.
    pub duration_ms: Option<i64>,
    /// A task's summary.
    pub summary: Option<String>,
    /// Why it failed (a safe message).
    pub error: Option<String>,
    /// Who ran it, if a user.
    pub initiated_by: Option<i64>,
}

/// No relations.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
