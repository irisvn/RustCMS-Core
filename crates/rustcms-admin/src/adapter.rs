//! Generic CRUD engine: one monomorphized [`ResourceAdapter`] per resource,
//! erased to [`DynResource`] for the HTTP layer.

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
/// Longest key of a key/value pair.
const KEY_LENGTH: usize = 255;

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

    /// Relations the user may list, and whether they may create through them.
    async fn relation_schemas(&self, ctx: &Ctx) -> Vec<crate::schema::RelationSchema> {
        let mut out = Vec::new();
        for relation in &self.relations {
            let Some(target) = ctx.registry.get(relation.target) else { continue };
            let (readable, creatable) = target.field_access(ctx, relation.field);
            if !readable || !target.allows(ctx, ability::VIEW_ANY).await {
                continue;
            }
            out.push(crate::schema::RelationSchema {
                name: relation.name,
                label: relation.label.clone().unwrap_or_else(|| humanize(relation.name)),
                resource: relation.target,
                field: relation.field,
                create: creatable && target.allows(ctx, ability::CREATE).await,
            });
        }
        out
    }

    /// The idempotency claim of a create (through a relation: of that parent).
    fn create_claim(&self, ctx: &Ctx, input: &Data) -> Option<crate::idempotency::Claim> {
        let operation = match &ctx.bound {
            Some((relation, _, parent)) => format!("create:{relation}:{parent}"),
            None => "create".to_owned(),
        };
        ctx.idempotency_key.as_deref().map(|key| {
            crate::idempotency::Claim::new(
                &ctx.principal,
                R::SLUG,
                &operation,
                key,
                &serde_json::Value::Object(input.clone()),
            )
        })
    }

    /// The wire value of field `field` for the parent key `key`.
    fn fk_value(&self, field: &str, key: &str) -> serde_json::Value {
        self.column(field).kind.from_query(&serde_json::Value::String(key.to_owned()))
    }

    /// A rich text field holds a valid document.
    fn check_rich_text(&self, name: &str, value: &serde_json::Value, errors: &mut ValidationErrors) -> bool {
        if value.is_null() || !self.field(name).is_some_and(|f| f.rich_text) {
            return true;
        }
        match crate::rich_text::parse(value) {
            Ok(_) => true,
            Err(e) => {
                errors.add(name, e.0);
                false
            }
        }
    }

    /// A key/value field holds an object of strings within its bounds.
    fn check_key_value(&self, name: &str, value: &serde_json::Value, errors: &mut ValidationErrors) -> bool {
        let Some(&(max_items, max_length)) = self.key_values.get(name) else { return true };
        if value.is_null() {
            return true;
        }
        let Some(object) = value.as_object() else {
            errors.add(name, "must be an object of text values");
            return false;
        };
        let message = if object.len() > max_items {
            format!("may not have more than {max_items} items")
        } else if object.keys().any(|k| k.trim().is_empty()) {
            "may not have an empty key".to_owned()
        } else if object.keys().any(|k| k.chars().count() > KEY_LENGTH) {
            format!("may not have keys longer than {KEY_LENGTH} characters")
        } else if object.values().any(|v| !v.is_string()) {
            "must be an object of text values".to_owned()
        } else if object.values().any(|v| v.as_str().is_some_and(|t| t.chars().count() > max_length)) {
            format!("may not have values longer than {max_length} characters")
        } else {
            return true;
        };
        errors.add(name, message);
        false
    }

    /// Validates client input against field permissions and types.
    fn accept_input(&self, ctx: &Ctx, op: Operation, input: Data) -> AppResult<Data> {
        let mut errors = ValidationErrors::new();
        let natural_create = op == Operation::Create && matches!(self.meta.key, KeyStrategy::Natural);
        for (name, value) in &input {
            let allowed =
                self.field(name).filter(|f| f.can_write(op, ctx)).filter(|_| *name != self.pk_name || natural_create);
            let Some(field) = allowed else {
                errors.add(name.as_str(), "is not allowed");
                continue;
            };
            let column = self.column(name);
            if value.is_null() {
                if field.required || !column.nullable {
                    errors.add(name.as_str(), "is required");
                }
            } else if let Err(e) = column.kind.decode(value) {
                errors.add(name.as_str(), e.0);
            } else if !self.check_key_value(name, value, &mut errors) || !self.check_rich_text(name, value, &mut errors)
            {
            } else if let (Some(max), Some(text)) = (field.max_length, value.as_str())
                && text.chars().count() > max
            {
                errors.add(name.as_str(), format!("may not be longer than {max} characters"));
            }
        }
        if op == Operation::Create {
            for field in self.fields.iter().filter(|f| f.required && f.can_write(op, ctx)) {
                if !input.contains_key(&field.name) {
                    errors.add(field.name.as_str(), "is required");
                }
            }
        }
        errors.into_result()?;
        Ok(input)
    }

    /// The final check of what a write stores (after hooks): related records
    /// and media must be visible to the user, media must match its field.
    async fn check_written(&self, ctx: &Ctx, txn: &DatabaseTransaction, data: &Data) -> AppResult<()> {
        self.check_relations(ctx, txn, data).await?;
        self.check_media(ctx, txn, data).await
    }

    /// Media references: well-formed, existing, not trashed, visible to the
    /// user, and of an accepted type and size. The referenced rows are locked
    /// (shared, in id order) so they cannot be trashed meanwhile.
    async fn check_media(&self, ctx: &Ctx, txn: &DatabaseTransaction, data: &Data) -> AppResult<()> {
        use crate::media::entity as media;
        let mut errors = ValidationErrors::new();
        let images_only = crate::field::MediaRule {
            multiple: true,
            max_items: usize::MAX,
            accept: vec!["image/*".to_owned()],
            max_size: None,
        };
        let mut wanted: Vec<(&Field, &crate::field::MediaRule, Vec<uuid::Uuid>)> = Vec::new();
        for field in self.fields.iter().filter(|f| f.rich_text) {
            let Some(value) = data.get(&field.name).filter(|v| !v.is_null()) else { continue };
            match crate::rich_text::parse(value) {
                Ok(doc) => {
                    let ids = crate::rich_text::media_ids(&doc);
                    if !ids.is_empty() {
                        wanted.push((field, &images_only, ids));
                    }
                }
                Err(e) => errors.add(field.name.as_str(), e.0),
            }
        }
        for field in &self.fields {
            let (Some(rule), Some(value)) = (&field.media, data.get(&field.name)) else { continue };
            let texts: Vec<Option<&str>> = match value {
                serde_json::Value::Null => continue,
                serde_json::Value::String(id) if !rule.multiple => vec![Some(id.as_str())],
                serde_json::Value::Array(items) if rule.multiple => items.iter().map(|v| v.as_str()).collect(),
                _ => {
                    errors.add(field.name.as_str(), "is invalid");
                    continue;
                }
            };
            let ids: Option<Vec<uuid::Uuid>> =
                texts.iter().map(|t| t.and_then(|t| uuid::Uuid::parse_str(t).ok())).collect();
            match ids {
                Some(ids) if ids.len() > rule.max_items => {
                    errors.add(field.name.as_str(), format!("may not have more than {} items", rule.max_items));
                }
                Some(ids) if ids.iter().collect::<HashSet<_>>().len() != ids.len() => {
                    errors.add(field.name.as_str(), "may not list the same file twice");
                }
                Some(ids) => wanted.push((field, rule, ids)),
                None => errors.add(field.name.as_str(), "is invalid"),
            }
        }
        errors.into_result()?;
        if wanted.is_empty() {
            return Ok(());
        }
        let library = ctx.registry.get(crate::media::SLUG).ok_or_else(|| {
            AppError::internal(format!("`{}`: media fields need the media library (`AdminProvider::media`)", R::SLUG))
        })?;
        let mut all: Vec<uuid::Uuid> = wanted.iter().flat_map(|(_, _, ids)| ids.iter().copied()).collect();
        all.sort_unstable();
        all.dedup();
        // Visibility first: records the user cannot see are never locked.
        let keys: Vec<String> = all.iter().map(uuid::Uuid::to_string).collect();
        let visible = library.keys_in_scope(ctx, txn, &keys).await?;
        let lockable: Vec<uuid::Uuid> = all.iter().copied().filter(|id| visible.contains(&id.to_string())).collect();
        // Re-read under the lock: trashed meanwhile means unavailable.
        let rows: HashMap<uuid::Uuid, media::Model> = match txn.get_database_backend() {
            // InnoDB locks while scanning: one point lock per id, in id order (the
            // byte order of `binary(16)` and of `Uuid`).
            DbBackend::MySql => {
                let mut rows = HashMap::new();
                for id in lockable {
                    if let Some(m) = media::Entity::find_by_id(id).lock_shared().one(txn).await? {
                        rows.insert(m.id, m);
                    }
                }
                rows
            }
            backend => {
                let mut select = media::Entity::find()
                    .filter(media::Column::Id.is_in(lockable))
                    .order_by(media::Column::Id, Order::Asc);
                if backend == DbBackend::Postgres {
                    select = select.lock_shared();
                }
                select.all(txn).await?.into_iter().map(|m| (m.id, m)).collect()
            }
        };
        let mut errors = ValidationErrors::new();
        for (field, rule, ids) in wanted {
            for id in ids {
                let usable = rows.get(&id).filter(|m| m.deleted_at.is_none() && visible.contains(&id.to_string()));
                let message = match usable {
                    None => Some("refers to a file that is not available".to_owned()),
                    Some(m) if !rule.accepts(&m.mime) => Some(format!("does not accept {} files", m.mime)),
                    Some(m) if rule.max_size.is_some_and(|max| u64::try_from(m.size).unwrap_or(u64::MAX) > max) => {
                        Some("refers to a file that is too large".to_owned())
                    }
                    Some(_) => None,
                };
                if let Some(message) = message {
                    errors.add(field.name.as_str(), message);
                    break;
                }
            }
        }
        errors.into_result()
    }

    /// Client-supplied foreign keys must point to records the user can see.
    async fn check_relations(&self, ctx: &Ctx, txn: &DatabaseTransaction, input: &Data) -> AppResult<()> {
        let mut errors = ValidationErrors::new();
        for field in &self.fields {
            let (Some(target), Some(value)) = (field.belongs_to, input.get(&field.name)) else {
                continue;
            };
            if value.is_null() {
                continue;
            }
            let key = value.as_str().map_or_else(|| value.to_string(), str::to_owned);
            let target =
                ctx.registry.get(target).ok_or_else(|| AppError::internal(format!("unknown resource `{target}`")))?;
            if !target.keys_in_scope(ctx, txn, std::slice::from_ref(&key)).await?.contains(&key) {
                errors.add(field.name.as_str(), "is invalid");
            }
        }
        errors.into_result()
    }

    /// Writes trusted data (validated input, possibly changed by hooks) into the active model.
    fn apply(&self, model: &mut <Entity<R> as EntityTrait>::ActiveModel, data: &Data, op: Operation) -> AppResult<()> {
        let mut errors = ValidationErrors::new();
        for (name, value) in data {
            let column = self
                .columns
                .get(name)
                .ok_or_else(|| AppError::internal(format!("`{}`: hook wrote unknown column `{name}`", R::SLUG)))?;
            if op == Operation::Update && *name == self.pk_name {
                errors.add(name.as_str(), "is not allowed");
                continue;
            }
            if self.soft.as_ref().is_some_and(|(_, soft, _)| soft == name) {
                return Err(AppError::internal(format!(
                    "`{}`: hooks may not write the soft delete column `{name}` (use delete and restore)",
                    R::SLUG
                )));
            }
            if value.is_null() && !column.nullable {
                errors.add(name.as_str(), "is required");
                continue;
            }
            // Hooks may change values: limits hold for what is actually written.
            if !self.check_key_value(name, value, &mut errors) || !self.check_rich_text(name, value, &mut errors) {
                continue;
            }
            let max_length = self.field(name).and_then(|f| f.max_length);
            if let (Some(max), Some(text)) = (max_length, value.as_str())
                && text.chars().count() > max
            {
                errors.add(name.as_str(), format!("may not be longer than {max} characters"));
                continue;
            }
            let candidates = match column.kind.decode(value) {
                Ok(candidates) => candidates,
                Err(e) => {
                    errors.add(name.as_str(), e.0);
                    continue;
                }
            };
            if !candidates.into_iter().any(|v| model.try_set(column.column, v).is_ok()) {
                // Only a number too large for an `f32` column (see `FieldKind::decode`).
                if column.kind == FieldKind::F64 {
                    errors.add(name.as_str(), "is out of range");
                    continue;
                }
                return Err(AppError::internal(format!("`{}`: column `{name}` does not match its wire kind", R::SLUG)));
            }
        }
        errors.into_result()
    }

    fn map_db_err(&self, err: DbErr) -> AppError {
        match err.sql_err() {
            Some(SqlErr::UniqueConstraintViolation(message)) => {
                let constraint = self.meta.constraints.iter().find(|c| {
                    let sqlite =
                        c.columns.iter().map(|col| format!("{}.{col}", self.table)).collect::<Vec<_>>().join(", ");
                    // PostgreSQL quotes the constraint; SQLite lists the columns; MySQL ends
                    // with `for key 'table.index'`, where a column's own `UNIQUE` index is
                    // named after the column.
                    let mysql = |index: &str| message.ends_with(&format!(" for key '{}.{index}'", self.table));
                    message.contains(&format!("\"{}\"", c.name))
                        || message.ends_with(&sqlite)
                        || mysql(&c.name)
                        || matches!(c.columns.as_slice(), [column] if mysql(column))
                });
                let mut errors = ValidationErrors::new();
                match constraint {
                    Some(c) => match &c.field {
                        Some(field) => errors.add(field.as_str(), c.message.as_str()),
                        None => errors.add_form(c.message.as_str()),
                    },
                    None => {
                        tracing::warn!(resource = R::SLUG, %message, "unmapped unique constraint");
                        errors.add_form("conflicts with existing data");
                    }
                }
                errors.into()
            }
            Some(SqlErr::ForeignKeyConstraintViolation(message)) => {
                tracing::warn!(resource = R::SLUG, %message, "foreign key violation");
                let mut errors = ValidationErrors::new();
                errors.add_form("references a missing or protected record");
                errors.into()
            }
            _ => err.into(),
        }
    }

    async fn envelopes(&self, ctx: &Ctx, models: Vec<Model<R>>) -> AppResult<Vec<Envelope>> {
        let readable: Vec<&Field> = self.fields.iter().filter(|f| f.can_read(ctx)).collect();
        // Labels are presentation: a failure (e.g. a related resource unavailable)
        // must not fail the request, least of all after a committed write.
        let labels = match self.labels(ctx, &readable, &models).await {
            Ok(labels) => labels,
            Err(err) => {
                tracing::warn!(resource = R::SLUG, %err, "relationship labels unavailable");
                vec![BTreeMap::new(); models.len()]
            }
        };
        let title_readable = self.meta.title_field.as_ref().filter(|t| readable.iter().any(|f| &f.name == *t));
        let mut out = Vec::with_capacity(models.len());
        for (model, labels) in models.iter().zip(labels) {
            let record = Self::record(model);
            let trashed = self.is_trashed(&record);
            let mut abilities = BTreeMap::new();
            abilities.insert(ability::VIEW, self.policy.allows(ctx, ability::VIEW, Some(&record)).await);
            // A trashed record can only be restored or deleted for good; an active
            // one can do anything but those.
            for name in [ability::UPDATE, ability::DELETE] {
                abilities.insert(name, !trashed && self.policy.allows(ctx, name, Some(&record)).await);
            }
            if self.soft.is_some() {
                for name in [ability::RESTORE, ability::FORCE_DELETE] {
                    abilities.insert(name, trashed && self.policy.allows(ctx, name, Some(&record)).await);
                }
            }
            // Decided before action entries are added (their names cannot collide,
            // see registration, but the view decision never depends on them).
            let viewable = abilities[ability::VIEW];
            for action in &self.actions {
                abilities.insert(action.name, !trashed && self.policy.allows(ctx, action.ability, Some(&record)).await);
            }
            // Every envelope (list, read, and the result of a write) carries data
            // only if the user may view the record; blind writes get key and abilities.
            out.push(Envelope {
                key: self.key_of(model),
                title: title_readable.filter(|_| viewable).map(|t| title_text(&record[t.as_str()])),
                abilities,
                data: if viewable {
                    readable.iter().map(|f| (f.name.clone(), record[f.name.as_str()].clone())).collect()
                } else {
                    Data::new()
                },
                labels: if viewable { labels } else { BTreeMap::new() },
                trashed,
            });
        }
        Ok(out)
    }

    /// One `titles` lookup per relationship field per page (no N+1).
    async fn labels(
        &self,
        ctx: &Ctx,
        readable: &[&Field],
        models: &[Model<R>],
    ) -> AppResult<Vec<BTreeMap<String, String>>> {
        let mut out = vec![BTreeMap::new(); models.len()];
        for field in readable {
            let Some(target) = field.belongs_to else {
                continue;
            };
            let column = self.column(&field.name).column;
            let keys: Vec<Option<String>> = models
                .iter()
                .map(|m| Some(m.get(column)).filter(|v| !encode(v).is_null()).map(|v| encode_key(&v)))
                .collect();
            let wanted: Vec<String> = keys.iter().flatten().cloned().collect::<BTreeSet<_>>().into_iter().collect();
            if wanted.is_empty() {
                continue;
            }
            let target =
                ctx.registry.get(target).ok_or_else(|| AppError::internal(format!("unknown resource `{target}`")))?;
            let titles = target.titles(ctx, &wanted).await?;
            for (labels, key) in out.iter_mut().zip(keys) {
                if let Some(key) = key {
                    let label = titles.get(&key).cloned().unwrap_or_else(|| format!("#{key}"));
                    labels.insert(field.name.clone(), label);
                }
            }
        }
        Ok(out)
    }

    fn filter_condition(&self, ctx: &Ctx, query: &ListQuery, errors: &mut ValidationErrors) -> Condition {
        let mut condition = Condition::all();
        // Children of one record (relation lists): an internal condition, not a client filter.
        if let Some((field, key)) = &query.parent {
            let column = self.column(field);
            match column.kind.decode(&column.kind.from_query(&serde_json::Value::String(key.clone()))) {
                Ok(values) if !values.is_empty() => condition = condition.add(column.column.eq(values[0].clone())),
                _ => errors.add_form("invalid parent key"),
            }
        }
        if let Some(search) = query.search.as_deref().filter(|s| !s.is_empty()) {
            if search.chars().count() > self.limits.search_len {
                errors.add("search", format!("may not be longer than {} characters", self.limits.search_len));
            } else {
                let mut any = Condition::any();
                let pattern = format!("%{}%", escape_like(search));
                for field in self.fields.iter().filter(|f| f.searchable && f.can_read(ctx)) {
                    any = any.add(search_match(
                        ctx.db.get_database_backend(),
                        self.column(&field.name).column,
                        self.column(&field.name).kind,
                        &pattern,
                    ));
                }
                condition = condition.add(any);
            }
        }
        if query.filters.len() > self.limits.filters {
            errors.add("filters", format!("may not have more than {} items", self.limits.filters));
            return condition;
        }
        for filter in &query.filters {
            let name = filter.field.as_str();
            if !self.field(name).is_some_and(|f| f.filterable && f.can_read(ctx)) {
                errors.add(name, "cannot be filtered");
                continue;
            }
            let column = self.column(name);
            let decode = |v: &serde_json::Value| {
                let value = if query.from_query_string { column.kind.from_query(v) } else { v.clone() };
                column.kind.decode(&value).map(|mut c| c.swap_remove(0))
            };
            let expr = if filter.op == FilterOp::In {
                // Query strings send `in` lists comma-separated.
                let split: Vec<serde_json::Value>;
                let items = match &filter.value {
                    serde_json::Value::String(list) if query.from_query_string => {
                        split = list.split(',').map(|v| serde_json::Value::String(v.trim().to_owned())).collect();
                        split.as_slice()
                    }
                    other => other.as_array().map(Vec::as_slice).unwrap_or_default(),
                };
                if items.is_empty() || items.len() > self.limits.in_items {
                    errors.add(name, format!("must be a list of 1 to {} items", self.limits.in_items));
                    continue;
                }
                match items.iter().map(decode).collect::<Result<Vec<_>, _>>() {
                    Ok(values) => column.column.is_in(values),
                    Err(e) => {
                        errors.add(name, e.0);
                        continue;
                    }
                }
            } else {
                let value = match decode(&filter.value) {
                    Ok(value) => value,
                    Err(e) => {
                        errors.add(name, e.0);
                        continue;
                    }
                };
                match filter.op {
                    FilterOp::Eq => column.column.eq(value),
                    FilterOp::Gt => column.column.gt(value),
                    FilterOp::Gte => column.column.gte(value),
                    FilterOp::Lt => column.column.lt(value),
                    FilterOp::Lte => column.column.lte(value),
                    FilterOp::In => unreachable!(),
                }
            };
            condition = condition.add(expr);
        }
        condition
    }
}

/// Case-insensitive `LIKE` on a table-qualified column (safe with joined scopes),
/// cast to text so numeric, UUID or date title columns can be searched too.
///
/// Both sides are folded by the database's own `LOWER`, so an exact value always
/// matches itself (SQLite folds ASCII only; PostgreSQL folds Unicode).
pub(crate) fn search_match(backend: DbBackend, column: impl ColumnTrait, kind: FieldKind, pattern: &str) -> Expr {
    let column = Expr::col(column.as_column_ref());
    let pattern = Expr::val(pattern);
    // sea-query substitutes the backend's own placeholder token in custom SQL.
    match (backend, kind) {
        (DbBackend::Postgres, _) => {
            Expr::cust_with_exprs("LOWER(CAST($1 AS TEXT)) LIKE LOWER($2) ESCAPE '\\'", [column, pattern])
        }
        // MySQL: `CHAR`, not `TEXT`; a backslash inside a string literal is itself escaped;
        // UUIDs are `binary(16)`.
        (DbBackend::MySql, FieldKind::Uuid) => Expr::cust_with_exprs(
            "LOWER(CONCAT(SUBSTR(HEX(?),1,8),'-',SUBSTR(HEX(?),9,4),'-',SUBSTR(HEX(?),13,4),'-',SUBSTR(HEX(?),17,4),'-',SUBSTR(HEX(?),21))) LIKE LOWER(?) ESCAPE '\\\\'",
            [column.clone(), column.clone(), column.clone(), column.clone(), column, pattern],
        ),
        (DbBackend::MySql, _) => {
            Expr::cust_with_exprs("LOWER(CAST(? AS CHAR)) LIKE LOWER(?) ESCAPE '\\\\'", [column, pattern])
        }
        // SQLite stores UUIDs as 16-byte blobs: rebuild the canonical text form
        // the wire format shows (`xxxxxxxx-xxxx-…`), so displayed values match.
        (_, FieldKind::Uuid) => Expr::cust_with_exprs(
            "LOWER(SUBSTR(HEX(?),1,8)||'-'||SUBSTR(HEX(?),9,4)||'-'||SUBSTR(HEX(?),13,4)||'-'||SUBSTR(HEX(?),17,4)||'-'||SUBSTR(HEX(?),21)) LIKE LOWER(?) ESCAPE '\\'",
            [column.clone(), column.clone(), column.clone(), column.clone(), column, pattern],
        ),
        _ => Expr::cust_with_exprs("LOWER(CAST(? AS TEXT)) LIKE LOWER(?) ESCAPE '\\'", [column, pattern]),
    }
}

/// Escapes `%`, `_` and `\\` for `LIKE … ESCAPE '\\'`.
pub(crate) fn escape_like(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
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

impl<R: Resource> ResourceAdapter<R> {
    /// Keys of up to `batches` × `batch` rows of `select`, read in batches from
    /// one snapshot (concurrent edits cannot shift rows between batches), which
    /// is released before returning: policies and label lookups then run
    /// without holding a connection. Rows keep the query's own projection, so
    /// any `DISTINCT`/`ORDER BY` from `scope()` stays valid; only their keys are
    /// kept, so memory is bounded by one batch. Returns the keys in order and
    /// whether they are all the matching rows.
    async fn read_candidate_keys(
        &self,
        db: &sea_orm::DatabaseConnection,
        select: &sea_orm::Select<Entity<R>>,
        batch: u64,
        batches: u64,
    ) -> AppResult<(Vec<String>, bool)> {
        let snapshot = rustcms_core::db::begin_snapshot(db).await?;
        let mut keys = Vec::new();
        let mut complete = false;
        for index in 0..batches {
            let models = select.clone().limit(batch).offset(index * batch).all(&snapshot).await?;
            let exhausted = (models.len() as u64) < batch;
            keys.extend(models.iter().map(|model| self.key_of(model)));
            if exhausted {
                complete = true;
                break;
            }
        }
        if !complete {
            // Exactly at the budget: one more row decides.
            complete = select.clone().limit(1).offset(batches * batch).one(&snapshot).await?.is_none();
        }
        snapshot.commit().await?;
        Ok((keys, complete))
    }

    /// The records of `chunk` (at most [`LOAD_CHUNK`] keys) that still match
    /// `select` (scope, search, filters), in the given order, with their trusted
    /// records. Keys deleted or no longer matching since they were read are
    /// skipped, so results never include records outside the query.
    async fn load_ordered(
        &self,
        ctx: &Ctx,
        select: &sea_orm::Select<Entity<R>>,
        chunk: &[String],
    ) -> AppResult<Vec<(Model<R>, Data)>> {
        let values = self.decode_keys(chunk)?;
        let models = select.clone().filter(self.pk.is_in(values)).all(&ctx.db).await?;
        let mut loaded: HashMap<String, (Model<R>, Data)> = models
            .into_iter()
            .map(|model| {
                let record = Self::record(&model);
                (self.key_of(&model), (model, record))
            })
            .collect();
        Ok(chunk.iter().filter_map(|key| loaded.remove(key)).collect())
    }

    /// The outcome as the user may see it now: processed and denied keys only
    /// for records they can view (an action may have hidden or trashed its
    /// records; a replay comes later still). Keys they sent but which were not
    /// found stay in `missing`; hidden ones are left out, never moved there.
    async fn disclosed(&self, ctx: &Ctx, mut outcome: ActionOutcome) -> AppResult<ActionOutcome> {
        let keys: Vec<String> = outcome.processed.iter().chain(&outcome.denied).cloned().collect();
        let mut visible = HashSet::new();
        for chunk in keys.chunks(LOAD_CHUNK) {
            for (model, record) in self.models_in_scope(ctx, &ctx.db, chunk).await? {
                if self.policy.allows(ctx, ability::VIEW, Some(&record)).await {
                    visible.insert(self.key_of(&model));
                }
            }
        }
        outcome.processed.retain(|k| visible.contains(k));
        outcome.denied.retain(|k| visible.contains(k));
        Ok(outcome)
    }

    /// Whether a list shows `record`: viewable, and when trashed, also restorable
    /// or deletable for good (the trash is not readable on `view` alone).
    async fn listable(&self, ctx: &Ctx, record: &Data) -> bool {
        self.policy.allows(ctx, ability::VIEW, Some(record)).await
            && (!self.is_trashed(record)
                || self.policy.allows(ctx, ability::RESTORE, Some(record)).await
                || self.policy.allows(ctx, ability::FORCE_DELETE, Some(record)).await)
    }

    /// Envelope of a record whose write has committed. Building it must not
    /// turn the committed write into an error (a retry would repeat it): on
    /// failure, the key alone is returned, with no data.
    async fn committed_envelope(&self, ctx: &Ctx, model: Model<R>) -> Envelope {
        self.committed_key_envelope(ctx, self.key_of(&model)).await
    }

    /// [`Self::committed_envelope`] by key (replays of committed creates).
    async fn committed_key_envelope(&self, ctx: &Ctx, key: String) -> Envelope {
        // Bounded: a slow lookup must not let the HTTP timeout (or a client
        // giving up) hide the committed key and invite a duplicating retry.
        // Within the request's own deadline too (minus a margin to write the response).
        let budget = rustcms_core::http::request_deadline().map_or(POST_COMMIT_HYDRATION, |deadline| {
            deadline
                .saturating_duration_since(tokio::time::Instant::now())
                .saturating_sub(RESPONSE_MARGIN)
                .min(POST_COMMIT_HYDRATION)
        });
        let key_only = |key: String| Envelope {
            key,
            title: None,
            abilities: BTreeMap::new(),
            data: Data::new(),
            labels: BTreeMap::new(),
            trashed: false,
        };
        // Re-read in scope: a write may move the record out of the user's scope
        // (e.g. a new owner), and then the response shows nothing of it.
        let hydrate = async {
            match self.find_model(ctx, &ctx.db, &key).await {
                Ok(model) => Ok(self.envelopes(ctx, vec![model]).await?.remove(0)),
                Err(AppError::NotFound) => Ok(key_only(key.clone())),
                Err(err) => Err(err),
            }
        };
        let hydrated = tokio::time::timeout(budget, hydrate).await;
        match hydrated.unwrap_or_else(|_| Err(AppError::internal("post-commit hydration timed out"))) {
            Ok(envelope) => envelope,
            Err(err) => {
                tracing::warn!(resource = R::SLUG, %key, %err, "write committed; response without record data");
                key_only(key)
            }
        }
    }

    /// Opens a write transaction and a context whose [`Ctx::conn`] is that
    /// transaction, for policies and hooks.
    async fn begin(&self, ctx: &Ctx) -> AppResult<(Arc<sea_orm::DatabaseTransaction>, Ctx)> {
        // A busy database (SQLite) or a lock wait timing out: nothing written.
        let txn = rustcms_core::db::begin_write(&ctx.db).await.map_err(crate::idempotency::conflict)?;
        if ctx.idempotency_key.is_some() {
            crate::idempotency::bound_claim_wait(&txn).await?;
        }
        let txn = Arc::new(txn);
        let wctx = ctx.within(txn.clone());
        Ok((txn, wctx))
    }

    /// Ends a write opened with [`Self::begin`] without committing it.
    async fn abort(txn: Arc<sea_orm::DatabaseTransaction>, wctx: Ctx) -> AppResult<()> {
        drop(wctx);
        let txn = Arc::try_unwrap(txn)
            .map_err(|_| AppError::internal(format!("`{}`: the write transaction is still in use", R::SLUG)))?;
        txn.rollback().await?;
        Ok(())
    }

    /// Commits a write opened with [`Self::begin`].
    async fn commit(txn: Arc<sea_orm::DatabaseTransaction>, wctx: Ctx) -> AppResult<()> {
        drop(wctx);
        let txn = Arc::try_unwrap(txn)
            .map_err(|_| AppError::internal(format!("`{}`: the write transaction is still in use", R::SLUG)))?;
        txn.commit().await?;
        Ok(())
    }

    /// The record `key` for a write with `ability`: found in scope and allowed
    /// by the policy *before* locking (hidden or forbidden records are never
    /// locked, and answer at once), then locked and checked again, so ownership
    /// or tenancy changed meanwhile is seen.
    async fn find_authorized_locked(
        &self,
        ctx: &Ctx,
        txn: &sea_orm::DatabaseTransaction,
        key: &str,
        ability: &str,
        trashed: Trashed,
    ) -> AppResult<Model<R>> {
        let model = self.find_model_in(ctx, txn, key, trashed).await?;
        if !self.policy.allows(ctx, ability, Some(&Self::record(&model))).await {
            return Err(AppError::Forbidden);
        }
        if self.lock_rows(txn, std::slice::from_ref(&self.key_of(&model)), trashed).await?.is_empty() {
            return Err(AppError::NotFound);
        }
        let model = self.find_model_in(ctx, txn, key, trashed).await?;
        if !self.policy.allows(ctx, ability, Some(&Self::record(&model))).await {
            return Err(AppError::Forbidden);
        }
        Ok(model)
    }

    /// Locks the rows of `keys` until commit, by primary key only (scope joins or
    /// `DISTINCT` do not interfere). Callers pass only keys already found in
    /// scope and authorized, then re-check after locking. SQLite write
    /// transactions already hold the database lock.
    ///
    /// Returns the keys actually locked. A row deleted while waiting is skipped
    /// by PostgreSQL, so callers must use only these keys: a replacement row
    /// inserted under the same key afterwards is not locked.
    async fn lock_rows(
        &self,
        txn: &sea_orm::DatabaseTransaction,
        keys: &[String],
        trashed: Trashed,
    ) -> AppResult<HashSet<String>> {
        let backend = txn.get_database_backend();
        if backend == DbBackend::Sqlite {
            return Ok(keys.iter().cloned().collect());
        }
        let values = self.decode_keys(keys)?;
        if values.is_empty() {
            return Ok(HashSet::new());
        }
        // Key order: concurrent writes to overlapping records lock them in the
        // same order. A row trashed (or restored) meanwhile is not locked.
        let rows = |select: sea_orm::Select<Entity<R>>| match (&self.soft, trashed) {
            (Some((column, _, _)), Trashed::Without) => select.filter(column.is_null()),
            (Some((column, _, _)), Trashed::Only) => select.filter(column.is_not_null()),
            _ => select,
        };
        let ordered = Entity::<R>::find().filter(self.pk.is_in(values)).order_by(self.pk, Order::Asc);
        if backend != DbBackend::MySql {
            let locked = rows(ordered).lock_exclusive().all(txn).await?;
            return Ok(locked.iter().map(|m| self.key_of(m)).collect());
        }
        // InnoDB locks rows as it scans, not in the sorted order: one point lock
        // per key, in the order a plain read returns them.
        let mut locked = HashSet::new();
        for model in rows(ordered).all(txn).await? {
            let key = model.get(self.pk);
            if let Some(model) = rows(Entity::<R>::find().filter(self.pk.eq(key))).lock_exclusive().one(txn).await? {
                locked.insert(self.key_of(&model));
            }
        }
        Ok(locked)
    }

    /// Sets the soft delete marker to now, or clears it.
    fn set_marker(&self, active: &mut <Entity<R> as EntityTrait>::ActiveModel, trash: bool) -> AppResult<()> {
        let Some((column, name, kind)) = &self.soft else { return Ok(()) };
        let candidates = if trash {
            let now = chrono::Utc::now();
            let text = match kind {
                FieldKind::LocalDateTime => now.naive_utc().format("%Y-%m-%dT%H:%M:%S%.6f").to_string(),
                _ => now.to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
            };
            kind.decode(&serde_json::Value::String(text)).map_err(|e| AppError::internal(e.0))?
        } else {
            kind.nulls()
        };
        if candidates.into_iter().any(|v| active.try_set(*column, v).is_ok()) {
            Ok(())
        } else {
            Err(AppError::internal(format!("`{}`: cannot write the soft delete column `{name}`", R::SLUG)))
        }
    }

    fn info(&self) -> ResourceInfo {
        ResourceInfo {
            slug: R::SLUG,
            label: self.meta.label.clone(),
            plural_label: self.meta.plural_label.clone().unwrap_or_else(|| format!("{}s", self.meta.label)),
            icon: self.meta.icon.clone(),
            group: self.meta.group.clone(),
            title_field: self.meta.title_field.clone(),
        }
    }

    fn input_schema(&self, ctx: &Ctx, input: &Input, op: SchemaOperation) -> Option<InputSchema> {
        let field = self.field(&input.name)?;
        let is_key = input.name == self.pk_name;
        let (include, disabled) = match op {
            SchemaOperation::Create => {
                let natural = is_key && matches!(self.meta.key, KeyStrategy::Natural);
                (field.can_write(Operation::Create, ctx) && (!is_key || natural), false)
            }
            SchemaOperation::Edit => {
                // Write-only fields (e.g. a password) stay editable without showing a value.
                let writable = !is_key && field.can_write(Operation::Update, ctx);
                (field.can_read(ctx) || writable, !writable)
            }
            SchemaOperation::View => (field.can_read(ctx), true),
        };
        if !include {
            return None;
        }
        let write_only = !field.can_read(ctx);
        let mut rules = Vec::new();
        // Leaving a write-only input empty keeps the stored value: never required on edit.
        if field.required && !disabled && !(write_only && op == SchemaOperation::Edit) {
            rules.push(Rule::Required);
        }
        if let Some(&(items, length)) = self.key_values.get(&input.name).filter(|_| input.kind == InputType::KeyValue) {
            rules.push(Rule::MaxItems { value: items });
            rules.push(Rule::MaxLength { value: length });
        } else if let Some(max) = field.max_length {
            rules.push(Rule::MaxLength { value: max });
        }
        Some(InputSchema {
            kind: input.kind,
            name: input.name.clone(),
            label: input.label.clone().unwrap_or_else(|| field.display_label()),
            value_type: self.column(&input.name).kind.name(),
            nullable: self.column(&input.name).nullable,
            // A default is data: only for fields the user may read.
            default: (op == SchemaOperation::Create && !write_only)
                .then(|| self.column(&input.name).default.clone())
                .flatten(),
            disabled,
            write_only,
            rules,
            column_span: input.column_span,
            placeholder: input.placeholder.clone(),
            help: input.help.clone(),
            mode: input.mode,
            options: input.options.clone(),
            relationship: field.belongs_to.map(|resource| Relationship { resource }),
            widget: input.widget.clone(),
            props: input.props.clone(),
            media: field.media.as_ref().map(|rule| crate::schema::MediaSchema {
                multiple: rule.multiple,
                max_items: rule.max_items,
                accept: rule.accept.clone(),
                max_size: rule.max_size,
            }),
        })
    }

    fn components(&self, ctx: &Ctx, components: &[Component], op: SchemaOperation) -> Vec<ComponentSchema> {
        components
            .iter()
            .filter_map(|component| match component {
                Component::Input(input) => self.input_schema(ctx, input, op).map(ComponentSchema::Input),
                Component::Layout(layout) => {
                    let children = self.components(ctx, &layout.children, op);
                    (!children.is_empty()).then(|| {
                        ComponentSchema::Layout(LayoutSchema {
                            kind: layout.kind,
                            id: layout.id.clone(),
                            label: layout.label.clone(),
                            columns: layout.columns,
                            components: children,
                        })
                    })
                }
            })
            .collect()
    }

    async fn table_schema(&self, ctx: &Ctx) -> TableSchema {
        let readable = |name: &str| self.field(name).is_some_and(|f| f.can_read(ctx));
        let columns = self
            .table_ui
            .columns
            .iter()
            .filter(|c| readable(&c.name))
            .map(|c| {
                let field = self.field(&c.name).expect("validated at registration");
                ColumnSchema {
                    kind: c.kind,
                    name: c.name.clone(),
                    label: c.label.clone().unwrap_or_else(|| field.display_label()),
                    value_type: self.column(&c.name).kind.name(),
                    sortable: field.sortable,
                    searchable: field.searchable,
                    colors: c
                        .colors
                        .iter()
                        .map(|(value, color)| BadgeColor { value: value.clone(), color: color.clone() })
                        .collect(),
                    relationship: field.belongs_to.map(|resource| Relationship { resource }),
                }
            })
            .collect();
        let filters = self
            .table_ui
            .filters
            .iter()
            .filter(|f| readable(&f.name))
            .map(|f| {
                let field = self.field(&f.name).expect("validated at registration");
                FilterSchema {
                    kind: f.kind,
                    name: f.name.clone(),
                    label: f.label.clone().unwrap_or_else(|| field.display_label()),
                    value_type: self.column(&f.name).kind.name(),
                    options: f.options.clone(),
                    relationship: field.belongs_to.map(|resource| Relationship { resource }),
                }
            })
            .collect();
        let mut actions = Vec::new();
        for action in &self.actions {
            // Listed unless an explicit admission permission excludes the user;
            // availability per record is in each envelope's `abilities`.
            if action.required_permission.is_none_or(|p| ctx.can(p)) {
                actions.push(ActionSchema {
                    name: action.name,
                    label: action.label.clone().unwrap_or_else(|| humanize(action.name)),
                    bulk: action.bulk,
                    confirm: action.confirm.clone(),
                    color: action.color.clone(),
                    icon: action.icon.clone(),
                });
            }
        }
        TableSchema {
            columns,
            filters,
            default_sort: self.table_ui.default_sort.clone().filter(|s| readable(s.trim_start_matches('-'))),
            per_page_options: PER_PAGE_OPTIONS.to_vec(),
            searchable: self.fields.iter().any(|f| f.searchable && f.can_read(ctx)),
            actions,
            soft_deletes: self.soft.is_some(),
        }
    }
}

#[async_trait]
impl<R: Resource> DynResource for ResourceAdapter<R> {
    fn slug(&self) -> &'static str {
        R::SLUG
    }

    fn meta(&self) -> &ResourceMeta {
        &self.meta
    }

    fn info(&self) -> ResourceInfo {
        ResourceInfo::clone(&ResourceAdapter::info(self))
    }

    async fn allows(&self, ctx: &Ctx, ability: &str) -> bool {
        self.policy.allows(ctx, ability, None).await
    }

    async fn admits(&self, ctx: &Ctx, ability: &str) -> bool {
        self.policy.admits(ctx, ability).await
    }

    fn relations(&self) -> &[Relation] {
        &self.relations
    }

    fn field_link(&self, name: &str) -> Option<(Option<&'static str>, &'static str)> {
        self.field(name).map(|f| (f.belongs_to, self.column(name).kind.name()))
    }

    fn key_kind(&self) -> &'static str {
        self.pk_kind.name()
    }

    fn key_name(&self) -> &str {
        &self.pk_name
    }

    fn canonical_key(&self, key: &str) -> Option<String> {
        self.pk_kind.decode_key(key).ok().map(|value| encode_key(&value))
    }

    fn field_access(&self, ctx: &Ctx, name: &str) -> (bool, bool) {
        self.field(name).map_or((false, false), |f| (f.can_read(ctx), f.can_write(Operation::Create, ctx)))
    }

    async fn visible_key(&self, ctx: &Ctx, key: &str) -> AppResult<String> {
        match self.find(ctx, key).await {
            Ok(envelope) => Ok(envelope.key),
            // Denied or hidden: indistinguishable from missing.
            Err(AppError::Forbidden) => Err(AppError::NotFound),
            Err(err) => Err(err),
        }
    }

    async fn replay_create(&self, ctx: &Ctx, input: &Data) -> AppResult<Option<Envelope>> {
        let Some(claim) = self.create_claim(ctx, input) else { return Ok(None) };
        let Some(receipt) = crate::idempotency::existing(&ctx.db, &claim).await? else { return Ok(None) };
        let key = receipt["key"].as_str().ok_or_else(|| AppError::internal("create receipt without a key"))?;
        Ok(Some(self.committed_key_envelope(ctx, key.to_owned()).await))
    }

    fn permissions(&self) -> Vec<String> {
        let mut abilities: Vec<&str> =
            vec![ability::VIEW_ANY, ability::VIEW, ability::CREATE, ability::UPDATE, ability::DELETE];
        if self.soft.is_some() {
            abilities.extend([ability::RESTORE, ability::FORCE_DELETE]);
        }
        abilities.extend(self.actions.iter().map(|a| a.ability));
        abilities.sort_unstable();
        abilities.dedup();
        let mut permissions: Vec<String> = abilities.into_iter().map(|a| format!("{}.{a}", R::SLUG)).collect();
        // Explicit admission permissions are checked verbatim: declare them too.
        permissions.extend(self.actions.iter().filter_map(|a| a.required_permission).map(str::to_owned));
        permissions.sort_unstable();
        permissions.dedup();
        permissions
    }

    async fn schema(&self, ctx: &Ctx, op: SchemaOperation, key: Option<&str>) -> AppResult<ResourceSchema> {
        let mut abilities = BTreeMap::new();
        abilities.insert("viewAny", self.policy.allows(ctx, ability::VIEW_ANY, None).await);
        abilities.insert("create", self.policy.allows(ctx, ability::CREATE, None).await);
        if self.soft.is_some() {
            // Whether the trash can be listed; each record still decides.
            abilities.insert("restore", self.policy.admits(ctx, ability::RESTORE).await);
            abilities.insert("forceDelete", self.policy.admits(ctx, ability::FORCE_DELETE).await);
        }
        if let Some(key) = key {
            let admission = if op == SchemaOperation::Edit { ability::UPDATE } else { ability::VIEW };
            self.admit(ctx, admission).await?;
            let record = Self::record(&self.find_model(ctx, &ctx.db, key).await?);
            for name in [ability::VIEW, ability::UPDATE, ability::DELETE] {
                abilities.insert(name, self.policy.allows(ctx, name, Some(&record)).await);
            }
        }
        let allowed = match (op, key) {
            (SchemaOperation::Create, _) => abilities["create"],
            (SchemaOperation::Edit, Some(_)) => abilities[ability::UPDATE],
            (SchemaOperation::View, Some(_)) => abilities[ability::VIEW],
            (_, None) => abilities["viewAny"],
        };
        if !allowed {
            return Err(AppError::Forbidden);
        }
        Ok(ResourceSchema {
            schema_version: SCHEMA_VERSION,
            vocabulary_version: VOCABULARY_VERSION,
            resource: ResourceAdapter::info(self),
            operation: op,
            abilities,
            form: FormSchema {
                columns: self.form.columns,
                components: self.components(ctx, &self.form.components, op),
            },
            table: self.table_schema(ctx).await,
            relations: if op == SchemaOperation::View && key.is_some() {
                self.relation_schemas(ctx).await
            } else {
                Vec::new()
            },
        })
    }

    async fn lookup(&self, ctx: &Ctx, search: Option<&str>, keys: &[String]) -> AppResult<Vec<(String, String)>> {
        let Some(title) = &self.meta.title_field else { return Ok(Vec::new()) };
        if !self.field(title).is_some_and(|f| f.can_read(ctx))
            || !self.policy.allows(ctx, ability::VIEW_ANY, None).await
        {
            return Ok(Vec::new());
        }
        let title_column = self.column(title).column;
        let mut select = self.base(ctx, Trashed::Without);
        if !keys.is_empty() {
            select = select.filter(self.pk.is_in(self.decode_keys(keys)?));
        } else if let Some(search) = search.filter(|s| !s.is_empty()) {
            if search.chars().count() > self.limits.search_len {
                return Err(ValidationErrors::field("search", "is too long").into());
            }
            let pattern = format!("%{}%", escape_like(search));
            let kind = self.column(title).kind;
            select = select.filter(search_match(ctx.db.get_database_backend(), title_column, kind, &pattern));
        }
        // The policy may deny records the scope lets through: read in stable
        // batches until enough authorized options are found (bounded work).
        let select = select.order_by(title_column, Order::Asc).order_by(self.pk, Order::Asc);
        let (keys, complete) = self.read_candidate_keys(&ctx.db, &select, MAX_OPTIONS, MAX_OPTION_BATCHES).await?;
        let mut out = Vec::new();
        'chunks: for chunk in keys.chunks(LOAD_CHUNK) {
            for (model, record) in self.load_ordered(ctx, &select, chunk).await? {
                if self.policy.allows(ctx, ability::VIEW, Some(&record)).await {
                    out.push((self.key_of(&model), title_text(&record[title.as_str()])));
                    if out.len() as u64 == MAX_OPTIONS {
                        break 'chunks;
                    }
                }
            }
        }
        if out.len() as u64 == MAX_OPTIONS || complete {
            Ok(out)
        } else {
            // Budget spent before the candidates ran out: say so instead of
            // returning a list that silently misses valid choices.
            Err(ValidationErrors::field("search", "matches too many records to check; refine the search").into())
        }
    }

    async fn options(
        &self,
        ctx: &Ctx,
        field: &str,
        search: Option<&str>,
        keys: &[String],
        record: Option<&str>,
    ) -> AppResult<Vec<SelectOption>> {
        let Some(field) = self.field(field) else { return Err(AppError::NotFound) };
        let Some(target) = field.belongs_to else { return Err(AppError::NotFound) };
        // Options exist to show or fill the field: allowed for any operation the
        // user may perform with it. With `record` (an edit or view form), that
        // record is loaded in scope and the policy decides on it, exactly as
        // the form and the update itself do. The target's scope and policy
        // still decide which records appear.
        let usable = match record {
            Some(key) => {
                if !self.policy.admits(ctx, ability::UPDATE).await && !self.policy.admits(ctx, ability::VIEW).await {
                    return Err(AppError::Forbidden);
                }
                let record = Self::record(&self.find_model(ctx, &ctx.db, key).await?);
                (field.can_write(Operation::Update, ctx)
                    && self.policy.allows(ctx, ability::UPDATE, Some(&record)).await)
                    || (field.can_read(ctx) && self.policy.allows(ctx, ability::VIEW, Some(&record)).await)
            }
            None => {
                (field.can_read(ctx) && self.policy.allows(ctx, ability::VIEW_ANY, None).await)
                    || (field.can_write(Operation::Create, ctx) && self.policy.allows(ctx, ability::CREATE, None).await)
                    || (field.can_write(Operation::Update, ctx) && self.policy.allows(ctx, ability::UPDATE, None).await)
            }
        };
        if !usable {
            return Err(AppError::Forbidden);
        }
        if keys.len() > 100 {
            return Err(ValidationErrors::field("keys", "may not have more than 100 items").into());
        }
        let target =
            ctx.registry.get(target).ok_or_else(|| AppError::internal(format!("unknown resource `{target}`")))?;
        let kind = self.column(&field.name).kind;
        Ok(target
            .lookup(ctx, search, keys)
            .await?
            .into_iter()
            .map(|(key, label)| SelectOption { value: kind.from_query(&serde_json::Value::String(key)), label })
            .collect())
    }

    fn targets(&self) -> Vec<&'static str> {
        self.fields.iter().filter_map(|f| f.belongs_to).collect()
    }

    async fn admit_list(&self, ctx: &Ctx, query: &ListQuery) -> AppResult<()> {
        if !self.policy.allows(ctx, ability::VIEW_ANY, None).await {
            return Err(AppError::Forbidden);
        }
        let per_page = query.per_page.unwrap_or(DEFAULT_PER_PAGE).clamp(1, self.limits.per_page);
        let page = query.page.unwrap_or(1).max(1);
        // The SQL offset must fit (databases take a signed 64-bit offset).
        let offset_fits = (page - 1).checked_mul(per_page).is_some_and(|offset| i64::try_from(offset).is_ok());
        if !offset_fits {
            return Err(ValidationErrors::field("page", "is too large").into());
        }

        if let Some((field, _)) = &query.parent
            && !self.field(field).is_some_and(|f| f.can_read(ctx))
        {
            // The association itself is data the user may not read.
            return Err(AppError::Forbidden);
        }
        if query.trashed != Trashed::Without {
            if self.soft.is_none() {
                return Err(ValidationErrors::field("trashed", "is not supported by this resource").into());
            }
            if !self.policy.admits(ctx, ability::RESTORE).await && !self.policy.admits(ctx, ability::FORCE_DELETE).await
            {
                return Err(AppError::Forbidden);
            }
        }
        Ok(())
    }

    async fn list(&self, ctx: &Ctx, query: &ListQuery) -> AppResult<Page> {
        self.admit_list(ctx, query).await?;
        let per_page = query.per_page.unwrap_or(DEFAULT_PER_PAGE).clamp(1, self.limits.per_page);
        let page = query.page.unwrap_or(1).max(1);
        let mut errors = ValidationErrors::new();
        let mut select = self.base(ctx, query.trashed).filter(self.filter_condition(ctx, query, &mut errors));
        // Without `sort`, the table's default sort applies (when this user may use it),
        // so the order matches what the schema announces.
        let explicit = query.sort.is_some();
        let sort = query.sort.clone().or_else(|| self.table_ui.default_sort.clone());
        if let Some(sort) = sort.as_deref() {
            let (order, name) = match sort.strip_prefix('-') {
                Some(name) => (Order::Desc, name),
                None => (Order::Asc, sort),
            };
            if self.field(name).is_some_and(|f| f.sortable && f.can_read(ctx)) {
                select = select.order_by(self.column(name).column, order);
            } else if explicit {
                errors.add("sort", format!("cannot sort by `{name}`"));
            }
        }
        errors.into_result()?;
        let select = select.order_by(self.pk, Order::Asc);
        if !self.policy.view_depends_on_record() && query.trashed == Trashed::Without {
            // The same decision holds for every record: deny once, show nothing.
            if !self.policy.allows(ctx, ability::VIEW, None).await {
                return Ok(Page { data: Vec::new(), meta: PageMeta { page, per_page, total: 0, last_page: 1 } });
            }
            let paginator = select.paginate(&ctx.db, per_page);
            let totals = paginator.num_items_and_pages().await?;
            let models = paginator.fetch_page(page - 1).await?;
            return Ok(Page {
                data: self.envelopes(ctx, models).await?,
                meta: PageMeta {
                    page,
                    per_page,
                    total: totals.number_of_items,
                    last_page: totals.number_of_pages.max(1),
                },
            });
        }
        // Record-dependent visibility: check every matching record, so denied
        // ones are neither shown nor counted (bounded scan, fail closed).
        let (keys, complete) =
            self.read_candidate_keys(&ctx.db, &select, LIST_SCAN_BATCH, MAX_LIST_SCAN / LIST_SCAN_BATCH).await?;
        if !complete {
            let mut errors = ValidationErrors::new();
            errors.add_form("Too many matching records to check individually; narrow the search or filters.");
            return Err(errors.into());
        }
        let mut visible = Vec::new();
        let mut total = 0u64;
        let skip = (page - 1) * per_page;
        // Only the page's rows are kept; the rest are counted and dropped.
        for chunk in keys.chunks(LOAD_CHUNK) {
            for (model, record) in self.load_ordered(ctx, &select, chunk).await? {
                if self.listable(ctx, &record).await {
                    if total >= skip && (visible.len() as u64) < per_page {
                        visible.push(model);
                    }
                    total += 1;
                }
            }
        }
        Ok(Page {
            data: self.envelopes(ctx, visible).await?,
            meta: PageMeta { page, per_page, total, last_page: total.div_ceil(per_page).max(1) },
        })
    }

    async fn find(&self, ctx: &Ctx, key: &str) -> AppResult<Envelope> {
        self.admit(ctx, ability::VIEW).await?;
        let model = self.find_model(ctx, &ctx.db, key).await?;
        if !self.policy.allows(ctx, ability::VIEW, Some(&Self::record(&model))).await {
            return Err(AppError::Forbidden);
        }
        Ok(self.envelopes(ctx, vec![model]).await?.remove(0))
    }

    async fn create(&self, ctx: &Ctx, input: Data) -> AppResult<Envelope> {
        if !self.policy.allows(ctx, ability::CREATE, None).await {
            return Err(AppError::Forbidden);
        }
        // Through a relation, the foreign key is the relation's: never the client's.
        if let Some((_, field, _)) = &ctx.bound
            && input.contains_key(field)
        {
            return Err(ValidationErrors::field(field.as_str(), "is set by the relation").into());
        }
        // The request as sent is what a retry must repeat: a receipt is found
        // before validating, which may have changed (field permissions) since.
        let claim = self.create_claim(ctx, &input);
        let (txn, wctx) = self.begin(ctx).await?;
        if let Some(claim) = &claim
            && !crate::idempotency::acquire(txn.as_ref(), claim).await?
        {
            // Committed earlier: answer with that record, as the user sees it now.
            Self::abort(txn, wctx).await?;
            let receipt = crate::idempotency::receipt(&ctx.db, claim).await?;
            let key = receipt["key"].as_str().ok_or_else(|| AppError::internal("create receipt without a key"))?;
            return Ok(self.committed_key_envelope(ctx, key.to_owned()).await);
        }
        let mut input = input;
        let bound = ctx.bound.as_ref().map(|(_, field, parent)| (field.clone(), self.fk_value(field, parent)));
        if let Some((field, value)) = &bound {
            input.insert(field.clone(), value.clone());
        }
        // The usual rules apply to the bound key too (writable, typed, required).
        let input = self.accept_input(ctx, Operation::Create, input)?;
        let data = R::mutate_before_create(&wctx, &txn, input).await?;
        if let Some((field, value)) = &bound
            && data.get(field) != Some(value)
        {
            return Err(AppError::internal(format!("`{}`: a hook changed the relation key `{field}`", R::SLUG)));
        }
        // What is written, hooks included: references are checked last.
        self.check_written(&wctx, &txn, &data).await?;

        let mut active = <<Entity<R> as EntityTrait>::ActiveModel as ActiveModelBehavior>::new();
        self.apply(&mut active, &data, Operation::Create)?;
        // New records are active, whatever the model's or the column's default.
        self.set_marker(&mut active, false)?;
        match self.meta.key {
            KeyStrategy::DbGenerated => active.not_set(self.pk),
            KeyStrategy::AppGenerated(generate) => active
                .try_set(self.pk, generate())
                .map_err(|e| AppError::internal(format!("`{}` key: {e}", R::SLUG)))?,
            KeyStrategy::Natural if active.is_not_set(self.pk) => {
                return Err(ValidationErrors::field(self.pk_name.as_str(), "is required").into());
            }
            KeyStrategy::Natural => {}
        }
        let model = active.insert(txn.as_ref()).await.map_err(|e| self.map_db_err(e))?;
        R::after_save(&wctx, &txn, &model).await?;
        if let Some(claim) = &claim {
            crate::idempotency::complete(txn.as_ref(), claim, &serde_json::json!({ "key": self.key_of(&model) }))
                .await?;
        }
        Self::commit(txn, wctx).await?;
        Ok(self.committed_envelope(ctx, model).await)
    }

    async fn update(&self, ctx: &Ctx, key: &str, input: Data) -> AppResult<Envelope> {
        self.admit(ctx, ability::UPDATE).await?;
        let input = self.accept_input(ctx, Operation::Update, input)?;
        let (txn, wctx) = self.begin(ctx).await?;
        let model = self.find_authorized_locked(&wctx, &txn, key, ability::UPDATE, Trashed::Without).await?;
        let data = R::mutate_before_update(&wctx, &txn, &model, input).await?;
        self.check_written(&wctx, &txn, &data).await?;
        let model = if data.is_empty() {
            model
        } else {
            let mut active = model.into_active_model();
            self.apply(&mut active, &data, Operation::Update)?;
            active.update(txn.as_ref()).await.map_err(|e| self.map_db_err(e))?
        };
        R::after_save(&wctx, &txn, &model).await?;
        Self::commit(txn, wctx).await?;
        Ok(self.committed_envelope(ctx, model).await)
    }

    async fn delete(&self, ctx: &Ctx, key: &str) -> AppResult<()> {
        self.admit(ctx, ability::DELETE).await?;
        let (txn, wctx) = self.begin(ctx).await?;
        let model = self.find_authorized_locked(&wctx, &txn, key, ability::DELETE, Trashed::Without).await?;
        if self.soft.is_some() {
            // To the trash: related records are kept (no database cascade runs).
            let mut active = model.into_active_model();
            self.set_marker(&mut active, true)?;
            active.update(txn.as_ref()).await.map_err(|e| self.map_db_err(e))?;
        } else {
            Entity::<R>::delete_many()
                .filter(self.pk.eq(model.get(self.pk)))
                .exec(txn.as_ref())
                .await
                .map_err(|e| self.map_db_err(e))?;
        }
        Self::commit(txn, wctx).await?;
        Ok(())
    }

    async fn restore(&self, ctx: &Ctx, key: &str) -> AppResult<Envelope> {
        if self.soft.is_none() {
            return Err(AppError::NotFound);
        }
        self.admit(ctx, ability::RESTORE).await?;
        let (txn, wctx) = self.begin(ctx).await?;
        let model = self.find_authorized_locked(&wctx, &txn, key, ability::RESTORE, Trashed::Only).await?;
        // Back among active records, its relationships must point to records
        // the user can see, as when it was written.
        let record = Self::record(&model);
        let relations: Data = self
            .fields
            .iter()
            .filter(|f| f.belongs_to.is_some() || f.media.is_some() || f.rich_text)
            .filter_map(|f| record.get(&f.name).map(|v| (f.name.clone(), v.clone())))
            .collect();
        self.check_written(&wctx, &txn, &relations).await?;
        let mut active = model.into_active_model();
        self.set_marker(&mut active, false)?;
        let model = active.update(txn.as_ref()).await.map_err(|e| self.map_db_err(e))?;
        R::after_save(&wctx, &txn, &model).await?;
        Self::commit(txn, wctx).await?;
        Ok(self.committed_envelope(ctx, model).await)
    }

    async fn force_delete(&self, ctx: &Ctx, key: &str) -> AppResult<()> {
        if self.soft.is_none() {
            return Err(AppError::NotFound);
        }
        self.admit(ctx, ability::FORCE_DELETE).await?;
        let (txn, wctx) = self.begin(ctx).await?;
        let model = self.find_authorized_locked(&wctx, &txn, key, ability::FORCE_DELETE, Trashed::Only).await?;
        Entity::<R>::delete_many()
            .filter(self.pk.eq(model.get(self.pk)))
            .exec(txn.as_ref())
            .await
            .map_err(|e| self.map_db_err(e))?;
        R::after_force_delete(&wctx, &txn, &model).await?;
        Self::commit(txn, wctx).await?;
        Ok(())
    }

    async fn run_action(&self, ctx: &Ctx, name: &str, keys: &[String], data: Data) -> AppResult<ActionOutcome> {
        let action = self.actions.iter().find(|a| a.name == name).ok_or(AppError::NotFound)?;
        // Per-record policy checks are authoritative; an explicit admission
        // permission, when declared, refuses outright before them.
        if action.required_permission.is_some_and(|p| !ctx.can(p)) {
            return Err(AppError::Forbidden);
        }
        self.admit(ctx, action.ability).await?;
        let claim = ctx.idempotency_key.as_deref().map(|key| {
            let unique: BTreeSet<&String> = keys.iter().collect();
            let request = serde_json::json!({ "keys": unique, "data": data });
            crate::idempotency::Claim::new(&ctx.principal, R::SLUG, &format!("action:{name}"), key, &request)
        });
        let (txn, wctx) = self.begin(ctx).await?;
        if let Some(claim) = &claim
            && !crate::idempotency::acquire(txn.as_ref(), claim).await?
        {
            // The action ran with this key before: report that run, never run it again.
            Self::abort(txn, wctx).await?;
            let receipt = crate::idempotency::receipt(&ctx.db, claim).await?;
            let mut outcome: ActionOutcome = serde_json::from_value(receipt).map_err(AppError::internal)?;
            outcome.replayed = true;
            return self.disclosed(ctx, outcome).await;
        }
        // Lock only records the user may act on, then re-read just those under
        // the lock: a record that became visible meanwhile is never processed
        // unlocked.
        let mut outcome = ActionOutcome::default();
        let mut candidates = Vec::new();
        let mut seen = HashSet::new();
        for (model, record) in self.models_in_scope(&wctx, txn.as_ref(), keys).await? {
            let key = self.key_of(&model);
            seen.insert(key.clone());
            if self.policy.allows(&wctx, action.ability, Some(&record)).await {
                candidates.push(key);
            } else {
                outcome.denied.push(key);
            }
        }
        // Candidates are re-counted below; any not locked (deleted meanwhile)
        // or gone on reload end up in `missing`.
        seen.retain(|k| !candidates.contains(k));
        let locked = self.lock_rows(&txn, &candidates, Trashed::Without).await?;
        candidates.retain(|k| locked.contains(k));
        let mut allowed = Vec::new();
        for (model, record) in self.models_in_scope(&wctx, txn.as_ref(), &candidates).await? {
            let key = self.key_of(&model);
            seen.insert(key.clone());
            if self.policy.allows(&wctx, action.ability, Some(&record)).await {
                outcome.processed.push(key);
                allowed.push(model);
            } else {
                outcome.denied.push(key);
            }
        }
        outcome.missing =
            keys.iter().filter(|k| !seen.contains(*k)).cloned().collect::<BTreeSet<_>>().into_iter().collect();
        if !allowed.is_empty() {
            (action.handler)(&wctx, txn.as_ref(), allowed, data).await?;
        }
        outcome.processed.sort();
        outcome.denied.sort();
        if let Some(claim) = &claim {
            let result = serde_json::to_value(&outcome).map_err(AppError::internal)?;
            crate::idempotency::complete(txn.as_ref(), claim, &result).await?;
        }
        Self::commit(txn, wctx).await?;
        self.disclosed(ctx, outcome).await
    }

    async fn titles(&self, ctx: &Ctx, keys: &[String]) -> AppResult<BTreeMap<String, String>> {
        let Some(title) = &self.meta.title_field else {
            return Ok(BTreeMap::new());
        };
        if !self.field(title).is_some_and(|f| f.can_read(ctx))
            || !self.policy.allows(ctx, ability::VIEW_ANY, None).await
        {
            return Ok(BTreeMap::new());
        }
        let mut out = BTreeMap::new();
        for (model, record) in self.models_in_scope(ctx, &ctx.db, keys).await? {
            if self.policy.allows(ctx, ability::VIEW, Some(&record)).await {
                out.insert(self.key_of(&model), title_text(&record[title.as_str()]));
            }
        }
        Ok(out)
    }

    async fn keys_in_scope(&self, ctx: &Ctx, txn: &DatabaseTransaction, keys: &[String]) -> AppResult<HashSet<String>> {
        if !self.policy.allows(ctx, ability::VIEW_ANY, None).await {
            return Ok(HashSet::new());
        }
        let mut out = HashSet::new();
        for (model, record) in self.models_in_scope(ctx, txn, keys).await? {
            if self.policy.allows(ctx, ability::VIEW, Some(&record)).await {
                out.insert(self.key_of(&model));
            }
        }
        Ok(out)
    }
}
