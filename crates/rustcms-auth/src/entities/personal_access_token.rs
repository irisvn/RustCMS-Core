//! API tokens (`personal_access_tokens`).

use sea_orm::entity::prelude::*;

/// A personal access token. Only the SHA-256 of its secret is stored.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "personal_access_tokens")]
pub struct Model {
    /// Primary key, also the public part of the token.
    #[sea_orm(primary_key)]
    pub id: i64,
    /// Owner.
    pub user_id: i64,
    /// Label chosen when issuing (device, integration).
    pub name: String,
    /// Hex SHA-256 of the secret part.
    pub token_hash: String,
    /// JSON array of abilities (`["*"]` for all of the user's permissions).
    pub abilities: String,
    /// Last authenticated use (updated at most once a minute).
    pub last_used_at: Option<DateTimeUtc>,
    /// Expiry, if any.
    pub expires_at: Option<DateTimeUtc>,
    /// Revocation time; revoked tokens never authenticate.
    pub revoked_at: Option<DateTimeUtc>,
    /// Creation time.
    pub created_at: DateTimeUtc,
}

/// No relations.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
