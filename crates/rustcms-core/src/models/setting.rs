//! `settings` table (created by `rustcms-migration`), read and written
//! through [`crate::settings`].

use sea_orm::entity::prelude::*;

/// One settings group.
#[derive(Clone, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "settings")]
pub struct Model {
    /// The group (`Settings::GROUP`).
    #[sea_orm(primary_key, auto_increment = false)]
    pub name: String,
    /// The values, without secrets (wire JSON).
    pub value: Json,
    /// `{field: base64(nonce ‖ ciphertext)}`, AES-256-GCM.
    pub secrets: Option<Json>,
    /// Incremented by every change (optimistic concurrency, encryption context).
    pub version: i64,
    /// Last change.
    pub updated_at: DateTimeUtc,
    /// Who made it, if a user.
    pub updated_by: Option<i64>,
}

impl std::fmt::Debug for Model {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Setting").field("name", &self.name).field("version", &self.version).finish_non_exhaustive()
    }
}

/// No relations.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
