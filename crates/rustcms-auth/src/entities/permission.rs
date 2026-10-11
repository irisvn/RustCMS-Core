//! `permissions`.

use sea_orm::entity::prelude::*;

/// A named permission within one guard (`posts.update`).
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "permissions")]
pub struct Model {
    /// Primary key.
    #[sea_orm(primary_key)]
    pub id: i64,
    /// Unique per guard. Never renamed.
    pub name: String,
    /// Guard the permission applies to.
    pub guard_name: String,
    /// Grouping for the UI (usually the resource slug).
    pub group: Option<String>,
    /// No longer declared by the application (kept for audit, ignored by checks).
    pub orphaned: bool,
    /// Creation time.
    pub created_at: DateTimeUtc,
}

/// No relations.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
