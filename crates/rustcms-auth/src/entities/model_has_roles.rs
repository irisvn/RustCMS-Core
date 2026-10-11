//! `model_has_roles` pivot (polymorphic).

use sea_orm::entity::prelude::*;

/// Role assigned to a model (a user).
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "model_has_roles")]
pub struct Model {
    /// Role.
    #[sea_orm(primary_key, auto_increment = false)]
    pub role_id: i64,
    /// Model kind, see [`super::USER_MODEL`].
    #[sea_orm(primary_key, auto_increment = false)]
    pub model_type: String,
    /// Model key as a string (integer or UUID keys).
    #[sea_orm(primary_key, auto_increment = false)]
    pub model_id: String,
}

/// No relations.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
