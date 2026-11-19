//! Reading: envelopes, labels, filters and search, record-checked lists.

use super::*;

impl<R: Resource> ResourceAdapter<R> {
    pub(super) async fn envelopes(&self, ctx: &Ctx, models: Vec<Model<R>>) -> AppResult<Vec<Envelope>> {
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
        // Every field of the title readable, or no title (ADR 0038).
        let title = self.title.as_ref().filter(|t| t.fields().all(|f| readable.iter().any(|r| r.name == f)));
        let mut out = Vec::with_capacity(models.len());
        for (model, labels) in models.iter().zip(labels) {
            let record = self.record(model);
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
                let offered = !trashed && action.shows_for(ctx, model);
                abilities.insert(action.name, offered && self.policy.allows(ctx, action.ability, Some(&record)).await);
            }
            // Every envelope (list, read, and the result of a write) carries data
            // only if the user may view the record; blind writes get key and abilities.
            out.push(Envelope {
                key: self.key_of(model),
                title: title.filter(|_| viewable).and_then(|t| t.render(&record)),
                abilities,
                data: if viewable {
                    let mut data: Data = readable
                        .iter()
                        .filter(|f| f.secret.is_none())
                        .map(|f| (f.name.clone(), record[f.name.as_str()].clone()))
                        .collect();
                    // Encrypted fields: their masked view, also for those who
                    // may only write them (state and revision, no key names).
                    for field in self.fields.iter().filter(|f| f.secret.is_some()) {
                        let names = field.can_read(ctx);
                        if names || field.can_write(Operation::Update, ctx) {
                            data.insert(field.name.clone(), self.secret_view(ctx, &field.name, model, names));
                        }
                    }
                    data
                } else {
                    Data::new()
                },
                labels: if viewable { labels } else { BTreeMap::new() },
                trashed,
                counts: BTreeMap::new(),
            });
        }
        Ok(out)
    }

    /// For each relation a count column names, the number of its records each
    /// viewable record has that the user may list ([`Self::counted_by`] of the
    /// relation's resource). A relation the user may not list gets no counts.
    /// Presentation: a failure leaves the counts out.
    pub(super) async fn add_counts(&self, ctx: &Ctx, envelopes: &mut [Envelope]) {
        let parents: Vec<String> =
            envelopes.iter().filter(|e| e.abilities.get(ability::VIEW) == Some(&true)).map(|e| e.key.clone()).collect();
        if parents.is_empty() {
            return;
        }
        let mut done = HashSet::new();
        for name in self.table_ui.columns.iter().filter_map(|c| c.counts) {
            if !done.insert(name) {
                continue;
            }
            let Some(relation) = self.relations.iter().find(|r| r.name == name) else { continue };
            let Some(target) = ctx.registry.get(relation.target) else { continue };
            if !target.allows(ctx, ability::VIEW_ANY).await {
                continue;
            }
            match target.counted_by(ctx, relation.field, &relation.query_filters(), &parents).await {
                Ok(counts) => {
                    for envelope in envelopes.iter_mut() {
                        if let Some(count) = counts.get(&envelope.key) {
                            envelope.counts.insert(name.to_owned(), *count);
                        }
                    }
                }
                // The relation's field is not readable: no counts.
                Err(AppError::Forbidden) => {}
                Err(err) => tracing::warn!(resource = R::SLUG, relation = name, %err, "relation counts unavailable"),
            }
        }
    }

    /// One `titles` lookup per relationship field per page (no N+1).
    pub(super) async fn labels(
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

    /// What a filtered relation requires of `field` (ADR 0044); `None` for a
    /// value the field does not read (refused at boot) or an unknown field.
    pub(super) fn relation_filter_condition(&self, field: &str, filter: &RelationFilter) -> Option<Condition> {
        let column = self.columns.get(field)?;
        Some(match filter {
            RelationFilter::Null => Condition::all().add(column.column.is_null()),
            RelationFilter::Eq(value) => {
                let decoded = column.kind.decode(value).ok()?.into_iter().next()?;
                Condition::all().add(column.column.eq(decoded))
            }
        })
    }

    pub(super) fn filter_condition(&self, ctx: &Ctx, query: &ListQuery, errors: &mut ValidationErrors) -> Condition {
        use sea_orm::sea_query::IntoCondition;
        let mut condition = Condition::all();
        // Children of one record (relation lists): an internal condition, not a client filter.
        if let Some((field, key)) = &query.parent {
            let column = self.column(field);
            match column.kind.decode(&column.kind.from_query(&serde_json::Value::String(key.clone()))) {
                Ok(values) if !values.is_empty() => condition = condition.add(column.column.eq(values[0].clone())),
                _ => errors.add_form("invalid parent key"),
            }
        }
        // A filtered relation's records (ADR 0044): checked at boot to decode.
        for (field, filter) in &query.relation_filters {
            match self.relation_filter_condition(field, filter) {
                Some(filter) => condition = condition.add(filter),
                None => errors.add_form("invalid parent key"),
            }
        }
        if let Some(through) = &query.through {
            condition = condition.add(self.pk.in_subquery(through.0.clone()));
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
                // A select filter with a "none" option (`SelectFilter::none`) reads its token as no value.
                let none = self.table_ui.filters.iter().any(|f| f.name == name && f.none.is_some());
                let is_none = |v: &serde_json::Value| none && v.as_str() == Some(crate::ui::NONE_OPTION);
                let wants_null = items.iter().any(is_none);
                match items.iter().filter(|v| !is_none(v)).map(decode).collect::<Result<Vec<_>, _>>() {
                    Ok(values) if wants_null && values.is_empty() => column.column.is_null().into_condition(),
                    Ok(values) if wants_null => {
                        Condition::any().add(column.column.is_null()).add(column.column.is_in(values)).into_condition()
                    }
                    Ok(values) => column.column.is_in(values).into_condition(),
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
                .into_condition()
            };
            condition = condition.add(expr);
        }
        condition
    }

    /// The rows a list query selects, in the list's order (sort or the table's
    /// default, then the key): scope, trash state, filters and search checked.
    pub(super) fn list_select(&self, ctx: &Ctx, query: &ListQuery) -> AppResult<sea_orm::Select<Entity<R>>> {
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
        Ok(select.order_by(self.pk, Order::Asc))
    }

    /// Keys of up to `batches` × `batch` rows of `select`, read in batches from
    /// one snapshot (concurrent edits cannot shift rows between batches), which
    /// is released before returning: policies and label lookups then run
    /// without holding a connection. Rows keep the query's own projection, so
    /// any `DISTINCT`/`ORDER BY` from `scope()` stays valid; only their keys are
    /// kept, so memory is bounded by one batch. Returns the keys in order and
    /// whether they are all the matching rows.
    pub(super) async fn read_candidate_keys(
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
    pub(super) async fn load_ordered(
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
                let record = self.record(&model);
                (self.key_of(&model), (model, record))
            })
            .collect();
        Ok(chunk.iter().filter_map(|key| loaded.remove(key)).collect())
    }

    /// The outcome as the user may see it now: processed and denied keys only
    /// for records they can view (an action may have hidden or trashed its
    /// records; a replay comes later still). Keys they sent but which were not
    /// found stay in `missing`; hidden ones are left out, never moved there.
    pub(super) async fn disclosed(&self, ctx: &Ctx, mut outcome: ActionOutcome) -> AppResult<ActionOutcome> {
        let keys: Vec<String> = outcome.processed.iter().chain(&outcome.denied).cloned().collect();
        let mut visible = HashSet::new();
        for chunk in keys.chunks(LOAD_CHUNK) {
            for (model, record) in self.models_in_scope(ctx, &ctx.db, chunk).await? {
                if self.policy.allows(ctx, ability::VIEW, Some(&record)).await {
                    visible.insert(self.key_of(&model));
                }
            }
        }
        let processed = outcome.processed.len();
        outcome.processed.retain(|k| visible.contains(k));
        outcome.denied.retain(|k| visible.contains(k));
        // The handler's message and page speak of the records it processed:
        // only for a user who may still view all of them (also on a replay).
        if outcome.processed.len() != processed {
            outcome.message = None;
            outcome.open = None;
        }
        Ok(outcome)
    }

    /// Whether a list shows `record`: viewable, and when trashed, also restorable
    /// or deletable for good (the trash is not readable on `view` alone).
    pub(super) async fn listable(&self, ctx: &Ctx, record: &Data) -> bool {
        self.policy.allows(ctx, ability::VIEW, Some(record)).await
            && (!self.is_trashed(record)
                || self.policy.allows(ctx, ability::RESTORE, Some(record)).await
                || self.policy.allows(ctx, ability::FORCE_DELETE, Some(record)).await)
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
