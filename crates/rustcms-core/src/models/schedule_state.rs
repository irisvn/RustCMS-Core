//! `schedule_states` table: one row per schedule, locked by each claim (see
//! [`crate::schedule`]).

use sea_orm::entity::prelude::*;

/// A schedule's state.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "schedule_states")]
pub struct Model {
    /// The schedule's name.
    #[sea_orm(primary_key, auto_increment = false)]
    pub name: String,
    /// The job of the last claimed slot (for `without_overlapping`).
    pub active_job_id: Option<Uuid>,
    /// The last claimed slot.
    pub last_slot_at: Option<DateTimeUtc>,
    /// When it last changed.
    pub updated_at: DateTimeUtc,
}

/// No relations.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
