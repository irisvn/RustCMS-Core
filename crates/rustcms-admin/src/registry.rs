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
use crate::resource::{Envelope, ListQuery, Page, Relation, Resource, ResourceMeta};
use crate::schema::{ResourceInfo, ResourceSchema, SchemaOperation};
use crate::ui::SelectOption;

/// Object-safe view of a [`Resource`], used by the non-generic HTTP layer.
#[async_trait]
pub trait DynResource: Send + Sync {
    /// URL slug.
    fn slug(&self) -> &'static str;
    /// Static metadata.
    fn meta(&self) -> &ResourceMeta;
    /// Slugs referenced by `belongs_to` fields.
    fn targets(&self) -> Vec<&'static str>;
    /// Has-many relations.
    fn relations(&self) -> &[Relation];
    /// `belongs_to` slug and wire kind of field `name`, if it is a field.
    fn field_link(&self, name: &str) -> Option<(Option<&'static str>, &'static str)>;
    /// Wire kind of the primary key.
    fn key_kind(&self) -> &'static str;
    /// `key` in canonical form (`01` → `1`), if it is a valid key.
    fn canonical_key(&self, key: &str) -> Option<String>;
    /// Whether field `name` is readable, and writable on create, by the user.
    fn field_access(&self, ctx: &Ctx, name: &str) -> (bool, bool);
    /// Name of the primary key column.
    fn key_name(&self) -> &str;
    /// [`Policy::admits`](crate::Policy::admits).
    async fn admits(&self, ctx: &Ctx, ability: &str) -> bool;
    /// The canonical key of record `key` if the user may view it; 404 otherwise
    /// (missing, outside scope, denied or trashed alike).
    async fn visible_key(&self, ctx: &Ctx, key: &str) -> AppResult<String>;
    /// The committed result of an idempotent create retried with `ctx`'s key
    /// (see [`Ctx::with_idempotency_key`]), as the user sees it now.
    async fn replay_create(&self, ctx: &Ctx, input: &Data) -> AppResult<Option<Envelope>>;
    /// Identity and navigation entry.
    fn info(&self) -> ResourceInfo;
    /// Whether the policy allows `ability` without a record (e.g. `view_any`, `create`).
    async fn allows(&self, ctx: &Ctx, ability: &str) -> bool;
    /// Permissions checked by the default policy (`{slug}.{ability}`), for `permission:sync`.
    fn permissions(&self) -> Vec<String>;
    /// Form and table for the current user; `key` selects a record for `edit`/`view`.
    async fn schema(&self, ctx: &Ctx, op: SchemaOperation, key: Option<&str>) -> AppResult<ResourceSchema>;
    /// `(key, title)` of visible records, by `keys` or matching `search` (at most 50).
    async fn lookup(&self, ctx: &Ctx, search: Option<&str>, keys: &[String]) -> AppResult<Vec<(String, String)>>;
    /// Options of the `belongs_to` field `field`, from the target resource;
    /// `record` is the key of the record being edited or viewed, if any.
    async fn options(
        &self,
        ctx: &Ctx,
        field: &str,
        search: Option<&str>,
        keys: &[String],
        record: Option<&str>,
    ) -> AppResult<Vec<SelectOption>>;
    /// The checks of `list` that do not depend on any record (abilities, paging,
    /// `trashed`, a readable parent field): run before a relation's parent is
    /// looked up, so its answer never depends on the parent key.
    async fn admit_list(&self, ctx: &Ctx, query: &ListQuery) -> AppResult<()>;
    /// Lists records in scope.
    async fn list(&self, ctx: &Ctx, query: &ListQuery) -> AppResult<Page>;
    /// Reads one record.
    async fn find(&self, ctx: &Ctx, key: &str) -> AppResult<Envelope>;
    /// Creates a record from client input.
    async fn create(&self, ctx: &Ctx, input: Data) -> AppResult<Envelope>;
    /// Updates a record from client input (PATCH semantics).
    async fn update(&self, ctx: &Ctx, key: &str, input: Data) -> AppResult<Envelope>;
    /// Deletes a record (moves it to the trash with soft deletes).
    async fn delete(&self, ctx: &Ctx, key: &str) -> AppResult<()>;
    /// Restores a trashed record (soft deletes; 404 otherwise).
    async fn restore(&self, ctx: &Ctx, key: &str) -> AppResult<Envelope>;
    /// Deletes a trashed record for good (soft deletes; 404 otherwise).
    async fn force_delete(&self, ctx: &Ctx, key: &str) -> AppResult<()>;
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

    /// Every resource, in registration order of slugs.
    pub fn all(&self) -> Vec<&Arc<dyn DynResource>> {
        let mut all: Vec<_> = self.resources.values().collect();
        all.sort_by_key(|r| r.slug());
        all
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
            let slug = resource.slug();
            let mut names = HashSet::new();
            for relation in resource.relations() {
                assert!(
                    !relation.name.is_empty()
                        && relation.name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
                        && names.insert(relation.name),
                    "resource `{slug}`: relation `{}` is not a unique snake_case name",
                    relation.name
                );
                let target = self.resources.get(relation.target).unwrap_or_else(|| {
                    panic!("resource `{slug}`: relation `{}` targets unknown `{}`", relation.name, relation.target)
                });
                let link = target.field_link(relation.field);
                assert!(
                    matches!(link, Some((Some(parent), _)) if parent == slug),
                    "resource `{slug}`: relation `{}` needs `{}.{}` to be a field with `belongs_to(\"{slug}\")`",
                    relation.name,
                    relation.target,
                    relation.field
                );
                assert!(
                    link.is_some_and(|(_, kind)| kind == resource.key_kind()) && relation.field != target.key_name(),
                    "resource `{slug}`: relation `{}`: `{}.{}` must have the type of `{slug}`'s key and not be its key",
                    relation.name,
                    relation.target,
                    relation.field
                );
            }
        }
        Arc::new(self)
    }
}
