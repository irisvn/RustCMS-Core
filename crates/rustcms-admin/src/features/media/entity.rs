//! The `media` table.
use sea_orm::entity::prelude::*;

/// A stored file.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "media")]
pub struct Model {
    /// Id (UUID v7).
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// Storage driver (`local`, `s3`).
    pub disk: String,
    /// Object key in the storage.
    pub key: String,
    /// Original file name, cleaned.
    pub name: String,
    /// Detected MIME type.
    pub mime: String,
    /// Size in bytes.
    pub size: i64,
    /// SHA-256 of the content, hex.
    pub sha256: String,
    /// Image width in pixels.
    pub width: Option<i32>,
    /// Image height in pixels.
    pub height: Option<i32>,
    /// Uploader.
    pub owner_id: i64,
    /// Upload time.
    pub created_at: DateTimeUtc,
    /// Trash marker (soft deletes).
    pub deleted_at: Option<DateTimeUtc>,
}

/// No relations.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
