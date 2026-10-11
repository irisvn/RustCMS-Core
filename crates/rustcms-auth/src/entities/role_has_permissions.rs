//! `role_has_permissions` pivot.

use sea_orm::entity::prelude::*;

/// Permission granted to a role.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "role_has_permissions")]
pub struct Model {
    /// Role.
    #[sea_orm(primary_key, auto_increment = false)]
    pub role_id: i64,
    /// Permission.
    #[sea_orm(primary_key, auto_increment = false)]
    pub permission_id: i64,
}

/// No relations.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
