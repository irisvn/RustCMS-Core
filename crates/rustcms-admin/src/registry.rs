//! Type-erased resources, looked up by slug.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use async_trait::async_trait;
use rustcms_core::AppResult;
use sea_orm::DatabaseTransaction;

use crate::Data;
use crate::action::ActionOutcome;
use crate::adapter::ResourceAdapter;
use crate::context::Ctx;
use crate::resource::{Envelope, ListQuery, Page, Resource, ResourceMeta};

/// Object-safe view of a [`Resource`], used by the non-generic HTTP layer.
#[async_trait]
pub trait DynResource: Send + Sync {
    /// URL slug.
    fn slug(&self) -> &'static str;
    /// Static metadata.
    fn meta(&self) -> &ResourceMeta;
    /// Slugs referenced by `belongs_to` fields.
    fn targets(&self) -> Vec<&'static str>;
    /// Lists records in scope.
    async fn list(&self, ctx: &Ctx, query: &ListQuery) -> AppResult<Page>;
    /// Reads one record.
    async fn find(&self, ctx: &Ctx, key: &str) -> AppResult<Envelope>;
    /// Creates a record from client input.
    async fn create(&self, ctx: &Ctx, input: Data) -> AppResult<Envelope>;
    /// Updates a record from client input (PATCH semantics).
    async fn update(&self, ctx: &Ctx, key: &str, input: Data) -> AppResult<Envelope>;
    /// Deletes a record.
    async fn delete(&self, ctx: &Ctx, key: &str) -> AppResult<()>;
    /// Runs an action on the given records.
    async fn run_action(&self, ctx: &Ctx, name: &str, keys: &[String], data: Data) -> AppResult<ActionOutcome>;
    /// Titles of visible records, for relationship labels and options. Invisible keys are absent.
    async fn titles(&self, ctx: &Ctx, keys: &[String]) -> AppResult<BTreeMap<String, String>>;
    /// Which of `keys` the user may reference from another record.
    async fn keys_in_scope(&self, ctx: &Ctx, txn: &DatabaseTransaction, keys: &[String]) -> AppResult<HashSet<String>>;
}

/// All resources of a panel.
#[derive(Default)]
pub struct Registry {
    resources: HashMap<&'static str, Arc<dyn DynResource>>,
}

impl Registry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a resource.
    ///
    /// # Panics
    /// At boot, on a misconfigured resource (unknown column, unsupported type,
    /// composite key, duplicate slug). Misconfiguration must not reach requests.
    pub fn register<R: Resource>(mut self) -> Self {
        let adapter = ResourceAdapter::<R>::new();
        let previous = self.resources.insert(R::SLUG, Arc::new(adapter));
        assert!(previous.is_none(), "resource `{}` registered twice", R::SLUG);
        self
    }

    /// Looks up a resource by slug.
    pub fn get(&self, slug: &str) -> Option<&Arc<dyn DynResource>> {
        self.resources.get(slug)
    }

    /// Checks that every `belongs_to` target is registered.
    ///
    /// # Panics
    /// When a target slug is unknown.
    pub fn finish(self) -> Arc<Self> {
        for resource in self.resources.values() {
            for target in resource.targets() {
                assert!(self.resources.contains_key(target), "unknown belongs_to target `{target}`");
            }
        }
        Arc::new(self)
    }
}
