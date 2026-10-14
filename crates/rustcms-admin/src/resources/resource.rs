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
    pub(crate) key: KeyStrategy,
    pub(crate) title_field: Option<String>,
    pub(crate) constraints: Vec<ConstraintMap>,
    pub(crate) soft_delete: Option<String>,
    pub(crate) audit: bool,
}

impl ResourceMeta {
    /// Creates metadata with a human label and a database-generated key.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            plural_label: None,
            icon: None,
            group: None,
            key: KeyStrategy::DbGenerated,
            title_field: None,
            constraints: Vec::new(),
            soft_delete: None,
            audit: true,
        }
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

    /// Field used as the record title (labels, relationship options).
    pub fn title(mut self, field: impl Into<String>) -> Self {
        self.title_field = Some(field.into());
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

    /// Runs in the transaction that deletes a trashed record for good (soft
    /// deletes): e.g. queue the removal of files the record owned.
    async fn after_force_delete(
        _ctx: &Ctx,
        _txn: &DatabaseTransaction,
        _record: &<Self::Entity as EntityTrait>::Model,
    ) -> AppResult<()> {
        Ok(())
    }
}

/// Records of another resource that point to this one (a has-many relation):
/// listed and created from a record's page.
#[derive(Debug, Clone)]
pub struct Relation {
    pub(crate) name: &'static str,
    pub(crate) target: &'static str,
    pub(crate) field: &'static str,
    pub(crate) label: Option<String>,
}

impl Relation {
    /// `name` (URL segment): the records of `target` whose `field` (a
    /// `belongs_to` this resource) holds the record's key.
    pub fn has_many(name: &'static str, target: &'static str, field: &'static str) -> Self {
        Self { name, target, field, label: None }
    }

    /// Section title (default: the name, humanized).
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
}
