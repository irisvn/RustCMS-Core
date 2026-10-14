//! JWT refresh families (`jwt_families`): one per JWT sign-in.

use sea_orm::entity::prelude::*;

/// A refresh family. Its abilities and `auth_version` are fixed at issue.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "jwt_families")]
pub struct Model {
    /// Random id, carried by access tokens as `fam`.
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    /// Owner.
    pub user_id: i64,
    /// JSON array of abilities (`["*"]` for all of the user's API permissions).
    pub abilities: String,
    /// The user's `auth_version` at issue: any credential change ends the family.
    pub auth_version: i64,
    /// Sign-in time.
    pub created_at: DateTimeUtc,
    /// End of the family, however often it is refreshed.
    pub expires_at: DateTimeUtc,
    /// Logout or detected reuse.
    pub revoked_at: Option<DateTimeUtc>,
}

/// No relations.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
