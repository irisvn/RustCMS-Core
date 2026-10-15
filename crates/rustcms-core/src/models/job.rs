//! `jobs` table (created by `rustcms-migration`), managed by [`crate::queue`].

use sea_orm::entity::prelude::*;

/// A queued, running or failed job.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "jobs")]
pub struct Model {
    /// UUID v7.
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// Queue name.
    pub queue: String,
    /// `Job::KIND`.
    pub kind: String,
    /// `Job::VERSION` of the payload.
    pub version: i32,
    /// The job, serialized.
    pub payload: Json,
    /// Claims so far (each run counts when it starts).
    pub attempts: i32,
    /// Most runs.
    pub max_attempts: i32,
    /// Not run before this.
    pub available_at: DateTimeUtc,
    /// Owner token of the run in progress.
    pub reserved_by: Option<String>,
    /// The run in progress owns the job until then.
    pub reserved_until: Option<DateTimeUtc>,
    /// Why the last run failed (a safe message, never the error itself).
    pub last_error: Option<String>,
    /// Who asked for it, if a user.
    pub initiated_by: Option<i64>,
    /// When it was dispatched.
    pub created_at: DateTimeUtc,
    /// Set when it will not run again.
    pub failed_at: Option<DateTimeUtc>,
}

/// No relations.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
