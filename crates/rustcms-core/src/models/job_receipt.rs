//! `job_receipts` table: keys of effects already applied (see [`crate::queue::once`]).

use sea_orm::entity::prelude::*;

/// An effect that already happened.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "job_receipts")]
pub struct Model {
    /// The application's key for the effect.
    #[sea_orm(primary_key, auto_increment = false)]
    pub key: String,
    /// When it happened.
    pub created_at: DateTimeUtc,
}

/// No relations.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
