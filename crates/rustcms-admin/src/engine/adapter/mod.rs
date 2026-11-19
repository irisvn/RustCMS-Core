//! Generic CRUD engine: one monomorphized [`ResourceAdapter`] per resource,
//! erased to [`DynResource`] for the HTTP layer.

mod action_form;
pub(crate) mod aggregate;
mod audit;
mod export;
mod import;
pub(crate) mod infolist;
mod input;
mod many;
mod operations;
pub(crate) mod pivot;
mod react;
mod read;
mod references;
pub(crate) mod rows;
mod rules;
pub(crate) mod schema;
mod secrets;
mod write;

pub use import::ImportField;
pub(crate) use pivot::{DbConn, DynPivot, PivotAdapter};
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
use crate::engine::validate::{InputRule, Options, check_input, check_written};
use crate::field::{Field, Operation};
use crate::policy::{Policy, ability};
use crate::registry::DynResource;
use crate::resource::{
    Envelope, FilterOp, KeyStrategy, Limits, ListQuery, Page, PageMeta, Relation, RelationFilter, Resource,
    ResourceMeta, Trashed,
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
    /// Conditions of each action's form, by action name (ADR 0037).
    action_conditions: HashMap<&'static str, crate::engine::conditions::FormConditions>,
    relations: Vec<Relation>,
    form: Form,
    /// Key/value fields: most pairs and longest value.
    key_values: HashMap<String, (usize, usize)>,
    /// Encrypted fields (`Field::encrypted*`).
    secrets: HashMap<String, secrets::SecretField>,
    /// Static options of selects and checkbox lists, by field.
    static_options: HashMap<String, input::FieldOptions>,
    /// Bounds of number inputs, by field.
    number_bounds: HashMap<String, crate::engine::validate::Bounds>,
    /// Conditions under which inputs are shown (ADR 0037).
    conditions: crate::engine::conditions::FormConditions,
    /// JSON repeaters, by field (ADR 0040).
    repeaters: HashMap<String, crate::engine::repeater::Rows>,
    /// Relationship repeaters (ADR 0040).
    row_repeaters: Vec<rows::RowRepeater>,
    /// Inputs that react, in form order (ADR 0043).
    reactions: Vec<react::InputReaction>,
    /// Inputs with rules computed on the server, in form order (ADR 0044, part E).
    input_rules: Vec<rules::InputRuleSet>,
    /// The record title (ADR 0038).
    title: Option<crate::engine::title::TitleTemplate>,
    /// The view page's entries, when declared (ADR 0039).
    infolist: Option<crate::ui::Infolist>,
    /// Values computed for the view page (ADR 0039).
    computed: Vec<crate::ui::Computed<Entity<R>>>,
    table_ui: Table,
    limits: Limits,
    /// The panel's rich text nodes (set at boot by the registry).
    rich_nodes: std::sync::OnceLock<Arc<crate::rich_text::AppNodes>>,
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
            assert!(
                field.rich_nodes.is_empty() || field.rich_text,
                "resource `{slug}`: field `{}` accepts rich text nodes but is not `rich_text()`",
                field.name
            );
            assert!(
                field.collection.is_none() || field.media.is_some() || field.rich_text,
                "resource `{slug}`: field `{}` names a media collection but is not a media or rich text field",
                field.name
            );
            let unique: HashSet<_> = field.rich_nodes.iter().collect();
            assert!(
                unique.len() == field.rich_nodes.len(),
                "resource `{slug}`: field `{}` names a rich text node twice",
                field.name
            );
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
        let title = meta.title.as_deref().map(|template| {
            crate::engine::title::TitleTemplate::parse(template)
                .unwrap_or_else(|e| panic!("resource `{slug}`: the title `{template}` {e}"))
        });
        for name in title.iter().flat_map(|t| t.fields()) {
            assert!(seen.contains(name), "resource `{slug}`: title field `{name}` is not a field");
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
        if let Some(key) = &meta.import_key {
            assert!(seen.contains(key.as_str()), "resource `{slug}`: import key `{key}` is not a field");
            assert!(
                meta.constraints.iter().any(|c| c.columns == [key.clone()]),
                "resource `{slug}`: import key `{key}` needs a `ResourceMeta::unique` constraint on that column alone"
            );
        }
        for constraint in &meta.constraints {
            for column in &constraint.columns {
                assert!(columns.contains_key(column), "resource `{slug}`: constraint column `{column}` is unknown");
            }
        }

        // Encrypted fields: write operations on a text column, out of every
        // listing, search, sort, filter, key and default (ADR 0030).
        let mut secrets = HashMap::new();
        for field in &fields {
            let Some(kind) = field.secret else { continue };
            let name = field.name.as_str();
            let info = &columns[name];
            assert!(info.kind == FieldKind::String, "resource `{slug}`: encrypted field `{name}` needs a text column");
            assert!(
                !(field.sortable || field.searchable || field.filterable),
                "resource `{slug}`: encrypted field `{name}` cannot be sortable, searchable or filterable"
            );
            assert!(
                field.belongs_to.is_none()
                    // `max_items` bounds a map (it sets `key_value`).
                    && !(field.key_value && kind == crate::field::SecretKind::Text)
                    && field.media.is_none()
                    && !field.rich_text
                    && name != pk_name
                    && !title.iter().flat_map(|t| t.fields()).any(|f| f == name)
                    && meta.import_key.as_deref() != Some(name)
                    && !meta.constraints.iter().any(|c| c.columns.iter().any(|col| col == name)),
                "resource `{slug}`: encrypted field `{name}` cannot be a key, a title, a relation, media, rich \
                 text, key/value or part of a constraint"
            );
            assert!(!info.has_default, "resource `{slug}`: encrypted field `{name}` may not have a column default");
            let column = rustcms_core::crypto::EncryptedColumn::of::<Entity<R>>(
                info.column,
                rustcms_core::crypto::Binding::Column,
            );
            secrets.insert(field.name.clone(), secrets::SecretField { kind, column });
        }

        // A form without components is generated from the fields, keeping its options (width).
        let declared = R::form();
        let mut form = if declared.components.is_empty() {
            Form { width: declared.width, ..auto_form(&fields, &columns) }
        } else {
            declared
        };
        for input in form.inputs() {
            let secret = fields.iter().find(|f| f.name == input.name).and_then(|f| f.secret);
            let expected = match secret {
                Some(crate::field::SecretKind::Text) => Some(InputType::Secret),
                Some(crate::field::SecretKind::Map) => Some(InputType::SecretMap),
                None => None,
            };
            assert!(
                expected.map_or(!matches!(input.kind, InputType::Secret | InputType::SecretMap), |e| input.kind == e),
                "resource `{slug}`: input `{}` must be a SecretInput for `Field::encrypted`, a SecretMapInput for \
                 `Field::encrypted_map`, and those inputs only for them",
                input.name
            );
            assert!(
                secret.is_none() || input.options.is_empty(),
                "resource `{slug}`: encrypted field `{}` takes no options",
                input.name
            );
        }
        let relations = R::relations();
        let mut link_inputs = HashSet::new();
        for input in form.inputs() {
            if input.kind == InputType::BelongsToMany {
                // The links of a belongs-to-many relation (ADR 0035).
                let name = input.name.as_str();
                let relation = relations.iter().find(|r| r.name == name && r.is_many()).unwrap_or_else(|| {
                    panic!("resource `{slug}`: BelongsToManySelect `{name}` is not a belongs-to-many relation")
                });
                assert!(
                    !seen.contains(name),
                    "resource `{slug}`: BelongsToManySelect `{name}` shares its name with a field"
                );
                assert!(link_inputs.insert(name), "resource `{slug}`: BelongsToManySelect `{name}` appears twice");
                assert!(
                    !relation.pivot_fields.iter().any(|f| f.required),
                    "resource `{slug}`: relation `{name}` has required pivot columns; links are made in its \
                     relation manager, not with a BelongsToManySelect"
                );
                assert!(
                    input.options.is_empty() && input.widget.is_none(),
                    "resource `{slug}`: BelongsToManySelect `{name}` takes its options from the relation"
                );
                // Media rows are locked last by every write (ADR 0043): links to media
                // records would lock them in an earlier phase.
                assert!(
                    relation.target != crate::media::SLUG,
                    "resource `{slug}`: BelongsToManySelect `{name}` cannot link the media library; use a \
                     `Field::media_list` field"
                );
                continue;
            }
            // Relationship repeaters name a has-many relation (checked with the rows).
            if input.repeater.as_deref().is_some_and(|s| s.relationship) {
                continue;
            }
            assert!(
                seen.contains(input.name.as_str()),
                "resource `{slug}`: form input `{}` is not a field",
                input.name
            );
        }
        check_form_layout(&format!("resource `{slug}`"), &form);
        let static_options = input::static_options(slug, &mut form, &fields, &columns);
        let number_bounds = input::number_bounds(slug, &form, &columns);
        let conditions = input::form_conditions(slug, &form, &columns, &secrets, &fields);
        // Schemas send the conditions as the server compares them (UUIDs, decimals).
        let kind_of = |name: &str| input::condition_kind(&columns, &secrets, name);
        crate::engine::conditions::canonicalize(&mut form.components, &kind_of);
        let repeaters = input::json_repeaters(slug, &form, &columns, &secrets);
        let row_repeaters = rows::row_repeaters(slug, &form, &relations, &conditions);
        let reactions = react::reactions(slug, &form, &fields);
        let input_rules = rules::input_rules(slug, &form, &fields);
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
        for field in fields.iter().filter(|f| f.secret.is_none() && (f.key_value || inputs.contains(f.name.as_str()))) {
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
            if let Some(relation) = column.counts {
                // A count of a relation: the column is named by itself, never a field.
                assert!(
                    field(&column.name).is_none(),
                    "resource `{slug}`: count column `{}` has a field's name",
                    column.name
                );
                assert!(
                    relations.iter().any(|r| r.name == relation && !r.is_many()),
                    "resource `{slug}`: column `{}` counts `{relation}`, which is not a has-many relation",
                    column.name
                );
                assert!(
                    !column.count && column.cell.is_none(),
                    "resource `{slug}`: count column `{}` takes neither count() nor a custom cell",
                    column.name
                );
                continue;
            }
            assert!(field(&column.name).is_some(), "resource `{slug}`: table column `{}` is not a field", column.name);
            assert!(
                !column.count || columns[&column.name].kind == FieldKind::Json,
                "resource `{slug}`: column `{}` counts list items and needs a JSON field",
                column.name
            );
            assert!(
                !column.count || column.cell.is_none(),
                "resource `{slug}`: count column `{}` takes no custom cell",
                column.name
            );
            assert!(
                !secrets.contains_key(&column.name),
                "resource `{slug}`: encrypted field `{}` cannot be a table column",
                column.name
            );
        }
        let owner = format!("resource `{slug}`");
        for column in &mut table_ui.columns {
            let name = column.name.clone();
            let declared = !column.options.is_empty();
            if column.counts.is_some() {
                assert!(!declared && !column.raw, "{owner}: count column `{name}` takes no options");
                assert!(column.date_format.is_none(), "{owner}: count column `{name}` takes no date format");
                continue;
            }
            let field = field(&name).expect("checked above");
            let info = &columns[&name];
            assert!(!(column.count && (declared || column.raw)), "{owner}: count column `{name}` takes no options");
            if let Some(pattern) = &column.date_format {
                assert!(
                    matches!(column.kind, crate::ui::ColumnType::Text | crate::ui::ColumnType::Date) && !column.count,
                    "{owner}: column `{name}`: a date format needs a date or text column"
                );
                assert!(
                    column.cell.is_none(),
                    "{owner}: column `{name}`: a custom cell draws the value itself and takes no date format"
                );
                crate::engine::date_format::check(&format!("{owner}: column `{name}`"), pattern, Some(info.kind));
            }
            let labelled =
                matches!(column.kind, crate::ui::ColumnType::Text | crate::ui::ColumnType::Badge) && !column.count;
            assert!(!declared || labelled, "{owner}: column `{name}`: options need a text or badge column");
            assert!(!column.raw || labelled, "{owner}: column `{name}`: raw() needs a text or badge column");
            assert!(!(declared && column.raw), "{owner}: column `{name}` takes options or raw(), not both");
            assert!(
                !declared || field.belongs_to.is_none(),
                "{owner}: column `{name}` shows related records by their title and takes no options"
            );
            // Values compared as the wire sends them (an `i64` as a string).
            let what = format!("column `{name}`");
            for (value, _) in &mut column.colors {
                *value = input::canonical_option(&owner, &what, info, value);
            }
            for option in &mut column.options {
                option.value = input::canonical_option(&owner, &what, info, &option.value);
            }
            for (i, option) in column.options.iter().enumerate() {
                assert!(
                    !column.options[..i].iter().any(|o| o.value == option.value),
                    "{owner}: column `{name}` has option {} twice",
                    option.value
                );
            }
            // Declared labels name every colored value; the form's may not (a
            // color for a value only hooks write is kept).
            if declared {
                for (value, _) in &column.colors {
                    assert!(
                        column.options.iter().any(|o| o.value == *value),
                        "{owner}: column `{name}` colors {value}, which is not one of its options"
                    );
                }
            }
            // Without its own, the labels of the form's select or checkbox list.
            if labelled && !declared && !column.raw && field.belongs_to.is_none() {
                let input = form.inputs().into_iter().find(|i| {
                    i.name == name
                        && matches!(i.kind, InputType::Select | InputType::CheckboxList)
                        && !i.options.is_empty()
                });
                if let Some(input) = input {
                    column.options = input.options.clone();
                }
            }
        }
        for filter in &table_ui.filters {
            assert!(
                field(&filter.name).is_some_and(|f| f.filterable),
                "resource `{slug}`: filter `{}` needs a `.filterable()` field",
                filter.name
            );
            assert!(
                filter.none.is_none()
                    || filter.kind == crate::ui::FilterType::Select
                        && columns.get(&filter.name).is_some_and(|c| c.nullable)
                        && field(&filter.name).is_some_and(|f| f.belongs_to.is_none()),
                "resource `{slug}`: filter `{}` has a `none` option but is not a select filter with options on a \
                 nullable column (a `belongs_to` filter picks records)",
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
        let mut actions = R::actions();
        let mut action_names = HashSet::new();
        let mut action_conditions = HashMap::new();
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
                    ability::FORCE_DELETE,
                    ability::EXPORT,
                    ability::IMPORT
                ]
                .contains(&action.name),
                "resource `{slug}`: action name `{}` is reserved for CRUD abilities",
                action.name
            );
            action_form::check_action_form(slug, action.name, &action.form);
            action_conditions.insert(action.name, action_form::form_conditions(slug, action.name, &action.form));
        }
        for action in &mut actions {
            action_form::canonicalize_conditions(&mut action.form);
        }

        let mut infolist = R::infolist();
        let computed = R::computed();
        infolist::check_infolist(slug, &infolist, &computed, &fields, &columns, &relations);
        // Schemas send the infolist's conditions as the server compares them.
        let kind_of = |name: &str| input::condition_kind(&columns, &secrets, name);
        crate::engine::conditions::canonicalize(&mut infolist.components, &kind_of);

        let adapter = Self {
            // Without `infolist()`, the form read-only.
            infolist: Some(if infolist.is_empty() { infolist::from_form(&form, &fields, &columns) } else { infolist }),
            computed,
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
            relations,
            form,
            key_values,
            secrets,
            static_options,
            number_bounds,
            conditions,
            repeaters,
            row_repeaters,
            reactions,
            input_rules,
            action_conditions,
            title,
            table_ui,
            limits: Limits::default(),
            rich_nodes: std::sync::OnceLock::new(),
            _resource: PhantomData,
        };
        // Conditions never read an input whose visibility is computed (ADR 0044, part E).
        adapter.check_rules_unread();
        adapter
    }

    fn field(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.name == name)
    }

    fn column(&self, name: &str) -> &ColumnInfo<Column<R>> {
        &self.columns[name]
    }

    /// Every column of the record in wire format. Trusted: for policies and
    /// hooks only. Encrypted fields, and any other column holding an
    /// envelope, are `null`: neither plaintext nor ciphertext leaves here.
    fn record(&self, model: &Model<R>) -> Data {
        Column::<R>::iter()
            .map(|c| {
                let value = model.get(c);
                let sealed = matches!(&value, Value::String(Some(text)) if rustcms_core::crypto::looks_sealed(text));
                let value = if sealed || self.secrets.contains_key(c.as_str()) {
                    serde_json::Value::Null
                } else {
                    encode_column(c.def().get_column_type(), &value)
                };
                (c.as_str().to_owned(), value)
            })
            .collect()
    }

    /// The title template, when the user of `ctx` may read every field of it
    /// (ADR 0038: otherwise no title).
    fn readable_title(&self, ctx: &Ctx) -> Option<&crate::engine::title::TitleTemplate> {
        self.title.as_ref().filter(|t| t.fields().all(|f| self.field(f).is_some_and(|f| f.can_read(ctx))))
    }

    /// The encrypted columns of this resource (for `key:reencrypt`).
    pub(crate) fn encrypted_columns(&self) -> Vec<rustcms_core::crypto::EncryptedColumn> {
        self.secrets.values().map(|s| s.column.clone()).collect()
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
                let r = self.record(&m);
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
/// The layout rules of a form ([`check_layouts`]), and prefixes and suffixes
/// on text inputs only. `owner` names the form in messages (`resource `posts``).
pub(crate) fn check_form_layout(owner: &str, form: &Form) {
    check_layouts(owner, &form.components, None);
    assert!(!holds_entries(&form.components), "{owner}: a form holds inputs, not entries (entries go in `infolist()`)");
    for input in form.inputs().into_iter().filter(|i| i.prefix.is_some() || i.suffix.is_some()) {
        assert!(
            input.kind == InputType::TextInput,
            "{owner}: input `{}`: only text inputs have a prefix or a suffix",
            input.name
        );
    }
    for input in form.inputs().into_iter().filter(|i| !i.preview) {
        assert!(
            input.kind == InputType::FileUpload,
            "{owner}: input `{}`: only file uploads take `preview`",
            input.name
        );
    }
}

/// Whether `components` hold an entry, at any depth.
fn holds_entries(components: &[Component]) -> bool {
    components.iter().any(|c| match c {
        Component::Entry(_) => true,
        Component::Layout(layout) => holds_entries(&layout.children),
        Component::Input(_) => false,
    })
}

/// Whether `components` hold an input, at any depth.
pub(crate) fn holds_inputs(components: &[Component]) -> bool {
    components.iter().any(|c| match c {
        Component::Input(_) => true,
        Component::Layout(layout) => holds_inputs(&layout.children),
        Component::Entry(_) => false,
    })
}

/// Tabs hold only tabs, tabs sit only in tabs, and tab ids are unique per tabs;
/// one split at most, at the form's top level, holding its two panes; only
/// sections fold or are compact.
fn check_layouts(owner: &str, components: &[Component], parent: Option<LayoutType>) {
    let mut ids = HashSet::new();
    let mut splits = 0;
    for component in components {
        let Component::Layout(Layout { kind, id, children, collapsed, compact, .. }) = component else {
            assert!(parent != Some(LayoutType::Tabs), "{owner}: tabs may only contain `Tab`s");
            assert!(parent != Some(LayoutType::Split), "{owner}: a split holds only its panes");
            continue;
        };
        assert!(
            (*kind == LayoutType::Tab) == (parent == Some(LayoutType::Tabs)),
            "{owner}: a `Tab` belongs directly in `Tabs`, and `Tabs` hold only `Tab`s"
        );
        assert!(
            (*kind == LayoutType::Pane) == (parent == Some(LayoutType::Split)),
            "{owner}: a split holds only its panes"
        );
        if *kind == LayoutType::Split {
            splits += 1;
            assert!(parent.is_none() && splits == 1, "{owner}: one `Split` at most, at the form's top level");
        }
        assert!(
            *kind == LayoutType::Section || (collapsed.is_none() && !compact),
            "{owner}: only sections are collapsible or compact"
        );
        if let Some(id) = id {
            assert!(!id.is_empty() && ids.insert(id.clone()), "{owner}: tab `{id}` is empty or declared twice");
        }
        check_layouts(owner, children, Some(*kind));
    }
}

fn auto_form<C>(fields: &[Field], columns: &HashMap<String, ColumnInfo<C>>) -> Form {
    let inputs = fields
        .iter()
        .filter(|f| !f.guarded)
        .map(|f| {
            let kind = if let Some(secret) = f.secret {
                match secret {
                    crate::field::SecretKind::Text => InputType::Secret,
                    crate::field::SecretKind::Map => InputType::SecretMap,
                }
            } else if f.belongs_to.is_some() {
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
        .filter(|f| !f.guarded && f.secret.is_none() && columns[&f.name].kind != FieldKind::Json)
        .take(6)
        .map(|f| match columns[&f.name].kind {
            FieldKind::Bool => BooleanColumn::make(&f.name),
            FieldKind::Date | FieldKind::LocalDateTime | FieldKind::Instant => DateColumn::make(&f.name),
            _ => TextColumn::make(&f.name),
        })
        .collect()
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
