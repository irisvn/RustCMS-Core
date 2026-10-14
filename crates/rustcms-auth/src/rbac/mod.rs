//! Roles and permissions: resolution (with cache) and mutations (with invariants).
//!
//! Every change to roles, permissions or role assignments goes through
//! [`RbacService`], which enforces:
//!
//! - the actor holds the required permission (`roles.assign`, `roles.update`, …);
//! - users only manage roles of their own guard;
//! - nobody grants, through a role or directly, a permission they do not hold;
//! - protected roles (system or privileged) are managed by super admins only,
//!   and system roles cannot be renamed, re-flagged or deleted;
//! - nobody changes their own roles or direct permissions, nor edits a role they hold;
//! - each guard keeps at least one active super admin.
//!
//! Changes are serialized by [`lock`] and authorize with the actor's grants
//! re-read under it.

mod queries;
mod resolver;
mod rules;
mod service;

use queries::*;
pub use resolver::*;
use rules::*;
pub use service::*;

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use moka::future::Cache;
use rustcms_core::models::user;
use rustcms_core::{AppError, AppResult, Principal, ValidationErrors};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, DatabaseTransaction, DbBackend, EntityTrait,
    PaginatorTrait, QueryFilter, QuerySelect, Set,
};

use crate::entities::{
    USER_MODEL, model_has_permissions as user_perm, model_has_roles as user_role, permission, role,
    role_has_permissions as role_perm,
};

/// Serializes RBAC changes for the rest of the transaction
/// ([`rustcms_core::db::xact_lock`] on `rbac`). Every [`RbacService`] change
/// takes it first, so checks and writes of concurrent changes never interleave
/// (READ COMMITTED reads after the lock see every earlier change).
pub async fn lock(txn: &DatabaseTransaction) -> AppResult<()> {
    Ok(rustcms_core::db::xact_lock(txn, "rbac").await?)
}
