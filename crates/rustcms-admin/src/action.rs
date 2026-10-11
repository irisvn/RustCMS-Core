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
pub struct Action<E: EntityTrait> {
    pub(crate) name: &'static str,
    pub(crate) ability: &'static str,
    pub(crate) handler: Handler<E>,
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
        Self { name, ability: name, handler: Arc::new(handler) }
    }

    /// Overrides the policy ability checked per record.
    pub fn ability(mut self, ability: &'static str) -> Self {
        self.ability = ability;
        self
    }
}

/// Per-record result of an action.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct ActionOutcome {
    /// Keys the handler processed.
    pub processed: Vec<String>,
    /// Keys the policy denied.
    pub denied: Vec<String>,
    /// Keys not found in the user's scope.
    pub missing: Vec<String>,
}
