//! The `Resource` trait implemented by applications, and its wire types.

pub use super::query::*;
use std::sync::Arc;

use async_trait::async_trait;
use rustcms_core::AppResult;
use sea_orm::sea_query::Value;
use sea_orm::{ActiveModelBehavior, DatabaseTransaction, EntityTrait, IntoActiveModel, Select};

use crate::Data;
use crate::action::Action;
use crate::context::Ctx;
use crate::field::Field;
use crate::policy::{PermissionPolicy, Policy};
use crate::ui::{Form, Table};

/// SeaORM bounds the generic CRUD engine needs, written once.
pub trait ResourceEntity:
    EntityTrait<Model: IntoActiveModel<Self::ActiveModel> + Sync, ActiveModel: ActiveModelBehavior + Send + Sync>
{
}

impl<E> ResourceEntity for E
where
    E: EntityTrait,
    E::Model: IntoActiveModel<E::ActiveModel> + Sync,
    E::ActiveModel: ActiveModelBehavior + Send + Sync,
{
}

/// How the primary key of new records is produced.
#[derive(Clone, Copy)]
pub enum KeyStrategy {
    /// The database generates it (auto-increment, column default). Never written.
    DbGenerated,
    /// Server code generates it before insert, e.g. `|| Value::Uuid(Some(Uuid::now_v7()))`.
    AppGenerated(fn() -> Value),
    /// Natural key supplied by the client on create (if its field is creatable). Never updated.
    Natural,
}

/// A database constraint mapped to a user-facing validation error.
#[derive(Debug, Clone)]
pub struct ConstraintMap {
    pub(crate) name: String,
    pub(crate) columns: Vec<String>,
    pub(crate) field: Option<String>,
    pub(crate) message: String,
}

/// Static description of a resource.
#[derive(Clone)]
pub struct ResourceMeta {
    pub(crate) label: String,
    pub(crate) plural_label: Option<String>,
    pub(crate) icon: Option<String>,
    pub(crate) group: Option<String>,
    pub(crate) sort: i32,
    pub(crate) navigation: bool,
    pub(crate) key: KeyStrategy,
    /// The title template (`{field}` placeholders), parsed and checked at boot.
    pub(crate) title: Option<String>,
    pub(crate) constraints: Vec<ConstraintMap>,
    pub(crate) soft_delete: Option<String>,
    pub(crate) audit: bool,
    pub(crate) parent_lock: Option<(String, &'static str)>,
    pub(crate) exportable: bool,
    pub(crate) importable: bool,
    pub(crate) import_key: Option<String>,
}

impl ResourceMeta {
    /// Creates metadata with a human label and a database-generated key.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            plural_label: None,
            icon: None,
            group: None,
            sort: 0,
            navigation: true,
            key: KeyStrategy::DbGenerated,
            title: None,
            constraints: Vec::new(),
            soft_delete: None,
            audit: true,
            parent_lock: None,
            exportable: false,
            importable: false,
            import_key: None,
        }
    }

    /// Records can be imported from CSV files (each row created through the
    /// usual create path: field rules, hooks, policy), with the ability
    /// `import` (`{slug}.import` by default) on top of `create`.
    pub fn importable(mut self) -> Self {
        self.importable = true;
        self
    }

    /// Imported rows whose `field` matches a record the user may see update
    /// it instead of creating one (also needs `update`). `field` must have a
    /// [`ResourceMeta::unique`] constraint of its own (checked at boot).
    pub fn import_key(mut self, field: &str) -> Self {
        self.importable = true;
        self.import_key = Some(field.to_owned());
        self
    }

    /// Records can be exported as CSV (the list as shown: scope, filters,
    /// search, sort; readable fields only), with the ability `export`
    /// (`{slug}.export` by default) on top of listing them.
    pub fn exportable(mut self) -> Self {
        self.exportable = true;
        self
    }

    /// Every write to a record of this resource first locks its parent: the
    /// record of `target` that the `belongs_to` field `field` points to (both
    /// parents when a write moves the record). Writes under one parent then
    /// run one at a time, in the same order as writes to the parent itself
    /// (parent, then child), so a hook can check a rule over the parent's
    /// children (a course's seats) and ownership read through the parent
    /// (its instructor) cannot change during the write.
    ///
    /// At most one per resource; `target` must not lock a parent of its own.
    pub fn lock_parent(mut self, field: impl Into<String>, target: &'static str) -> Self {
        self.parent_lock = Some((field.into(), target));
        self
    }

    /// Whether writes to this resource are recorded in the audit log (on by default).
    pub fn audit(mut self, enabled: bool) -> Self {
        self.audit = enabled;
        self
    }

    /// Sets the primary key strategy.
    pub fn key(mut self, key: KeyStrategy) -> Self {
        self.key = key;
        self
    }

    /// Field used as the record title (labels, relationship options):
    /// `title("name")` is `title_template("{name}")`.
    pub fn title(mut self, field: impl Into<String>) -> Self {
        self.title = Some(format!("{{{}}}", field.into()));
        self
    }

    /// The record title from several fields, for records that share a name:
    /// `title_template("{name} (grade {grade})")` shows "Math (grade 10)" in
    /// headings, breadcrumbs, relationship selects (searched over every field
    /// of the template, each word of the search matching one of them) and
    /// labels. A `null` field shows as empty. A user who
    /// may not read one of the fields sees no title (the record's key). The
    /// fields are checked at boot.
    pub fn title_template(mut self, template: impl Into<String>) -> Self {
        self.title = Some(template.into());
        self
    }

    /// Maps a unique constraint to a field error.
    ///
    /// `name` is the constraint name (PostgreSQL); `columns` identify it on SQLite.
    /// The error goes to `columns[0]`, or to form errors for composite keys.
    pub fn unique(mut self, name: &str, columns: &[&str], message: &str) -> Self {
        let field = (columns.len() == 1).then(|| columns[0].to_owned());
        self.constraints.push(ConstraintMap {
            name: name.to_owned(),
            columns: columns.iter().map(|c| (*c).to_owned()).collect(),
            field,
            message: message.to_owned(),
        });
        self
    }

    /// Soft deletes: deleting sets `deleted_at` (a nullable timestamp column),
    /// trashed records are hidden everywhere, and `restore` / `force_delete`
    /// bring them back or remove them for good.
    pub fn soft_deletes(self) -> Self {
        self.soft_deletes_column("deleted_at")
    }

    /// Soft deletes with the marker column `column`.
    pub fn soft_deletes_column(mut self, column: impl Into<String>) -> Self {
        self.soft_delete = Some(column.into());
        self
    }

    /// Plural label (default: label + "s").
    pub fn plural(mut self, label: impl Into<String>) -> Self {
        self.plural_label = Some(label.into());
        self
    }

    /// Navigation icon.
    pub fn icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    /// Navigation group.
    pub fn group(mut self, group: impl Into<String>) -> Self {
        self.group = Some(group.into());
        self
    }

    /// Position in the navigation: entries by `(sort, base order)`, groups by
    /// their smallest `sort` (default 0).
    pub fn sort(mut self, sort: i32) -> Self {
        self.sort = sort;
        self
    }

    /// `false`: no navigation entry and no dashboard card. Routes, record
    /// pages and the API stay, authorized as always (UX only).
    pub fn navigation(mut self, shown: bool) -> Self {
        self.navigation = shown;
        self
    }

    /// Human label.
    pub fn label(&self) -> &str {
        &self.label
    }
}

/// An admin resource over one SeaORM entity, in the spirit of Filament resources.
///
/// Hooks receive the open transaction; everything they write commits or rolls
/// back with the record.
#[async_trait]
pub trait Resource: Send + Sync + 'static {
    /// Entity managed by this resource.
    type Entity: ResourceEntity;

    /// URL segment and permission prefix (`posts` → `posts.update`).
    const SLUG: &'static str;

    /// Static metadata.
    fn meta() -> ResourceMeta;

    /// Exposed fields. Columns not listed here are neither readable nor writable by clients.
    fn fields() -> Vec<Field>;

    /// Form layout. Empty (the default): one input per field, by column type.
    fn form() -> Form {
        Form::default()
    }

    /// The view page's entries (ADR 0039). Empty (the default): entries made
    /// from the form, each value shown as the form's read-only view shows it.
    /// Declared: it replaces the form there.
    fn infolist() -> crate::ui::Infolist {
        crate::ui::Infolist::default()
    }

    /// Values computed when a record's view page opens, shown by
    /// `ComputedEntry`s of the same names (ADR 0039).
    fn computed() -> Vec<crate::ui::Computed<Self::Entity>> {
        Vec::new()
    }

    /// Table layout. Empty (the default): the first readable fields.
    fn table() -> Table {
        Table::default()
    }

    /// Authorization policy. Defaults to [`PermissionPolicy`] (deny by default).
    fn policy() -> Arc<dyn Policy> {
        Arc::new(PermissionPolicy::new(Self::SLUG))
    }

    /// Restricts which records the user may see at all (tenancy, ownership).
    /// Applied to lists, lookups, writes, actions and relationship options.
    fn scope(query: Select<Self::Entity>, _ctx: &Ctx) -> Select<Self::Entity> {
        query
    }

    /// Row and bulk actions.
    fn actions() -> Vec<Action<Self::Entity>> {
        Vec::new()
    }

    /// Adjusts validated input before insert. May set guarded columns.
    async fn mutate_before_create(_ctx: &Ctx, _txn: &DatabaseTransaction, data: Data) -> AppResult<Data> {
        Ok(data)
    }

    /// Adjusts validated input before update. May set guarded columns.
    async fn mutate_before_update(
        _ctx: &Ctx,
        _txn: &DatabaseTransaction,
        _record: &<Self::Entity as EntityTrait>::Model,
        data: Data,
    ) -> AppResult<Data> {
        Ok(data)
    }

    /// Has-many relations shown on a record's page.
    fn relations() -> Vec<Relation> {
        Vec::new()
    }

    /// Runs after insert or update, in the same transaction.
    async fn after_save(
        _ctx: &Ctx,
        _txn: &DatabaseTransaction,
        _record: &<Self::Entity as EntityTrait>::Model,
    ) -> AppResult<()> {
        Ok(())
    }

    /// Runs in the transaction that deletes a record (resources without soft
    /// deletes), after the delete and before the commit, with the record as it
    /// was: e.g. dispatch a job (outbox) that removes what the record
    /// configured. An error rolls the delete back. Trashing a soft-deleting
    /// resource's record does not call it; see [`Self::after_force_delete`].
    async fn after_delete(
        _ctx: &Ctx,
        _txn: &DatabaseTransaction,
        _record: &<Self::Entity as EntityTrait>::Model,
    ) -> AppResult<()> {
        Ok(())
    }

    /// Runs in the transaction that moves a record to the trash (soft
    /// deletes), after the update, with the record as stored (its marker
    /// set): e.g. unpublish what the record put online. An error rolls the
    /// trashing back. Not called by hard deletes ([`Self::after_delete`]).
    async fn after_trash(
        _ctx: &Ctx,
        _txn: &DatabaseTransaction,
        _record: &<Self::Entity as EntityTrait>::Model,
    ) -> AppResult<()> {
        Ok(())
    }

    /// Runs in the transaction that restores a trashed record (soft deletes),
    /// after the update, with the record as stored (its marker cleared). An
    /// error rolls the restore back.
    ///
    /// By default it calls [`Self::after_save`], as restores did before this
    /// hook existed; an override replaces that call (call `Self::after_save`
    /// from it to keep both).
    async fn after_restore(
        ctx: &Ctx,
        txn: &DatabaseTransaction,
        record: &<Self::Entity as EntityTrait>::Model,
    ) -> AppResult<()> {
        Self::after_save(ctx, txn, record).await
    }

    /// Runs in the transaction of a write that changed links of a
    /// belongs-to-many relation of `record` (the parent, as stored): an
    /// attach, a detach or a pivot update from the relation manager, or the
    /// links chosen in the record's own form. Once per relation and write,
    /// after the pivot rows are written; an error rolls the whole write back.
    /// The parent row is locked but not updated: touch its `updated_at` here
    /// if links count as a change of the record.
    async fn after_relation_change(
        _ctx: &Ctx,
        _txn: &DatabaseTransaction,
        _record: &<Self::Entity as EntityTrait>::Model,
        _change: &crate::links::RelationChange,
    ) -> AppResult<()> {
        Ok(())
    }

    /// Runs in the transaction that deletes a trashed record for good (soft
    /// deletes), after the delete, with the record as it was: e.g. queue the
    /// removal of files the record owned. An error rolls the delete back.
    async fn after_force_delete(
        _ctx: &Ctx,
        _txn: &DatabaseTransaction,
        _record: &<Self::Entity as EntityTrait>::Model,
    ) -> AppResult<()> {
        Ok(())
    }
}

/// Records of another resource related to this one, shown on a record's page:
/// a has-many relation (the target points here), or a belongs-to-many
/// relation through a pivot table.
#[derive(Clone)]
pub struct Relation {
    pub(crate) name: &'static str,
    pub(crate) target: &'static str,
    /// Has-many: the target's field holding the parent key. Belongs-to-many:
    /// the pivot's parent column.
    pub(crate) field: &'static str,
    pub(crate) label: Option<String>,
    /// Belongs-to-many: the pivot table, and its columns users see or write.
    pub(crate) pivot: Option<Arc<dyn crate::adapter::DynPivot>>,
    pub(crate) pivot_fields: Vec<Field>,
    /// Has-many: the target's records the relation holds (`where_null`,
    /// `where_eq`, all of them; ADR 0044).
    pub(crate) filters: Vec<(&'static str, RelationFilter)>,
}

/// What a filtered has-many relation requires of a target field (ADR 0044).
#[derive(Debug, Clone, PartialEq)]
pub enum RelationFilter {
    /// No value (`IS NULL`).
    Null,
    /// This value (in wire form, as the field's kind reads it).
    Eq(serde_json::Value),
}

impl RelationFilter {
    /// The value records of the relation hold (`null` for [`Self::Null`]).
    pub(crate) fn value(&self) -> serde_json::Value {
        match self {
            Self::Null => serde_json::Value::Null,
            Self::Eq(value) => value.clone(),
        }
    }
}

impl std::fmt::Debug for Relation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Relation")
            .field("name", &self.name)
            .field("target", &self.target)
            .field("field", &self.field)
            .field("pivot", &self.pivot.as_ref().map(|p| p.table().to_owned()))
            .field("filters", &self.filters)
            .finish_non_exhaustive()
    }
}

impl Relation {
    /// `name` (URL segment): the records of `target` whose `field` (a
    /// `belongs_to` this resource) holds the record's key.
    pub fn has_many(name: &'static str, target: &'static str, field: &'static str) -> Self {
        Self { name, target, field, label: None, pivot: None, pivot_fields: Vec::new(), filters: Vec::new() }
    }

    /// `name` (URL segment): the records of `target` linked to the record by
    /// rows of a pivot table, whose column `parent` holds this resource's key
    /// and `related` the target's. The pivot needs a unique key over both
    /// columns and foreign keys deleting its rows with either record.
    ///
    /// ```ignore
    /// Relation::belongs_to_many("tags", "tags", post_tag::Column::PostId, post_tag::Column::TagSlug)
    /// ```
    ///
    /// # Panics
    /// When either column is not supported, or both are the same.
    pub fn belongs_to_many<C>(name: &'static str, target: &'static str, parent: C, related: C) -> Self
    where
        C: sea_orm::ColumnTrait,
        C::EntityName: EntityTrait<Column = C>,
        <C::EntityName as EntityTrait>::Model: IntoActiveModel<<C::EntityName as EntityTrait>::ActiveModel> + Sync,
        <C::EntityName as EntityTrait>::ActiveModel: Send,
    {
        let pivot = crate::adapter::PivotAdapter::<C::EntityName>::new(name, parent.as_str(), related.as_str());
        Self {
            name,
            target,
            field: parent.as_str(),
            label: None,
            pivot: Some(Arc::new(pivot)),
            pivot_fields: Vec::new(),
            filters: Vec::new(),
        }
    }

    /// Columns of the pivot shown and edited with each link (belongs-to-many),
    /// with the usual field rules (`readable`, `creatable`, `updatable`, …).
    /// Other pivot columns never leave the server.
    pub fn pivot(mut self, fields: Vec<Field>) -> Self {
        self.pivot_fields = fields;
        self
    }

    /// Section title (default: the name, humanized).
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Has-many: only the target's records whose `field` holds no value
    /// (ADR 0044). Wherever the relation is read (its section, counts,
    /// repeaters, view pages); records created through it get `null` there.
    /// Several repeaters may edit one target through relations that cannot
    /// share a record:
    ///
    /// ```ignore
    /// Relation::has_many("options", "question_options", "question_id").where_null("blank_id")
    /// ```
    ///
    /// # Panics
    /// On a belongs-to-many relation (the field is checked at boot).
    pub fn where_null(self, field: &'static str) -> Self {
        self.filtered(field, RelationFilter::Null)
    }

    /// Has-many: only the target's records whose `field` holds `value` (in
    /// wire form, as the API sends the field: `7` for an `i32`, `"7"` for an
    /// `i64`, `"1.5"` or `"1.50"` for a decimal); records
    /// created through the relation get it. See [`Self::where_null`].
    ///
    /// # Panics
    /// On a belongs-to-many relation (the field and value are checked at boot).
    pub fn where_eq(self, field: &'static str, value: impl Into<serde_json::Value>) -> Self {
        self.filtered(field, RelationFilter::Eq(value.into()))
    }

    fn filtered(mut self, field: &'static str, filter: RelationFilter) -> Self {
        assert!(
            self.pivot.is_none(),
            "relation `{}`: `where_null` and `where_eq` are for has-many relations",
            self.name
        );
        assert!(
            !self.filters.iter().any(|(f, _)| *f == field),
            "relation `{}`: `{field}` is filtered twice",
            self.name
        );
        self.filters.push((field, filter));
        self
    }

    /// The filters as a list query takes them.
    pub(crate) fn query_filters(&self) -> Vec<(String, RelationFilter)> {
        self.filters.iter().map(|(f, v)| ((*f).to_owned(), v.clone())).collect()
    }

    /// Whether no record can belong to both relations (some field filtered
    /// apart: null against a value, or two values), the values compared as
    /// `canonical` reads them for a field (`"1.5"` and `"1.50"` are one value
    /// of a decimal field).
    pub(crate) fn disjoint(
        &self,
        other: &Relation,
        canonical: &dyn Fn(&str, &RelationFilter) -> serde_json::Value,
    ) -> bool {
        self.filters.iter().any(|(field, filter)| {
            other.filters.iter().any(|(f, o)| f == field && canonical(field, filter) != canonical(field, o))
        })
    }

    /// Whether this is a belongs-to-many relation.
    pub(crate) fn is_many(&self) -> bool {
        self.pivot.is_some()
    }
}
