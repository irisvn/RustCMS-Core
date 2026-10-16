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
    /// Invited and not yet accepted: cannot sign in until a password is set
    /// from the invitation.
    #[sea_orm(default_value = false)]
    pub invitation_pending: bool,
    /// When the invitation was sent first.
    pub invited_at: Option<DateTimeUtc>,
    /// Who invited them (may no longer exist).
    pub invited_by: Option<i64>,
    /// Creation time.
    pub created_at: DateTimeUtc,
    /// Last update time.
    pub updated_at: DateTimeUtc,
}

/// No relations yet.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

#[async_trait::async_trait]
impl ActiveModelBehavior for ActiveModel {
    /// New users are not invited unless said so: the insert always carries the
    /// value (also where the row is rebuilt from the active model, on SQLite).
    async fn before_save<C>(mut self, _db: &C, insert: bool) -> Result<Self, DbErr>
    where
        C: ConnectionTrait,
    {
        if insert && self.invitation_pending.is_not_set() {
            self.invitation_pending = sea_orm::ActiveValue::Set(false);
        }
        Ok(self)
    }
}
