//! The `exports` table: exports of resource lists made in the background.
use sea_orm::entity::prelude::*;

/// A background export.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "exports")]
pub struct Model {
    /// Id (UUID v7).
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// Who asked for it.
    pub user_id: i64,
    /// Their credential version then (the export stops if it changes).
    pub auth_version: i64,
    /// Resource slug.
    pub resource: String,
    /// The list query, canonical (typed filters, no page).
    pub query: Json,
    /// The fields exported, fixed at creation.
    pub columns: Json,
    /// Keys of the records exported, in order (checked again at download).
    pub keys: Option<Json>,
    /// `pending`, `running`, `done` or `failed`.
    pub status: String,
    /// Rows written.
    pub rows: i32,
    /// Storage driver of the file.
    pub disk: Option<String>,
    /// Object of the file; named at creation (a crash leaves it to pruning).
    pub storage_key: Option<String>,
    /// Why it failed (a message safe to show its owner).
    pub error: Option<String>,
    /// When it was asked for.
    pub created_at: DateTimeUtc,
    /// When the row and the file are deleted.
    pub expires_at: DateTimeUtc,
}

/// No relations.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
