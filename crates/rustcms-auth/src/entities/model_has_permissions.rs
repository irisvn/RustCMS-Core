//! `model_has_permissions` pivot (polymorphic).

use sea_orm::entity::prelude::*;

/// Permission granted directly to a model (a user).
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "model_has_permissions")]
pub struct Model {
    /// Permission.
    #[sea_orm(primary_key, auto_increment = false)]
    pub permission_id: i64,
    /// Model kind, see [`super::USER_MODEL`].
    #[sea_orm(primary_key, auto_increment = false)]
    pub model_type: String,
    /// Model key as a string.
    #[sea_orm(primary_key, auto_increment = false)]
    pub model_id: String,
}

/// No relations.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
