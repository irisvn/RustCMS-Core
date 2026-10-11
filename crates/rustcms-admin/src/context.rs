//! Per-request context handed to resources, policies, hooks and actions.

use std::sync::Arc;

use rustcms_core::Principal;
use sea_orm::{DatabaseConnection, DatabaseExecutor, DatabaseTransaction};

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
    /// The write in progress, if any (see [`Ctx::conn`]).
    pub txn: Option<Arc<DatabaseTransaction>>,
}

impl Ctx {
    /// Context outside any write.
    pub fn new(db: DatabaseConnection, principal: Principal, registry: Arc<Registry>) -> Self {
        Self { db, principal, registry, txn: None }
    }

    /// Shorthand for [`Principal::can`].
    pub fn can(&self, permission: &str) -> bool {
        self.principal.can(permission)
    }

    /// The connection policies and hooks should query: the transaction of the
    /// write in progress (so they see its state and never wait for a second
    /// pooled connection), otherwise the pool.
    pub fn conn(&self) -> DatabaseExecutor<'_> {
        match &self.txn {
            Some(txn) => DatabaseExecutor::from(txn.as_ref()),
            None => DatabaseExecutor::from(&self.db),
        }
    }

    pub(crate) fn within(&self, txn: Arc<DatabaseTransaction>) -> Self {
        Self { txn: Some(txn), ..self.clone() }
    }
}
