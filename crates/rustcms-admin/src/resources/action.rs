//! Row and bulk actions.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use rustcms_core::AppResult;
use sea_orm::{DatabaseTransaction, EntityTrait};
use serde::Serialize;

use crate::Data;
use crate::context::Ctx;

/// Boxed `Send` future, used to store async closures.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

type Handler<E> = Arc<
    dyn for<'a> Fn(
            &'a Ctx,
            &'a DatabaseTransaction,
            Vec<<E as EntityTrait>::Model>,
            Data,
        ) -> BoxFuture<'a, AppResult<()>>
        + Send
        + Sync,
>;

/// A named operation run on selected records inside a transaction.
///
/// Each record is authorized individually with the policy ability `ability`
/// (default: the action name); denied records are reported, not processed.
///
/// Name the entity so the closure's parameter types are inferred:
///
/// ```rust,ignore
/// Action::<post::Entity>::new("publish", |_ctx, _txn, _posts, _data| Box::pin(async move { Ok(()) }))
/// ```
pub struct Action<E: EntityTrait> {
    pub(crate) name: &'static str,
    pub(crate) ability: &'static str,
    pub(crate) handler: Handler<E>,
    pub(crate) label: Option<String>,
    pub(crate) bulk: bool,
    pub(crate) confirm: Option<String>,
    pub(crate) color: Option<String>,
    pub(crate) icon: Option<String>,
    pub(crate) required_permission: Option<&'static str>,
}

impl<E: EntityTrait> Action<E> {
    /// Declares an action. The handler receives the authorized records.
    pub fn new<F>(name: &'static str, handler: F) -> Self
    where
        F: for<'a> Fn(&'a Ctx, &'a DatabaseTransaction, Vec<E::Model>, Data) -> BoxFuture<'a, AppResult<()>>
            + Send
            + Sync
            + 'static,
    {
        Self {
            name,
            ability: name,
            handler: Arc::new(handler),
            label: None,
            bulk: false,
            confirm: None,
            color: None,
            icon: None,
            required_permission: None,
        }
    }

    /// Refuses the action outright (403, and hides it from the schema) for users
    /// without `permission`. Per-record policy checks still apply to the others.
    pub fn requires_permission(mut self, permission: &'static str) -> Self {
        self.required_permission = Some(permission);
        self
    }

    /// Button label (default: the name, humanized).
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Also offered for selections of records.
    pub fn bulk(mut self) -> Self {
        self.bulk = true;
        self
    }

    /// Asks this question before running.
    pub fn requires_confirmation(mut self, question: impl Into<String>) -> Self {
        self.confirm = Some(question.into());
        self
    }

    /// Color (`primary`, `success`, `warning`, `danger`, `gray`).
    pub fn color(mut self, color: impl Into<String>) -> Self {
        self.color = Some(color.into());
        self
    }

    /// Icon name.
    pub fn icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    /// Overrides the policy ability checked per record.
    pub fn ability(mut self, ability: &'static str) -> Self {
        self.ability = ability;
        self
    }
}

/// Per-record result of an action.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ActionOutcome {
    /// Keys the handler processed.
    pub processed: Vec<String>,
    /// Keys the policy denied.
    pub denied: Vec<String>,
    /// Keys not found in the user's scope.
    pub missing: Vec<String>,
    /// This is the outcome of an earlier request with the same `Idempotency-Key`:
    /// the action did not run again.
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub replayed: bool,
}
