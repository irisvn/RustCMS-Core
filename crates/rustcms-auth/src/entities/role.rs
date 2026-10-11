//! `roles`.

use sea_orm::entity::prelude::*;

/// A role within one guard.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "roles")]
pub struct Model {
    /// Primary key.
    #[sea_orm(primary_key)]
    pub id: i64,
    /// Unique per guard.
    pub name: String,
    /// Guard the role applies to (`admin`, `api`).
    pub guard_name: String,
    /// Created by migrations; name, guard and flags are immutable, cannot be deleted.
    pub is_system: bool,
    /// Only super admins may edit, assign or revoke it.
    pub is_privileged: bool,
    /// Creation time.
    pub created_at: DateTimeUtc,
}

/// No relations.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

/// Name of the system role that bypasses permission checks within its guard.
pub const SUPER_ADMIN: &str = "super_admin";

impl Model {
    /// The system super-admin role of its guard.
    pub fn is_super_admin(&self) -> bool {
        self.is_system && self.name == SUPER_ADMIN
    }

    /// Requires a super admin to change, assign or revoke.
    pub fn is_protected(&self) -> bool {
        self.is_system || self.is_privileged
    }
}
