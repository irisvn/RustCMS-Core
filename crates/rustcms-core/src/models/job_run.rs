//! `job_runs` table: one row per finished job (done, failed, cancelled), kept
//! for `[queue] history_retention_days` (see [`crate::queue`]).

use sea_orm::entity::prelude::*;

/// A finished job.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "job_runs")]
pub struct Model {
    /// UUID v7 of this row.
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// The job (a retried job finishes more than once).
    pub job_id: Uuid,
    /// Queue name.
    pub queue: String,
    /// `Job::KIND`.
    pub kind: String,
    /// `Job::describe`.
    pub description: Option<String>,
    /// `done`, `failed` or `cancelled`.
    pub status: String,
    /// Claims used.
    pub attempts: i32,
    /// When it was dispatched.
    pub created_at: DateTimeUtc,
    /// When its last attempt started.
    pub started_at: Option<DateTimeUtc>,
    /// When it finished.
    pub finished_at: DateTimeUtc,
    /// How long its last attempt ran.
    pub duration_ms: Option<i64>,
    /// Why it failed (a safe message).
    pub last_error: Option<String>,
    /// Who asked for it, if a user.
    pub initiated_by: Option<i64>,
}

/// No relations.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
