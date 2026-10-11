//! `sessions`.

use sea_orm::entity::prelude::*;

/// A stored admin session. The id is the SHA-256 of the cookie value.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "sessions")]
pub struct Model {
    /// Hex SHA-256 of the cookie token.
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    /// JSON session data.
    pub data: String,
    /// Expiry: sliding for signed-in sessions, fixed for anonymous ones.
    pub expires_at: DateTimeUtc,
    /// Id of the first session of this chain of re-issues (`None`: this row's id).
    /// Signing out ends the whole lineage.
    pub lineage: Option<String>,
}

/// No relations.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
