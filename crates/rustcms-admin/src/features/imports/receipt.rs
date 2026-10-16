//! The `import_rows` table: one receipt per row an import wrote, committed
//! with the write (a later run of the job skips the row).
use sea_orm::entity::prelude::*;

/// A row written.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "import_rows")]
pub struct Model {
    /// The import.
    #[sea_orm(primary_key, auto_increment = false)]
    pub import_id: Uuid,
    /// Line in the file (the header is line 1).
    #[sea_orm(primary_key, auto_increment = false)]
    pub row: i32,
    /// `created` or `updated`.
    pub outcome: String,
    /// The record written.
    pub record_key: String,
}

/// No relations.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
