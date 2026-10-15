//! The `imports` table: CSV files imported into a resource.
use sea_orm::entity::prelude::*;

/// An import.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "imports")]
pub struct Model {
    /// Id (UUID v7).
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// Who uploaded it.
    pub user_id: i64,
    /// Their credential version then (the import stops if it changes).
    pub auth_version: i64,
    /// Resource slug.
    pub resource: String,
    /// Storage driver of the file.
    pub disk: String,
    /// Object of the file.
    pub storage_key: String,
    /// SHA-256 of the file, hex.
    pub sha256: String,
    /// Column names of the file.
    pub header: Json,
    /// Rows of the file (header excluded).
    pub rows: i32,
    /// Column → field, fixed when it runs.
    pub mapping: Option<Json>,
    /// `uploaded`, `running`, `done` or `failed`.
    pub status: String,
    /// Records created.
    pub created: i32,
    /// Records updated.
    pub updated: i32,
    /// Rows that failed.
    pub failed: i32,
    /// Rows that failed: `[{row, field, message}]` (no values).
    pub report: Option<Json>,
    /// Why the import stopped (a message safe to show its owner).
    pub error: Option<String>,
    /// When it was uploaded.
    pub created_at: DateTimeUtc,
    /// When the row and the file are deleted.
    pub expires_at: DateTimeUtc,
}

/// No relations.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
