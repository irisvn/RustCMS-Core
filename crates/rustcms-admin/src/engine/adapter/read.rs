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

    pub(super) fn filter_condition(&self, ctx: &Ctx, query: &ListQuery, errors: &mut ValidationErrors) -> Condition {
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
        outcome.processed.retain(|k| visible.contains(k));
        outcome.denied.retain(|k| visible.contains(k));
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
