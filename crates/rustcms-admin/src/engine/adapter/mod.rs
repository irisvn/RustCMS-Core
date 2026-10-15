//! Generic CRUD engine: one monomorphized [`ResourceAdapter`] per resource,
//! erased to [`DynResource`] for the HTTP layer.

mod audit;
mod input;
mod operations;
mod read;
mod references;
mod schema;
mod write;

pub(crate) use read::{escape_like, search_match};

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::marker::PhantomData;
use std::sync::Arc;

use async_trait::async_trait;
use rustcms_core::{AppError, AppResult, ValidationErrors};
use sea_orm::sea_query::{Expr, Value};
use sea_orm::{
    ActiveModelBehavior, ActiveModelTrait, ColumnTrait, Condition, ConnectionTrait, DatabaseTransaction, DbBackend,
    DbErr, EntityName, EntityTrait, IdenStatic, IntoActiveModel, Iterable, ModelTrait, Order, PaginatorTrait,
    PrimaryKeyToColumn, QueryFilter, QueryOrder, QuerySelect, SqlErr,
};

use crate::Data;
use crate::action::{Action, ActionOutcome};
use crate::context::Ctx;
use crate::engine::validate::{InputRule, check_input, check_written};
use crate::field::{Field, Operation};
use crate::policy::{Policy, ability};
use crate::registry::DynResource;
use crate::resource::{
    Envelope, FilterOp, KeyStrategy, Limits, ListQuery, Page, PageMeta, Relation, Resource, ResourceMeta, Trashed,
};
use crate::schema::{
    ActionSchema, BadgeColor, ColumnSchema, ComponentSchema, FilterSchema, FormSchema, InputSchema, LayoutSchema,
    Relationship, ResourceInfo, ResourceSchema, Rule, SCHEMA_VERSION, SchemaOperation, TableSchema, VOCABULARY_VERSION,
};
use crate::ui::{Component, Form, Input, InputType, Layout, LayoutType, SelectOption, Table, TableColumn, humanize};
use crate::wire::{FieldKind, encode, encode_column, encode_key};

type Entity<R> = <R as Resource>::Entity;
type Model<R> = <Entity<R> as EntityTrait>::Model;
type Column<R> = <Entity<R> as EntityTrait>::Column;

const DEFAULT_PER_PAGE: u64 = 25;
/// Default most pairs of a key/value input.
const KEY_VALUE_ITEMS: usize = 50;
/// Default longest value of a key/value pair (keys: 255).
const KEY_VALUE_LENGTH: usize = 1000;
struct ColumnInfo<C> {
    column: C,
    kind: FieldKind,
    nullable: bool,
    /// Literal default declared on the entity (`default_value`), in wire format.
    default: Option<serde_json::Value>,
    /// Whether the entity declares any default (also ones not shown, like JSON
    /// or expression defaults).
    has_default: bool,
}

/// Generic implementation of [`DynResource`] for a [`Resource`].
pub(crate) struct ResourceAdapter<R: Resource> {
    meta: ResourceMeta,
    fields: Vec<Field>,
    columns: HashMap<String, ColumnInfo<Column<R>>>,
    pk: Column<R>,
    pk_name: String,
    pk_kind: FieldKind,
    table: String,
    /// Soft delete marker: column, name and kind.
    soft: Option<(Column<R>, String, FieldKind)>,
    policy: Arc<dyn Policy>,
    actions: Vec<Action<Entity<R>>>,
    relations: Vec<Relation>,
    form: Form,
    /// Key/value fields: most pairs and longest value.
    key_values: HashMap<String, (usize, usize)>,
    table_ui: Table,
    limits: Limits,
    _resource: PhantomData<fn() -> R>,
}

impl<R: Resource> ResourceAdapter<R> {
    pub(crate) fn new() -> Self {
        let slug = R::SLUG;
        let columns: HashMap<_, _> = Column::<R>::iter()
            .filter_map(|column| {
                let def = column.def();
                let kind = FieldKind::from_column_type(def.get_column_type())?;
                let default = match def.get_column_default() {
                    Some(sea_orm::sea_query::Expr::Value(value)) => {
                        normalize_default(kind, crate::wire::encode_column(def.get_column_type(), value))
                    }
                    _ => None,
                };
                let has_default = def.get_column_default().is_some();
                let info = ColumnInfo { column, kind, nullable: def.is_null(), default, has_default };
                Some((column.as_str().to_owned(), info))
            })
            .collect();

        let mut keys = <<Entity<R> as EntityTrait>::PrimaryKey as Iterable>::iter();
        let pk = keys.next().unwrap_or_else(|| panic!("resource `{slug}`: entity has no primary key")).into_column();
        assert!(keys.next().is_none(), "resource `{slug}`: composite primary keys are not supported");
        let pk_name = pk.as_str().to_owned();
        let pk_kind = columns.get(&pk_name).map(|c| c.kind);
        let pk_kind = match pk_kind {
            Some(kind @ (FieldKind::I32 | FieldKind::I64 | FieldKind::String | FieldKind::Uuid)) => kind,
            other => panic!("resource `{slug}`: unsupported primary key type {other:?}"),
        };

        let meta = R::meta();
        let fields = R::fields();
        let mut seen = HashSet::new();
        for field in &fields {
            assert!(
                columns.contains_key(&field.name),
                "resource `{slug}`: unknown or unsupported column `{}`",
                field.name
            );
            assert!(seen.insert(field.name.as_str()), "resource `{slug}`: field `{}` declared twice", field.name);
        }
        // The audit log names resources and actions as they are declared.
        assert!(slug.len() <= 64, "resource `{slug}`: slugs are at most 64 bytes");
        // Subjects of the framework's own audit entries.
        assert!(
            !["user", "role", "permission", "settings", "session", "job", "queue"].contains(&slug),
            "resource `{slug}`: this slug is reserved"
        );
        assert!(
            !(meta.audit && fields.iter().any(|f| f.name == pk_name && f.audit_redact)),
            "resource `{slug}`: the primary key identifies records in the audit log and cannot be \
             `.audit_redact()`; use an opaque key or `ResourceMeta::audit(false)`"
        );
        if let Some(title) = &meta.title_field {
            assert!(seen.contains(title.as_str()), "resource `{slug}`: title field `{title}` is not a field");
        }
        let soft = meta.soft_delete.as_deref().map(|name| {
            let info = columns
                .get(name)
                .unwrap_or_else(|| panic!("resource `{slug}`: soft delete column `{name}` is unknown or unsupported"));
            assert!(info.nullable, "resource `{slug}`: soft delete column `{name}` must be nullable");
            assert!(
                matches!(info.kind, FieldKind::Instant | FieldKind::LocalDateTime),
                "resource `{slug}`: soft delete column `{name}` must be a timestamp"
            );
            assert!(
                fields.iter().all(|f| f.name != name || f.guarded),
                "resource `{slug}`: soft delete column `{name}` may only be a `guarded()` field"
            );
            (info.column, name.to_owned(), info.kind)
        });
        for constraint in &meta.constraints {
            for column in &constraint.columns {
                assert!(columns.contains_key(column), "resource `{slug}`: constraint column `{column}` is unknown");
            }
        }

        let form = Some(R::form()).filter(|f| !f.components.is_empty()).unwrap_or_else(|| auto_form(&fields, &columns));
        for input in form.inputs() {
            assert!(
                seen.contains(input.name.as_str()),
                "resource `{slug}`: form input `{}` is not a field",
                input.name
            );
        }
        check_layouts(slug, &form.components, None);
        for input in form.inputs().into_iter().filter(|i| i.kind == InputType::Custom) {
            let widget = input.widget.as_deref().unwrap_or_default();
            assert!(crate::ui::valid_widget(widget), "resource `{slug}`: invalid widget name `{widget}`");
            let size = input.props.as_ref().map_or(0, |p| p.to_string().len());
            assert!(size <= 4096, "resource `{slug}`: props of widget `{widget}` exceed 4 KiB");
        }
        for field in &fields {
            assert!(
                !field.rich_text || columns[&field.name].kind == FieldKind::Json,
                "resource `{slug}`: rich text field `{}` needs a JSON column",
                field.name
            );
            // A default would be stored without passing the reference checks.
            assert!(
                !(field.rich_text || field.media.is_some()) || !columns[&field.name].has_default,
                "resource `{slug}`: media and rich text field `{}` may not have a column default",
                field.name
            );
            if let Some(rule) = &field.media {
                let expected = if rule.multiple { FieldKind::Json } else { FieldKind::Uuid };
                assert!(
                    columns[&field.name].kind == expected,
                    "resource `{slug}`: media field `{}` needs a {} column",
                    field.name,
                    if rule.multiple { "JSON" } else { "UUID" }
                );
            }
        }
        // Key/value fields: declared on the field, or implied by a `KeyValue` input.
        // Either way the bounds hold on every write, whatever the form shows.
        let inputs: HashSet<&str> =
            form.inputs().into_iter().filter(|i| i.kind == InputType::KeyValue).map(|i| i.name.as_str()).collect();
        let mut key_values = HashMap::new();
        for field in fields.iter().filter(|f| f.key_value || inputs.contains(f.name.as_str())) {
            assert!(
                columns[&field.name].kind == FieldKind::Json,
                "resource `{slug}`: key/value field `{}` needs a JSON column",
                field.name
            );
            key_values.insert(
                field.name.clone(),
                (field.max_items.unwrap_or(KEY_VALUE_ITEMS), field.max_length.unwrap_or(KEY_VALUE_LENGTH)),
            );
        }
        let mut table_ui = R::table();
        for column in &table_ui.columns {
            if let Some(cell) = &column.cell {
                assert!(crate::ui::valid_widget(cell), "resource `{slug}`: invalid cell name `{cell}`");
            }
            let size = column.props.as_ref().map_or(0, |p| p.to_string().len());
            assert!(size <= 4096, "resource `{slug}`: props of column `{}` exceed 4 KiB", column.name);
        }
        if table_ui.columns.is_empty() {
            table_ui.columns = auto_columns(&fields, &columns);
        }
        let field = |name: &str| fields.iter().find(|f| f.name == name);
        for column in &table_ui.columns {
            assert!(field(&column.name).is_some(), "resource `{slug}`: table column `{}` is not a field", column.name);
        }
        for filter in &table_ui.filters {
            assert!(
                field(&filter.name).is_some_and(|f| f.filterable),
                "resource `{slug}`: filter `{}` needs a `.filterable()` field",
                filter.name
            );
        }
        if let Some(sort) = &table_ui.default_sort {
            let name = sort.trim_start_matches('-');
            assert!(
                field(name).is_some_and(|f| f.sortable),
                "resource `{slug}`: default sort `{name}` needs a `.sortable()` field"
            );
        }
        let actions = R::actions();
        let mut action_names = HashSet::new();
        for action in &actions {
            assert!(action_names.insert(action.name), "resource `{slug}`: action `{}` declared twice", action.name);
            assert!(action.name.len() <= 57, "resource `{slug}`: action `{}` is longer than 57 bytes", action.name);
            assert!(
                ![
                    ability::VIEW_ANY,
                    ability::VIEW,
                    ability::CREATE,
                    ability::UPDATE,
                    ability::DELETE,
                    ability::RESTORE,
                    ability::FORCE_DELETE
                ]
                .contains(&action.name),
                "resource `{slug}`: action name `{}` is reserved for CRUD abilities",
                action.name
            );
        }

        Self {
            table: Entity::<R>::default().table_name().to_owned(),
            soft,
            meta,
            fields,
            columns,
            pk,
            pk_name,
            pk_kind,
            policy: R::policy(),
            actions,
            relations: R::relations(),
            form,
            key_values,
            table_ui,
            limits: Limits::default(),
            _resource: PhantomData,
        }
    }

    fn field(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.name == name)
    }

    fn column(&self, name: &str) -> &ColumnInfo<Column<R>> {
        &self.columns[name]
    }

    /// Every column of the record in wire format. Trusted: for policies and hooks only.
    fn record(model: &Model<R>) -> Data {
        Column::<R>::iter()
            .map(|c| (c.as_str().to_owned(), encode_column(c.def().get_column_type(), &model.get(c))))
            .collect()
    }

    fn key_of(&self, model: &Model<R>) -> String {
        encode_key(&model.get(self.pk))
    }

    fn decode_keys(&self, keys: &[String]) -> AppResult<Vec<Value>> {
        if keys.len() > self.limits.keys {
            return Err(
                ValidationErrors::field("keys", format!("may not have more than {} items", self.limits.keys)).into()
            );
        }
        let unique: BTreeSet<&String> = keys.iter().collect();
        Ok(unique.into_iter().filter_map(|k| self.pk_kind.decode_key(k).ok()).collect())
    }

    /// Refuses before any lookup by key when the policy does not admit
    /// `ability` at all (see [`Policy::admits`]).
    async fn admit(&self, ctx: &Ctx, ability: &str) -> AppResult<()> {
        if self.policy.admits(ctx, ability).await { Ok(()) } else { Err(AppError::Forbidden) }
    }

    /// Records in the user's scope, by deletion state. Every read goes through
    /// here, so trashed records stay hidden unless asked for.
    fn base(&self, ctx: &Ctx, trashed: Trashed) -> sea_orm::Select<Entity<R>> {
        let select = R::scope(Entity::<R>::find(), ctx);
        match (&self.soft, trashed) {
            (Some((column, _, _)), Trashed::Without) => select.filter(column.is_null()),
            (Some((column, _, _)), Trashed::Only) => select.filter(column.is_not_null()),
            _ => select,
        }
    }

    /// Whether the trusted `record` is in the trash.
    fn is_trashed(&self, record: &Data) -> bool {
        self.soft.as_ref().is_some_and(|(_, name, _)| record.get(name).is_some_and(|v| !v.is_null()))
    }

    async fn find_model<C: ConnectionTrait>(&self, ctx: &Ctx, conn: &C, key: &str) -> AppResult<Model<R>> {
        self.find_model_in(ctx, conn, key, Trashed::Without).await
    }

    async fn find_model_in<C: ConnectionTrait>(
        &self,
        ctx: &Ctx,
        conn: &C,
        key: &str,
        trashed: Trashed,
    ) -> AppResult<Model<R>> {
        let key = self.pk_kind.decode_key(key).map_err(|_| AppError::NotFound)?;
        self.base(ctx, trashed).filter(self.pk.eq(key)).one(conn).await?.ok_or(AppError::NotFound)
    }

    /// The rows of `keys` as stored (no scope, any trash state).
    async fn models_by_keys<C: ConnectionTrait>(&self, conn: &C, keys: &[String]) -> AppResult<Vec<Model<R>>> {
        let values = self.decode_keys(keys)?;
        if values.is_empty() {
            return Ok(Vec::new());
        }
        Ok(Entity::<R>::find().filter(self.pk.is_in(values)).all(conn).await?)
    }

    /// Models in scope among `keys`, together with their trusted records.
    async fn models_in_scope<C: ConnectionTrait>(
        &self,
        ctx: &Ctx,
        conn: &C,
        keys: &[String],
    ) -> AppResult<Vec<(Model<R>, Data)>> {
        let values = self.decode_keys(keys)?;
        if values.is_empty() {
            return Ok(Vec::new());
        }
        let models = self.base(ctx, Trashed::Without).filter(self.pk.is_in(values)).all(conn).await?;
        Ok(models
            .into_iter()
            .map(|m| {
                let r = Self::record(&m);
                (m, r)
            })
            .collect())
    }
}

/// A declared default in the column's wire type, or `None` when it cannot be
/// stated reliably: JSON defaults are text whose meaning depends on the
/// database (a document or a string), so they are always left to the database,
/// as is anything that does not decode as the column's kind.
fn normalize_default(kind: FieldKind, value: serde_json::Value) -> Option<serde_json::Value> {
    if kind == FieldKind::Json {
        return None;
    }
    kind.decode(&value).is_ok().then_some(value)
}

/// One input per field, by column type.
/// Tabs hold only tabs, tabs sit only in tabs, and tab ids are unique per tabs.
fn check_layouts(slug: &str, components: &[Component], parent: Option<LayoutType>) {
    let mut ids = HashSet::new();
    for component in components {
        let Component::Layout(Layout { kind, id, children, .. }) = component else {
            assert!(parent != Some(LayoutType::Tabs), "resource `{slug}`: tabs may only contain `Tab`s");
            continue;
        };
        assert!(
            (*kind == LayoutType::Tab) == (parent == Some(LayoutType::Tabs)),
            "resource `{slug}`: a `Tab` belongs directly in `Tabs`, and `Tabs` hold only `Tab`s"
        );
        if let Some(id) = id {
            assert!(
                !id.is_empty() && ids.insert(id.clone()),
                "resource `{slug}`: tab `{id}` is empty or declared twice"
            );
        }
        check_layouts(slug, children, Some(*kind));
    }
}

fn auto_form<C>(fields: &[Field], columns: &HashMap<String, ColumnInfo<C>>) -> Form {
    let inputs = fields
        .iter()
        .filter(|f| !f.guarded)
        .map(|f| {
            let kind = if f.belongs_to.is_some() {
                InputType::Select
            } else {
                match columns[&f.name].kind {
                    FieldKind::I32 | FieldKind::I64 | FieldKind::F64 | FieldKind::Decimal => InputType::NumberInput,
                    FieldKind::Bool => InputType::Toggle,
                    FieldKind::Date => InputType::DatePicker,
                    FieldKind::LocalDateTime | FieldKind::Instant => InputType::DateTimePicker,
                    FieldKind::Json => InputType::Textarea,
                    FieldKind::String | FieldKind::Uuid => InputType::TextInput,
                }
            };
            Component::Input(Input::with_kind(&f.name, kind))
        })
        .collect();
    Form::new().schema(inputs)
}

/// The first readable-by-default fields as columns.
fn auto_columns<C>(fields: &[Field], columns: &HashMap<String, ColumnInfo<C>>) -> Vec<TableColumn> {
    use crate::ui::{BooleanColumn, DateColumn, TextColumn};
    fields
        .iter()
        .filter(|f| !f.guarded && columns[&f.name].kind != FieldKind::Json)
        .take(6)
        .map(|f| match columns[&f.name].kind {
            FieldKind::Bool => BooleanColumn::make(&f.name),
            FieldKind::Date | FieldKind::LocalDateTime | FieldKind::Instant => DateColumn::make(&f.name),
            _ => TextColumn::make(&f.name),
        })
        .collect()
}

fn title_text(value: &serde_json::Value) -> String {
    value.as_str().map_or_else(|| value.to_string(), str::to_owned)
}

/// Page sizes offered by the table.
const PER_PAGE_OPTIONS: [u64; 4] = [10, 25, 50, 100];
/// Most options returned by one options lookup.
const MAX_OPTIONS: u64 = 50;
/// Time kept before the request deadline to send a committed write's response.
const RESPONSE_MARGIN: std::time::Duration = std::time::Duration::from_millis(250);
/// Longest wait for a written record's envelope before answering with its key
/// alone (also bounded by the request deadline).
const POST_COMMIT_HYDRATION: std::time::Duration = std::time::Duration::from_secs(2);
/// Records loaded at a time while checking candidates (bounds memory by row size).
const LOAD_CHUNK: usize = 100;
/// Rows read per batch when a list checks records individually.
const LIST_SCAN_BATCH: u64 = 100;
/// Most matching rows a list checks individually before refusing.
const MAX_LIST_SCAN: u64 = 10_000;
/// Candidate batches read at most while filtering options by policy.
const MAX_OPTION_BATCHES: u64 = 20;
