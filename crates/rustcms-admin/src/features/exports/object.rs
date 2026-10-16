//! The `export_objects` table: every object a run of an export job may have
//! written, recorded before the upload (pruning deletes them all).
use sea_orm::entity::prelude::*;

/// An object of an export run.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "export_objects")]
pub struct Model {
    /// Object key (unique to the run).
    #[sea_orm(primary_key, auto_increment = false)]
    pub storage_key: String,
    /// The export.
    pub export_id: Uuid,
    /// Storage driver.
    pub disk: String,
    /// When the run recorded it.
    pub created_at: DateTimeUtc,
}

/// No relations.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
