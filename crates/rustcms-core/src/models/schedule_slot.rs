//! `schedule_slots` table: one row per claimed slot (see [`crate::schedule`]).

use sea_orm::entity::prelude::*;

/// A claimed slot.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "schedule_slots")]
pub struct Model {
    /// The schedule's name.
    #[sea_orm(primary_key, auto_increment = false)]
    pub name: String,
    /// The slot's instant (whole seconds, UTC).
    #[sea_orm(primary_key, auto_increment = false)]
    pub slot_at: DateTimeUtc,
    /// When it was claimed.
    pub claimed_at: DateTimeUtc,
    /// Which instance claimed it.
    pub instance: String,
    /// The job it enqueued (`.job` schedules).
    pub job_id: Option<Uuid>,
}

/// No relations.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
