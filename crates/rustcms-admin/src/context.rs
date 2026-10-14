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
    /// Client `Idempotency-Key` of this request (creates and actions).
    pub idempotency_key: Option<String>,
    /// A create through a relation: (relation id, fk field, canonical parent
    /// key). The create sets that field itself and refuses it from the input.
    pub bound: Option<(String, String, String)>,
}

impl Ctx {
    /// Context outside any write.
    pub fn new(db: DatabaseConnection, principal: Principal, registry: Arc<Registry>) -> Self {
        Self { db, principal, registry, txn: None, idempotency_key: None, bound: None }
    }

    /// The same context, creating through a relation (`(relation id, fk field, parent key)`).
    pub(crate) fn with_bound(mut self, bound: (String, String, String)) -> Self {
        self.bound = Some(bound);
        self
    }

    /// The same context with an `Idempotency-Key`: a create or action retried
    /// with the same key and request replays the first result instead of running again.
    pub fn with_idempotency_key(mut self, key: Option<String>) -> Self {
        self.idempotency_key = key;
        self
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
