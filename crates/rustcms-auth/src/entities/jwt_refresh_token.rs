//! JWT refresh tokens (`jwt_refresh_tokens`): rotated on every use.

use sea_orm::entity::prelude::*;

/// A refresh token. Only a keyed hash of its secret is stored; used tokens are
/// kept until their family ends, so a replay is recognized.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "jwt_refresh_tokens")]
pub struct Model {
    /// Primary key, also the public part of the token.
    #[sea_orm(primary_key)]
    pub id: i64,
    /// Family.
    pub family_id: String,
    /// Hex HMAC-SHA256 of the secret part (key derived from `APP_KEY`).
    pub token_hash: String,
    /// Issue time.
    pub created_at: DateTimeUtc,
    /// Idle expiry: unused until then, the token stops working.
    pub expires_at: DateTimeUtc,
    /// When it was exchanged for the next one.
    pub used_at: Option<DateTimeUtc>,
}

/// No relations.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
