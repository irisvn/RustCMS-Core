//! Generic CRUD engine: one monomorphized [`ResourceAdapter`] per resource,
//! erased to [`DynResource`] for the HTTP layer.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::marker::PhantomData;
use std::sync::Arc;

use async_trait::async_trait;
use rustcms_core::{AppError, AppResult, ValidationErrors};
use sea_orm::sea_query::Value;
use sea_orm::{
    ActiveModelBehavior, ActiveModelTrait, ColumnTrait, Condition, ConnectionTrait, DatabaseTransaction, DbErr,
    EntityName, EntityTrait, IdenStatic, IntoActiveModel, Iterable, ModelTrait, Order, PaginatorTrait,
    PrimaryKeyToColumn, QueryFilter, QueryOrder, SqlErr,
};

use crate::Data;
use crate::action::{Action, ActionOutcome};
use crate::context::Ctx;
use crate::field::{Field, Operation};
use crate::policy::{Policy, ability};
use crate::registry::DynResource;
use crate::resource::{Envelope, FilterOp, KeyStrategy, Limits, ListQuery, Page, PageMeta, Resource, ResourceMeta};
use crate::wire::{FieldKind, encode, encode_column, encode_key};

type Entity<R> = <R as Resource>::Entity;
type Model<R> = <Entity<R> as EntityTrait>::Model;
type Column<R> = <Entity<R> as EntityTrait>::Column;

const DEFAULT_PER_PAGE: u64 = 25;

struct ColumnInfo<C> {
    column: C,
    kind: FieldKind,
    nullable: bool,
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
    policy: Arc<dyn Policy>,
    actions: Vec<Action<Entity<R>>>,
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
                Some((column.as_str().to_owned(), ColumnInfo { column, kind, nullable: def.is_null() }))
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
        for constraint in &meta.constraints {
            for column in &constraint.columns {
                assert!(columns.contains_key(column), "resource `{slug}`: constraint column `{column}` is unknown");
            }
        }

        Self {
            table: Entity::<R>::default().table_name().to_owned(),
            meta,
            fields,
            columns,
            pk,
            pk_name,
            pk_kind,
            policy: R::policy(),
            actions: R::actions(),
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

    async fn find_model<C: ConnectionTrait>(&self, ctx: &Ctx, conn: &C, key: &str) -> AppResult<Model<R>> {
        let key = self.pk_kind.decode_key(key).map_err(|_| AppError::NotFound)?;
        R::scope(Entity::<R>::find(), ctx).filter(self.pk.eq(key)).one(conn).await?.ok_or(AppError::NotFound)
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
        let models = R::scope(Entity::<R>::find(), ctx).filter(self.pk.is_in(values)).all(conn).await?;
        Ok(models
            .into_iter()
            .map(|m| {
                let r = Self::record(&m);
                (m, r)
            })
            .collect())
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
            if value.is_null() && !column.nullable {
                errors.add(name.as_str(), "is required");
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
                    message.contains(&format!("\"{}\"", c.name)) || message.ends_with(&sqlite)
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
        let labels = self.labels(ctx, &readable, &models).await?;
        let title_readable = self.meta.title_field.as_ref().filter(|t| readable.iter().any(|f| &f.name == *t));
        let mut out = Vec::with_capacity(models.len());
        for (model, labels) in models.iter().zip(labels) {
            let record = Self::record(model);
            let mut abilities = BTreeMap::new();
            for name in [ability::VIEW, ability::UPDATE, ability::DELETE] {
                abilities.insert(name, self.policy.allows(ctx, name, Some(&record)).await);
            }
            out.push(Envelope {
                key: self.key_of(model),
                title: title_readable.map(|t| title_text(&record[t.as_str()])),
                abilities,
                data: readable.iter().map(|f| (f.name.clone(), record[f.name.as_str()].clone())).collect(),
                labels,
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
        if let Some(search) = query.search.as_deref().filter(|s| !s.is_empty()) {
            if search.chars().count() > self.limits.search_len {
                errors.add("search", format!("may not be longer than {} characters", self.limits.search_len));
            } else {
                let mut any = Condition::any();
                for field in self.fields.iter().filter(|f| f.searchable && f.can_read(ctx)) {
                    any = any.add(self.column(&field.name).column.contains(search));
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
            let decode = |v: &serde_json::Value| column.kind.decode(v).map(|mut c| c.swap_remove(0));
            let expr = if filter.op == FilterOp::In {
                let items = filter.value.as_array().map(Vec::as_slice).unwrap_or_default();
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

fn title_text(value: &serde_json::Value) -> String {
    value.as_str().map_or_else(|| value.to_string(), str::to_owned)
}

#[async_trait]
impl<R: Resource> DynResource for ResourceAdapter<R> {
    fn slug(&self) -> &'static str {
        R::SLUG
    }

    fn meta(&self) -> &ResourceMeta {
        &self.meta
    }

    fn targets(&self) -> Vec<&'static str> {
        self.fields.iter().filter_map(|f| f.belongs_to).collect()
    }

    async fn list(&self, ctx: &Ctx, query: &ListQuery) -> AppResult<Page> {
        if !self.policy.allows(ctx, ability::VIEW_ANY, None).await {
            return Err(AppError::Forbidden);
        }
        let per_page = query.per_page.unwrap_or(DEFAULT_PER_PAGE).clamp(1, self.limits.per_page);
        let page = query.page.unwrap_or(1).max(1);

        let mut errors = ValidationErrors::new();
        let mut select = R::scope(Entity::<R>::find(), ctx).filter(self.filter_condition(ctx, query, &mut errors));
        if let Some(sort) = query.sort.as_deref() {
            let (order, name) = match sort.strip_prefix('-') {
                Some(name) => (Order::Desc, name),
                None => (Order::Asc, sort),
            };
            if self.field(name).is_some_and(|f| f.sortable && f.can_read(ctx)) {
                select = select.order_by(self.column(name).column, order);
            } else {
                errors.add("sort", format!("cannot sort by `{name}`"));
            }
        }
        errors.into_result()?;
        let paginator = select.order_by(self.pk, Order::Asc).paginate(&ctx.db, per_page);
        let totals = paginator.num_items_and_pages().await?;
        let models = paginator.fetch_page(page - 1).await?;
        Ok(Page {
            data: self.envelopes(ctx, models).await?,
            meta: PageMeta { page, per_page, total: totals.number_of_items, last_page: totals.number_of_pages.max(1) },
        })
    }

    async fn find(&self, ctx: &Ctx, key: &str) -> AppResult<Envelope> {
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
        let input = self.accept_input(ctx, Operation::Create, input)?;
        let txn = rustcms_core::db::begin_write(&ctx.db).await?;
        self.check_relations(ctx, &txn, &input).await?;
        let data = R::mutate_before_create(ctx, &txn, input).await?;

        let mut active = <<Entity<R> as EntityTrait>::ActiveModel as ActiveModelBehavior>::new();
        self.apply(&mut active, &data, Operation::Create)?;
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
        let model = active.insert(&txn).await.map_err(|e| self.map_db_err(e))?;
        R::after_save(ctx, &txn, &model).await?;
        txn.commit().await?;
        Ok(self.envelopes(ctx, vec![model]).await?.remove(0))
    }

    async fn update(&self, ctx: &Ctx, key: &str, input: Data) -> AppResult<Envelope> {
        let input = self.accept_input(ctx, Operation::Update, input)?;
        let txn = rustcms_core::db::begin_write(&ctx.db).await?;
        let model = self.find_model(ctx, &txn, key).await?;
        if !self.policy.allows(ctx, ability::UPDATE, Some(&Self::record(&model))).await {
            return Err(AppError::Forbidden);
        }
        self.check_relations(ctx, &txn, &input).await?;
        let data = R::mutate_before_update(ctx, &txn, &model, input).await?;
        let model = if data.is_empty() {
            model
        } else {
            let mut active = model.into_active_model();
            self.apply(&mut active, &data, Operation::Update)?;
            active.update(&txn).await.map_err(|e| self.map_db_err(e))?
        };
        R::after_save(ctx, &txn, &model).await?;
        txn.commit().await?;
        Ok(self.envelopes(ctx, vec![model]).await?.remove(0))
    }

    async fn delete(&self, ctx: &Ctx, key: &str) -> AppResult<()> {
        let txn = rustcms_core::db::begin_write(&ctx.db).await?;
        let model = self.find_model(ctx, &txn, key).await?;
        if !self.policy.allows(ctx, ability::DELETE, Some(&Self::record(&model))).await {
            return Err(AppError::Forbidden);
        }
        Entity::<R>::delete_many()
            .filter(self.pk.eq(model.get(self.pk)))
            .exec(&txn)
            .await
            .map_err(|e| self.map_db_err(e))?;
        txn.commit().await?;
        Ok(())
    }

    async fn run_action(&self, ctx: &Ctx, name: &str, keys: &[String], data: Data) -> AppResult<ActionOutcome> {
        let action = self.actions.iter().find(|a| a.name == name).ok_or(AppError::NotFound)?;
        let txn = rustcms_core::db::begin_write(&ctx.db).await?;
        let found = self.models_in_scope(ctx, &txn, keys).await?;
        let mut outcome = ActionOutcome::default();
        let mut allowed = Vec::new();
        let mut seen = HashSet::new();
        for (model, record) in found {
            let key = self.key_of(&model);
            seen.insert(key.clone());
            if self.policy.allows(ctx, action.ability, Some(&record)).await {
                outcome.processed.push(key);
                allowed.push(model);
            } else {
                outcome.denied.push(key);
            }
        }
        outcome.missing =
            keys.iter().filter(|k| !seen.contains(*k)).cloned().collect::<BTreeSet<_>>().into_iter().collect();
        if !allowed.is_empty() {
            (action.handler)(ctx, &txn, allowed, data).await?;
        }
        txn.commit().await?;
        outcome.processed.sort();
        outcome.denied.sort();
        Ok(outcome)
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
