//! Type-erased resources, looked up by slug.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use async_trait::async_trait;
use rustcms_core::AppResult;
use sea_orm::DatabaseTransaction;

use crate::Data;
use crate::action::ActionOutcome;
use crate::adapter::ResourceAdapter;
pub use crate::adapter::aggregate::Counted;
use crate::context::Ctx;
use crate::links::{LinkOp, LinkOption, LinkOutcome, LinkRequest};
use crate::resource::{Envelope, ListQuery, Page, Relation, Resource, ResourceMeta, Through};
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
    /// Gives the resource the panel's rich text nodes (once, at boot).
    ///
    /// # Panics
    /// When a field accepts a node that is not declared.
    fn attach_rich_nodes(&self, nodes: &Arc<crate::rich_text::AppNodes>);
    /// `(field, collection)` of the fields that name a media collection.
    fn media_collections(&self) -> Vec<(String, String)>;
    /// `belongs_to` slug and wire kind of field `name`, if it is a field.
    fn field_link(&self, name: &str) -> Option<(Option<&'static str>, &'static str)>;
    /// Whether field `name` references media (a media field, or rich text
    /// that may hold images) or is encrypted: what a relationship repeater's
    /// rows may not edit (ADR 0041).
    #[doc(hidden)]
    fn field_locks_media_or_secret(&self, name: &str) -> bool;
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
    /// Every label, help text, placeholder, option and confirmation this
    /// resource can show, whoever looks (for `lang:missing`), with whether it
    /// is looked up in the resource's table first (`true`) or only globally
    /// (`false`, the navigation group).
    fn display_strings(&self) -> Vec<(bool, String)>;
    /// Whether the policy allows `ability` without a record (e.g. `view_any`, `create`).
    async fn allows(&self, ctx: &Ctx, ability: &str) -> bool;
    /// Permissions checked by the default policy (`{slug}.{ability}`), for `permission:sync`.
    fn permissions(&self) -> Vec<String>;
    /// How this resource uses each of [`Self::permissions`]: `Some(ability)`
    /// for `{slug}.{ability}` checked by the default policy, `None` for an
    /// explicit admission permission (`Action::requires_permission`). A name
    /// may appear more than once.
    fn permission_uses(&self) -> Vec<(String, Option<String>)>;
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
    /// The schema of a relationship repeater's row (`components` name this
    /// resource's fields) for `op` (ADR 0040).
    #[doc(hidden)]
    fn row_schema(
        &self,
        ctx: &Ctx,
        components: &[crate::ui::Component],
        op: SchemaOperation,
    ) -> Vec<crate::schema::ComponentSchema>;
    /// The relationship repeaters of the form: relation, child slug, the
    /// child fields the rows edit, the order column (ADR 0040).
    #[doc(hidden)]
    fn row_repeater_fields(&self) -> Vec<(&'static str, &'static str, Vec<String>, Option<String>)>;
    /// Writes a relationship repeater's change to this resource's children
    /// of one parent, in the parent's write `txn` (ADR 0040).
    #[doc(hidden)]
    #[allow(private_interfaces)]
    async fn write_rows(
        &self,
        ctx: &Ctx,
        txn: &Arc<DatabaseTransaction>,
        target: crate::adapter::rows::RowTarget<'_>,
        change: crate::adapter::rows::RowChange,
    ) -> AppResult<crate::adapter::rows::RowsOutcome>;
    /// The computed values record `key`'s view page shows (ADR 0039), for a
    /// user who may view it.
    async fn computed(&self, ctx: &Ctx, key: &str) -> AppResult<crate::schema::ComputedValues>;

    /// The changes the reaction of an input of the create or edit form asks
    /// for (ADR 0041), for a user who may write that form.
    async fn react(
        &self,
        ctx: &Ctx,
        request: crate::schema::FormReactRequest,
    ) -> AppResult<crate::schema::FormReactResponse>;
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
    /// Locks the rows of `keys` (active or trashed) until commit, in key order,
    /// for children declared with [`ResourceMeta::lock_parent`].
    async fn lock_keys(&self, txn: &DatabaseTransaction, keys: &[String]) -> AppResult<()>;
    /// Which of `keys` the user may reference from another record.
    async fn keys_in_scope(&self, ctx: &Ctx, txn: &DatabaseTransaction, keys: &[String]) -> AppResult<HashSet<String>>;
    /// Refuses an import the user may not make (403; 404 when not importable).
    async fn admit_import(&self, ctx: &Ctx) -> AppResult<()>;
    /// What is wrong with a dashboard widget over this resource (static checks).
    fn widget_problem(&self, query: &ListQuery, date: Option<&str>, columns: &[String]) -> Option<String>;
    /// Whether the user may read every field of `names`.
    fn reads_all(&self, ctx: &Ctx, names: &[&str]) -> bool;
    /// The label of field `name` in the user's language.
    fn field_label(&self, ctx: &Ctx, name: &str) -> String;
    /// Counts the rows of `query` the user may list (and per date range).
    async fn counted(
        &self,
        ctx: &Ctx,
        query: &ListQuery,
        date: Option<&str>,
        ranges: &[(chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)],
    ) -> AppResult<Counted>;
    /// How many records the user may list hold each of `parents` in `field`
    /// (count columns); empty when too many must be checked one by one.
    #[doc(hidden)]
    async fn counted_by(&self, ctx: &Ctx, field: &str, parents: &[String]) -> AppResult<BTreeMap<String, u64>>;
    /// Adds the counts of the table's count columns to a page of the list
    /// route (the table only: relation lists and Rust callers go without).
    #[doc(hidden)]
    async fn add_counts(&self, ctx: &Ctx, envelopes: &mut [Envelope]);
    /// Fields the user may set by import.
    fn import_fields(&self, ctx: &Ctx) -> Vec<crate::ImportField>;
    /// Columns of `Field::encrypted*` fields (for `key:reencrypt`).
    fn encrypted_columns(&self) -> Vec<rustcms_core::crypto::EncryptedColumn>;
    /// The import key field, if any.
    fn import_key(&self) -> Option<&str>;
    /// The value of a CSV cell for field `name` (empty: null).
    fn import_value(&self, name: &str, text: &str, quoted: bool) -> serde_json::Value;
    /// A row checked as create (or update) input, before any hook.
    fn check_import_row(&self, ctx: &Ctx, op: crate::Operation, data: Data) -> AppResult<Data>;
    /// Keys (at most two) of visible records whose import key equals `value`.
    async fn import_matches(&self, ctx: &Ctx, value: &serde_json::Value) -> AppResult<Vec<String>>;
    /// Refuses an export the user may not make (403; 404 when not exportable).
    async fn admit_export(&self, ctx: &Ctx) -> AppResult<()>;
    /// Fields exported for the user (readable ones, in order) and whether each is text.
    fn export_columns(&self, ctx: &Ctx) -> Vec<(String, crate::exports::CellKind)>;
    /// `query` checked as a list query, in canonical form (typed filters, no page).
    async fn canonical_query(&self, ctx: &Ctx, query: &ListQuery) -> AppResult<ListQuery>;
    /// Keys the list `query` shows the user, in order; more than `max`: 422.
    async fn export_keys(&self, ctx: &Ctx, query: &ListQuery, max: usize) -> AppResult<Vec<String>>;
    /// Rows of `keys` the user may still see through `query` (key, `columns` only), in order.
    async fn export_rows(
        &self,
        ctx: &Ctx,
        query: &ListQuery,
        keys: &[String],
        columns: &[String],
    ) -> AppResult<Vec<(String, Data)>>;
    /// Refuses running action `name` before its body is read (403; 404 for no such action).
    async fn admit_action(&self, ctx: &Ctx, name: &str) -> AppResult<()>;
    /// Options of the `belongs_to` field `field` of action `name`'s form.
    async fn action_options(
        &self,
        ctx: &Ctx,
        name: &str,
        field: &str,
        search: Option<&str>,
        keys: &[String],
    ) -> AppResult<Vec<SelectOption>>;
    /// The records of `keys` the user may view, locked in key order and
    /// checked again under the lock (targets of link writes).
    async fn lock_visible(&self, ctx: &Ctx, txn: &DatabaseTransaction, keys: &[String]) -> AppResult<HashSet<String>>;
    /// [`Self::lock_visible`], locking the records of `also` (whoever may
    /// view them) in the same key-order pass: those a writing row's foreign
    /// keys reference (ADR 0035).
    async fn lock_visible_with(
        &self,
        ctx: &Ctx,
        txn: &DatabaseTransaction,
        keys: &[String],
        also: &[String],
    ) -> AppResult<HashSet<String>> {
        let mut all: Vec<String> = keys.iter().chain(also).cloned().collect();
        all.sort();
        all.dedup();
        self.lock_keys(txn, &all).await?;
        self.lock_visible(ctx, txn, keys).await
    }
    /// Refuses a write to the belongs-to-many relation `relation` the policy
    /// does not admit (403 before anything is read; 404 for no such relation).
    async fn admit_link(&self, ctx: &Ctx, relation: &str, op: LinkOp) -> AppResult<()>;
    /// The canonical key of the visible record `key` and the condition
    /// selecting the targets of its belongs-to-many relation `relation`.
    async fn link_targets(&self, ctx: &Ctx, relation: &str, key: &str) -> AppResult<(String, Through)>;
    /// Readable pivot columns of the links of record `key` (canonical) to `targets`.
    async fn link_values(
        &self,
        ctx: &Ctx,
        relation: &str,
        key: &str,
        targets: &[String],
    ) -> AppResult<BTreeMap<String, Data>>;
    /// Attaches, detaches or updates links of record `key`.
    async fn link(&self, ctx: &Ctx, relation: &str, key: &str, request: LinkRequest) -> AppResult<LinkOutcome>;
    /// Targets offered for attaching in a create form (no record yet).
    async fn link_options_new(
        &self,
        ctx: &Ctx,
        relation: &str,
        search: Option<&str>,
        keys: &[String],
    ) -> AppResult<Vec<LinkOption>>;
    /// Targets offered for attaching to record `key`.
    async fn link_options(
        &self,
        ctx: &Ctx,
        relation: &str,
        key: &str,
        search: Option<&str>,
        keys: &[String],
    ) -> AppResult<Vec<LinkOption>>;
}

/// All resources of a panel.
#[derive(Default)]
pub struct Registry {
    resources: HashMap<&'static str, Arc<dyn DynResource>>,
    /// Slugs in registration order (the navigation's).
    order: Vec<&'static str>,
    /// Position of each resource in the lock order (ADR 0018, 0020): rows of
    /// a lower position are locked first. Set by [`Registry::finish`].
    lock_order: HashMap<&'static str, usize>,
    /// The application's media hooks ([`crate::AdminProvider::media_hook`]).
    media_hooks: Vec<Arc<dyn crate::media::MediaHook>>,
    /// The application's rich text nodes ([`crate::AdminProvider::rich_text_node`]).
    rich_nodes: crate::rich_text::AppNodes,
    /// The same, shared with the resources (set by [`Registry::finish`]).
    rich_table: Arc<crate::rich_text::AppNodes>,
    /// The media library's collections, in declaration order
    /// ([`crate::AdminProvider::media_collection`]).
    media_collections: Vec<crate::media::MediaCollection>,
}

impl Registry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a hook run on every change to a media record.
    pub fn media_hook(mut self, hook: impl crate::media::MediaHook) -> Self {
        self.media_hooks.push(Arc::new(hook));
        self
    }

    /// Declares a collection of the media library.
    ///
    /// # Panics
    /// On a name other than 1 to 64 of `a-z`, `0-9`, `.`, `_`, `-`, an empty
    /// label, or a name declared twice.
    pub fn media_collection(mut self, name: &str, label: impl Into<String>) -> Self {
        let label = label.into();
        assert!(
            crate::media::valid_collection(name),
            "media collection `{name}`: names are 1 to 64 of a-z, 0-9, `.`, `_`, `-`"
        );
        assert!(!label.trim().is_empty(), "media collection `{name}`: the label is empty");
        assert!(self.media_collection_named(name).is_none(), "media collection `{name}` is declared twice");
        self.media_collections.push(crate::media::MediaCollection { name: name.to_owned(), label });
        self
    }

    /// The media library's collections, in declaration order.
    pub fn media_collections(&self) -> &[crate::media::MediaCollection] {
        &self.media_collections
    }

    /// The declared collection `name`, if any.
    pub fn media_collection_named(&self, name: &str) -> Option<&crate::media::MediaCollection> {
        self.media_collections.iter().find(|c| c.name == name)
    }

    /// Declares a rich text node (see [`crate::rich_text::RichTextNode`]).
    ///
    /// # Panics
    /// On an invalid declaration or a name declared twice.
    pub fn rich_text_node(mut self, node: crate::rich_text::RichTextNode) -> Self {
        self.rich_nodes.add(node);
        self
    }

    /// The panel's rich text nodes (after [`Registry::finish`]): parse and
    /// render documents with the declarations the panel checks them against.
    pub fn rich_nodes(&self) -> &Arc<crate::rich_text::AppNodes> {
        &self.rich_table
    }

    /// The media hooks, in registration order.
    pub fn media_hooks(&self) -> &[Arc<dyn crate::media::MediaHook>] {
        &self.media_hooks
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
        self.order.push(R::SLUG);
        self
    }

    /// Every resource, in the order of their slugs.
    pub fn all(&self) -> Vec<&Arc<dyn DynResource>> {
        let mut all: Vec<_> = self.resources.values().collect();
        all.sort_by_key(|r| r.slug());
        all
    }

    /// Every resource, in registration order (the panel's navigation).
    pub fn in_registration_order(&self) -> Vec<&Arc<dyn DynResource>> {
        self.order.iter().filter_map(|slug| self.resources.get(slug)).collect()
    }

    /// Looks up a resource by slug.
    pub fn get(&self, slug: &str) -> Option<&Arc<dyn DynResource>> {
        self.resources.get(slug)
    }

    /// Checks that every `belongs_to` target is registered.
    ///
    /// # Panics
    /// When a target slug is unknown.
    pub fn finish(mut self) -> Arc<Self> {
        let nodes = Arc::new(std::mem::take(&mut self.rich_nodes));
        for resource in self.resources.values() {
            resource.attach_rich_nodes(&nodes);
        }
        self.rich_table = nodes;
        // Pivot tables already used by a relation (one relation per pivot).
        let mut pivots = HashMap::new();
        for resource in self.resources.values() {
            for (field, collection) in resource.media_collections() {
                assert!(
                    self.media_collection_named(&collection).is_some(),
                    "resource `{}`: field `{field}` uses the media collection `{collection}`, which is not \
                     declared (`AdminProvider::media_collection`)",
                    resource.slug()
                );
            }
            for target in resource.targets() {
                assert!(self.resources.contains_key(target), "unknown belongs_to target `{target}`");
            }
            let slug = resource.slug();
            if let Some((field, target)) = &resource.meta().parent_lock {
                let parent = self
                    .resources
                    .get(target)
                    .unwrap_or_else(|| panic!("resource `{slug}`: lock_parent targets unknown `{target}`"));
                assert!(*target != slug, "resource `{slug}`: lock_parent cannot target itself");
                // Media rows are locked last by every write (ADR 0041).
                assert!(
                    *target != crate::media::SLUG,
                    "resource `{slug}`: lock_parent cannot target the media library (media rows are locked last)"
                );
                assert!(
                    matches!(resource.field_link(field), Some((Some(linked), _)) if linked == *target),
                    "resource `{slug}`: lock_parent needs `{field}` to be a field with `belongs_to(\"{target}\")`"
                );
                assert!(
                    parent.meta().parent_lock.is_none(),
                    "resource `{slug}`: lock_parent target `{target}` locks a parent of its own (no chains)"
                );
            }
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
                if let Some(pivot) = &relation.pivot {
                    check_pivot(resource.as_ref(), target.as_ref(), relation, pivot.as_ref(), &mut pivots);
                    continue;
                }
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
        // Relationship repeaters edit fields of their child (ADR 0040).
        for resource in self.resources.values() {
            let slug = resource.slug();
            for (name, target, inputs, order) in resource.row_repeater_fields() {
                let child = &self.resources[target];
                let here = format!("resource `{slug}`: repeater `{name}`");
                // The family serializes on the parent row: every write of a child
                // locks its parent first, through this relation (ADR 0041).
                let relation = resource.relations().iter().find(|r| r.name == name).map(|r| r.field);
                match &child.meta().parent_lock {
                    Some((field, locked)) => assert!(
                        *locked == slug && relation == Some(field.as_str()),
                        "{here}: `{target}` locks its parent `{locked}` through `{field}`, not through this relation"
                    ),
                    None => panic!(
                        "{here}: `{target}` must lock its parent: `ResourceMeta::lock_parent(\"{}\", \"{slug}\")`",
                        relation.unwrap_or_default()
                    ),
                }
                // One locking phase per child resource.
                assert!(
                    resource.row_repeater_fields().iter().filter(|(_, t, _, _)| *t == target).count() == 1,
                    "{here}: `{target}` is edited by another repeater of `{slug}` (one repeater per child resource)"
                );
                // The parent's media locks come last; linking media would lock them earlier.
                assert!(
                    !resource.relations().iter().any(|r| r.is_many() && r.target == crate::media::SLUG),
                    "{here}: a resource with relationship repeaters cannot link media records (belongs-to-many)"
                );
                for input in &inputs {
                    // A row takes no explicit lock but its own: no references edited in rows.
                    match child.field_link(input) {
                        None => panic!("{here}: row input `{input}` is not a field of `{target}`"),
                        Some((Some(_), _)) => panic!("{here}: row input `{input}` is a belongs_to field (not in rows)"),
                        Some((None, _)) => assert!(
                            !child.field_locks_media_or_secret(input),
                            "{here}: row input `{input}` is a media, rich-text or encrypted field (not in rows)"
                        ),
                    }
                }
                if let Some(column) = order {
                    assert!(
                        matches!(child.field_link(&column), Some((None, "i32" | "i64"))),
                        "resource `{slug}`: repeater `{name}`: the order column `{column}` must be an integer field of \
                         `{target}`"
                    );
                }
            }
        }
        let mut registry = self;
        registry.lock_order = registry.check_lock_order();
        Arc::new(registry)
    }

    /// The order in which a write that locks rows of several resources (the
    /// targets of a record form's links, ADR 0035) locks them: parents before
    /// their targets and `lock_parent` children, as every other writer does
    /// (a topological order of the lock graph, the same on every instance);
    /// resources outside the graph last, by slug.
    #[doc(hidden)]
    pub fn lock_rank(&self, slug: &str) -> (usize, String) {
        // Media rows are locked last by every write (references are checked
        // after every other lock, ADR 0041): a rank after all others.
        if slug == crate::media::SLUG {
            return (usize::MAX, char::MAX.to_string());
        }
        (self.lock_order.get(slug).copied().unwrap_or(usize::MAX), slug.to_owned())
    }
}

impl Registry {
    /// One lock order for every writer (ADR 0018, 0020): a parent before its
    /// `lock_parent` children, a belongs-to-many parent before its targets.
    /// A cycle among these would let two writes wait on each other.
    ///
    /// # Panics
    /// On a cycle.
    fn check_lock_order(&self) -> HashMap<&'static str, usize> {
        let mut edges: HashMap<&'static str, Vec<&'static str>> = HashMap::new();
        for resource in self.resources.values() {
            if let Some((_, parent)) = &resource.meta().parent_lock {
                edges.entry(*parent).or_default().push(resource.slug());
            }
            for relation in resource.relations().iter().filter(|r| r.is_many()) {
                edges.entry(resource.slug()).or_default().push(relation.target);
            }
            // A parent write with relationship repeaters locks its link targets
            // (before its row) before the rows of its children (after it).
            for (_, child, _, _) in resource.row_repeater_fields() {
                for relation in resource.relations().iter().filter(|r| r.is_many()) {
                    edges.entry(relation.target).or_default().push(child);
                }
            }
        }
        // The same order on every instance, whatever the maps' iteration order.
        for targets in edges.values_mut() {
            targets.sort_unstable();
            targets.dedup();
        }
        // Depth-first search; `on_path` holds the current path.
        fn visit<'a>(
            node: &'a str,
            edges: &HashMap<&'a str, Vec<&'a str>>,
            done: &mut HashSet<&'a str>,
            on_path: &mut Vec<&'a str>,
            finished: &mut Vec<&'a str>,
        ) {
            if done.contains(node) {
                return;
            }
            if let Some(at) = on_path.iter().position(|n| *n == node) {
                let cycle = on_path[at..].join(" → ");
                panic!("lock order cycle: {cycle} → {node} (belongs-to-many targets and `lock_parent` parents)");
            }
            on_path.push(node);
            for next in edges.get(node).into_iter().flatten() {
                visit(next, edges, done, on_path, finished);
            }
            on_path.pop();
            done.insert(node);
            finished.push(node);
        }
        let mut done = HashSet::new();
        let mut finished = Vec::new();
        let mut slugs: Vec<&str> = edges.keys().copied().collect();
        slugs.sort_unstable();
        for slug in slugs {
            visit(slug, &edges, &mut done, &mut Vec::new(), &mut finished);
        }
        // A node finishes after everything it locks before: reversed, parents come first.
        finished.into_iter().rev().enumerate().map(|(rank, slug)| (slug, rank)).collect()
    }
}

/// Boot checks of a belongs-to-many relation of `parent` (see ADR 0020: one
/// lock order, parent then targets, so configurations needing another are refused).
fn check_pivot(
    parent: &dyn DynResource,
    target: &dyn DynResource,
    relation: &Relation,
    pivot: &dyn crate::adapter::DynPivot,
    pivots: &mut HashMap<String, String>,
) {
    let slug = parent.slug();
    let name = relation.name;
    let table = pivot.table();
    assert!(
        target.slug() != slug,
        "resource `{slug}`: relation `{name}`: belongs-to-many to the same resource is not supported"
    );
    assert!(
        target.meta().parent_lock.is_none(),
        "resource `{slug}`: relation `{name}`: the target `{}` declares `lock_parent`, not supported with belongs-to-many",
        target.slug()
    );
    if let Some(other) = pivots.insert(table.to_owned(), format!("{slug}.{name}")) {
        panic!(
            "resource `{slug}`: relation `{name}`: pivot `{table}` is already used by `{other}` (one relation per pivot)"
        );
    }
    let kind = |column: &str| pivot.column(column).map(|c| c.kind.name());
    assert!(
        kind(pivot.parent_column()) == Some(parent.key_kind()),
        "resource `{slug}`: relation `{name}`: `{table}.{}` must have the type of `{slug}`'s key",
        pivot.parent_column()
    );
    assert!(
        kind(pivot.target_column()) == Some(target.key_kind()),
        "resource `{slug}`: relation `{name}`: `{table}.{}` must have the type of `{}`'s key",
        pivot.target_column(),
        target.slug()
    );
    let keys = [pivot.parent_column(), pivot.target_column()];
    let mut declared = HashSet::new();
    for field in &relation.pivot_fields {
        assert!(
            pivot.column(&field.name).is_some()
                && !keys.contains(&field.name.as_str())
                && declared.insert(field.name.as_str()),
            "resource `{slug}`: relation `{name}`: pivot field `{}` is unknown, a key column or declared twice",
            field.name
        );
        assert!(
            field.belongs_to.is_none() && field.media.is_none() && !field.rich_text && field.secret.is_none(),
            "resource `{slug}`: relation `{name}`: pivot field `{}` may not be a relationship, media, rich text \
             or encrypted",
            field.name
        );
    }
    // Inserts set the two keys and the declared fields: any other column must
    // be optional; a mandatory one is declared `.required()` (attaching then
    // needs it, see the engine).
    for column in pivot.column_names() {
        let info = pivot.column(&column).expect("listed");
        let required = relation.pivot_fields.iter().any(|f| f.name == column && f.required);
        assert!(
            keys.contains(&column.as_str()) || info.nullable || info.has_default || required,
            "resource `{slug}`: relation `{name}`: pivot column `{table}.{column}` needs to be nullable, have a \
             default or be a `.required()` pivot field"
        );
    }
}
