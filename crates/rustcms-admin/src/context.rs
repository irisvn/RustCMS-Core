//! Per-request context handed to resources, policies, hooks and actions.

use std::sync::Arc;

use rustcms_core::Principal;
use sea_orm::DatabaseConnection;

use crate::registry::Registry;

/// Everything a resource needs to serve one request.
#[derive(Clone)]
pub struct Ctx {
    /// Database pool.
    pub db: DatabaseConnection,
    /// Authenticated user.
    pub principal: Principal,
    /// All registered resources (used for relationships).
    pub registry: Arc<Registry>,
}

impl Ctx {
    /// Shorthand for [`Principal::can`].
    pub fn can(&self, permission: &str) -> bool {
        self.principal.can(permission)
    }
}
