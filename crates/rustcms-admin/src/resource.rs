//! The `Resource` trait implemented by applications, and its wire types.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use rustcms_core::AppResult;
use sea_orm::sea_query::Value;
use sea_orm::{ActiveModelBehavior, DatabaseTransaction, EntityTrait, IntoActiveModel, Select};
use serde::{Deserialize, Serialize};

use crate::Data;
use crate::action::Action;
use crate::context::Ctx;
use crate::field::Field;
use crate::policy::{PermissionPolicy, Policy};

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
    pub(crate) key: KeyStrategy,
    pub(crate) title_field: Option<String>,
    pub(crate) constraints: Vec<ConstraintMap>,
}

impl ResourceMeta {
    /// Creates metadata with a human label and a database-generated key.
    pub fn new(label: impl Into<String>) -> Self {
        Self { label: label.into(), key: KeyStrategy::DbGenerated, title_field: None, constraints: Vec::new() }
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

    /// Runs after insert or update, in the same transaction.
    async fn after_save(
        _ctx: &Ctx,
        _txn: &DatabaseTransaction,
        _record: &<Self::Entity as EntityTrait>::Model,
    ) -> AppResult<()> {
        Ok(())
    }
}

/// Operator of a list filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterOp {
    /// Equal.
    Eq,
    /// Member of a list.
    In,
    /// Greater than.
    Gt,
    /// Greater or equal.
    Gte,
    /// Less than.
    Lt,
    /// Less or equal.
    Lte,
}

/// One list filter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Filter {
    /// Field name.
    pub field: String,
    /// Operator.
    pub op: FilterOp,
    /// Value in wire format (an array for `in`).
    pub value: serde_json::Value,
}

/// Parameters of a list request.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListQuery {
    /// 1-based page.
    pub page: Option<u64>,
    /// Page size (capped).
    pub per_page: Option<u64>,
    /// Field to sort by, `-` prefix for descending.
    pub sort: Option<String>,
    /// Text searched in searchable fields.
    pub search: Option<String>,
    /// Filters, combined with AND.
    #[serde(default)]
    pub filters: Vec<Filter>,
}

/// Request size limits (see plan §4). Exceeding one is a 422.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Maximum page size.
    pub per_page: u64,
    /// Maximum search length (chars).
    pub search_len: usize,
    /// Maximum number of filters.
    pub filters: usize,
    /// Maximum items in an `in` filter.
    pub in_items: usize,
    /// Maximum keys in an action or lookup.
    pub keys: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self { per_page: 100, search_len: 100, filters: 10, in_items: 100, keys: 500 }
    }
}

/// A record as sent to clients.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Envelope {
    /// Encoded primary key.
    pub key: String,
    /// Record title, when the title field is readable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Record-level abilities computed by the policy.
    pub abilities: BTreeMap<&'static str, bool>,
    /// Readable fields only.
    pub data: Data,
    /// Titles of related records for `belongs_to` fields (`#key` when not visible).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
}

/// Pagination metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageMeta {
    /// Current page (1-based).
    pub page: u64,
    /// Page size.
    pub per_page: u64,
    /// Total records in scope matching the query.
    pub total: u64,
    /// Last page number.
    pub last_page: u64,
}

/// One page of records.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Page {
    /// Records.
    pub data: Vec<Envelope>,
    /// Pagination.
    pub meta: PageMeta,
}
