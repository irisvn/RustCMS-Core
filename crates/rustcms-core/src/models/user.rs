//! `users` table (created by `rustcms-migration`).

use sea_orm::entity::prelude::*;

/// A user able to sign in to the admin panel or the API.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "users")]
pub struct Model {
    /// Primary key.
    #[sea_orm(primary_key)]
    pub id: i64,
    /// Display name.
    pub name: String,
    /// Unique sign-in email.
    #[sea_orm(unique)]
    pub email: String,
    /// Argon2id hash. Never exposed.
    pub password: String,
    /// Disabled users cannot sign in.
    pub is_active: bool,
    /// Bumped to revoke every session and token of the user.
    pub auth_version: i64,
    /// Admin interface language the user chose; `None` follows the browser.
    pub locale: Option<String>,
    /// When the email was verified.
    pub email_verified_at: Option<DateTimeUtc>,
    /// Creation time.
    pub created_at: DateTimeUtc,
    /// Last update time.
    pub updated_at: DateTimeUtc,
}

/// No relations yet.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
