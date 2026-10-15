//! Per-request context handed to resources, policies, hooks and actions.

use std::sync::Arc;

use rustcms_core::Principal;
use rustcms_core::audit::{AuditActor, AuditContext, AuditEntry, RequestMeta};
use rustcms_core::i18n::Translator;
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
    /// Interface language of the request (`app.locales`).
    pub locale: String,
    /// The application's catalogs (see [`Ctx::t`]).
    pub translator: Arc<Translator>,
    /// Who acts, from where: the audit log's actor and request.
    pub audit_context: AuditContext,
}

impl Ctx {
    /// Context outside any write.
    /// Writes are audited as `principal` (no request facts: see [`Ctx::with_audit`]).
    pub fn new(db: DatabaseConnection, principal: Principal, registry: Arc<Registry>) -> Self {
        let audit_context = AuditContext::new(AuditActor::user(&principal), RequestMeta::default());
        Self {
            db,
            principal,
            registry,
            txn: None,
            idempotency_key: None,
            bound: None,
            locale: "en".into(),
            translator: Arc::default(),
            audit_context,
        }
    }

    /// Records of the resource `slug` as this user may list them: scope,
    /// policy and readable fields apply, as in the panel's table (for custom
    /// pages and slots).
    pub async fn records(
        &self,
        slug: &str,
        query: &crate::resource::ListQuery,
    ) -> rustcms_core::AppResult<crate::resource::Page> {
        let resource = self.registry.get(slug).ok_or(rustcms_core::AppError::NotFound)?.clone();
        resource.admit_list(self, query).await?;
        resource.list(self, query).await
    }

    /// The record `key` of the resource `slug` as this user may view it
    /// (404 when missing or out of scope, 403 when denied).
    pub async fn record(&self, slug: &str, key: &str) -> rustcms_core::AppResult<crate::resource::Envelope> {
        let resource = self.registry.get(slug).ok_or(rustcms_core::AppError::NotFound)?.clone();
        resource.find(self, key).await
    }

    /// The same context, audited as `audit_context`.
    pub fn with_audit(mut self, audit_context: AuditContext) -> Self {
        self.audit_context = audit_context;
        self
    }

    /// Queues `job` in `txn`, the transaction of the change it follows from
    /// (it exists only if that change commits), on behalf of this user.
    pub async fn dispatch<J: rustcms_core::queue::Job>(
        &self,
        txn: &DatabaseTransaction,
        job: &J,
    ) -> rustcms_core::AppResult<uuid::Uuid> {
        let user = self.audit_context.actor.user_id;
        rustcms_core::queue::dispatch_with(txn, job, None, user).await
    }

    /// Records `entry` in the audit log, in the transaction `txn` of the change
    /// it describes (for writes a hook or an action makes beyond the records
    /// the engine records itself).
    pub async fn audit(&self, txn: &DatabaseTransaction, entry: AuditEntry) -> rustcms_core::AppResult<()> {
        rustcms_core::audit::record(txn, &self.audit_context, entry).await
    }

    /// The same context in `locale`, translated with `translator`.
    pub fn with_locale(mut self, locale: impl Into<String>, translator: Arc<Translator>) -> Self {
        self.locale = locale.into();
        self.translator = translator;
        self
    }

    /// `source` in the request's language (e.g. a message of a hook), or as written.
    pub fn t(&self, source: &str) -> String {
        self.translator.get(&self.locale, None, source).to_owned()
    }

    /// Like [`Ctx::t`], looking in the table of resource `scope` first.
    pub fn t_in(&self, scope: &str, source: &str) -> String {
        self.translator.get(&self.locale, Some(scope), source).to_owned()
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
