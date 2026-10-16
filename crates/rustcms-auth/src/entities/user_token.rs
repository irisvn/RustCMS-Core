//! `user_tokens`: one-time tokens of password resets and invitations.

use sea_orm::entity::prelude::*;

/// A token. Only an HMAC of its secret is stored; at most one per user and purpose.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "user_tokens")]
pub struct Model {
    /// The public half of the token.
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// Whose.
    pub user_id: i64,
    /// `password_reset` or `invitation`.
    pub purpose: String,
    /// The user's email when it was issued (a change kills it).
    pub email: String,
    /// The user's credential version when it was issued (a change kills it).
    pub auth_version: i64,
    /// Hex HMAC-SHA256 of purpose, id and secret under a key derived from `APP_KEY`.
    pub token_hash: String,
    /// The request (a queued job) it answers.
    pub request_id: Uuid,
    /// When that request was made.
    pub requested_at: DateTimeUtc,
    /// When this secret was made.
    pub issued_at: DateTimeUtc,
    /// When it stops working.
    pub expires_at: DateTimeUtc,
    /// When its mail was sent (a retry then sends nothing).
    pub sent_at: Option<DateTimeUtc>,
}

/// No relations.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
