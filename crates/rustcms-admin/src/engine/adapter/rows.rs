//! Relationship repeaters, the child's side (ADR 0040): the rows a parent's
//! write changes, written in the parent's transaction with the child's own
//! checks (scope, view policy under the lock, field rules, references,
//! policy per record, hooks, history). A row may hold repeaters of its own
//! (ADR 0044, part C): their changes are written with the row, level by level.

use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

use super::*;

/// The changes of the relationship repeaters a row holds, by index in its
/// repeater's nested repeaters (`RowRules::nested`).
pub(crate) type Nested = Vec<(usize, RowChange)>;

/// What a relationship repeater changes, its shape already checked.
#[derive(Debug, Clone, Default)]
pub(crate) struct RowChange {
    /// New rows: `(ref, data, the changes of their own repeaters)`.
    pub(crate) create: Vec<(String, Data, Nested)>,
    /// Changed rows: `(key, data, the changes of their own repeaters)`.
    pub(crate) update: Vec<(String, Data, Nested)>,
    /// Removed rows (to the trash with soft deletes).
    pub(crate) delete: Vec<String>,
    /// Every row the user sees after the change, in order: stored rows by
    /// key, new rows by their reference (`{"new": ref}` on the wire).
    pub(crate) order: Option<Vec<RowId>>,
    /// Remove every row the user sees (the write hides the repeater, ADR
    /// 0044); the parts above are then empty.
    pub(crate) clear: bool,
    /// With `clear`, for the rows of a removed row (ADR 0044, part C): a row
    /// the user cannot see refuses the write (403) rather than being kept.
    pub(crate) strict: bool,
}

impl RowChange {
    /// The rows this change names, at every level.
    pub(crate) fn size(&self) -> usize {
        let nested = |rows: &[(String, Data, Nested)]| {
            rows.iter().flat_map(|(_, _, n)| n.iter()).map(|(_, c)| c.size()).sum::<usize>()
        };
        self.create.len() + self.update.len() + self.delete.len() + nested(&self.create) + nested(&self.update)
    }

    fn removing(strict: bool) -> Self {
        Self { clear: true, strict, ..Self::default() }
    }
}

/// A row named in a change's order.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum RowId {
    /// A stored row, by its (canonical) key.
    Key(String),
    /// A row this change creates, by its reference.
    New(String),
}

/// Most rows one write changes, at every level, removals included (ADR 0044).
pub(crate) const MAX_WRITE_ROWS: usize = 1000;

/// The rows a write has changed so far, at every level (ADR 0044, part C):
/// those it names, then the rows removed with their parent row or because
/// their repeater was hidden.
#[derive(Debug, Default)]
pub(crate) struct RowBudget(AtomicUsize);

impl RowBudget {
    pub(crate) fn new(named: usize) -> Self {
        Self(AtomicUsize::new(named))
    }

    /// Counts `rows` more; 422 beyond [`MAX_WRITE_ROWS`].
    pub(crate) fn spend(&self, rows: usize) -> AppResult<()> {
        let used = self.0.fetch_add(rows, AtomicOrdering::Relaxed) + rows;
        if used > MAX_WRITE_ROWS { Err(too_many_rows()) } else { Ok(()) }
    }
}

/// The form error of a write changing more than [`MAX_WRITE_ROWS`] rows
/// (`too_many_rows_changed`): the form's, whatever the level it is found at.
const TOO_MANY_ROWS: &str = "changes too many rows at once";

/// A write changing more than [`MAX_WRITE_ROWS`] rows.
pub(crate) fn too_many_rows() -> AppError {
    let mut errors = ValidationErrors::new();
    errors.add_form(TOO_MANY_ROWS);
    errors.into()
}

/// Where the rows belong.
pub(crate) struct RowTarget<'a> {
    /// The parent resource (its slug) and the relation's name (error paths).
    pub(crate) parent_slug: &'a str,
    pub(crate) relation: &'a str,
    /// The child's field holding the parent's key, and that key.
    pub(crate) field: &'a str,
    pub(crate) parent_key: &'a str,
    /// The child's integer field holding each row's position.
    pub(crate) order_column: Option<&'a str>,
    /// The child fields the repeater's rows edit: anything else sent is refused.
    pub(crate) inputs: &'a [String],
    /// The repeater's bounds on the parent's rows (checked when rows are added or removed).
    pub(crate) min_items: Option<u32>,
    pub(crate) max_items: Option<u32>,
    /// Whether rows may be added and removed (`addable`, `deletable`).
    pub(crate) addable: bool,
    pub(crate) deletable: bool,
    /// The rows' conditions, components and nested repeaters (ADR 0044).
    pub(crate) rules: &'a RowRules,
    /// The record's form as the rows' `when_root` reads it.
    pub(crate) root: &'a Data,
    /// The enclosing row as the rows' `when_parent` reads it (nested
    /// repeaters, ADR 0044 part C).
    pub(crate) parent: Option<&'a Data>,
    /// The keys of the levels above the rows, the record's first and the
    /// parent's last: what `inherit` fills in.
    pub(crate) ancestors: &'a [String],
    /// A filtered relation's requirements (ADR 0044): rows outside them are
    /// not the relation's; new rows get them.
    pub(crate) filters: &'a [(&'static str, RelationFilter)],
    /// The rows the write changes, at every level.
    pub(crate) budget: &'a RowBudget,
    /// What holds the rows (the record, or the enclosing row) as the write
    /// leaves it, and whether it is created: what `validate_rows` reads.
    pub(crate) holder: &'a Data,
    pub(crate) holder_op: Operation,
    /// The repeater's rule over its final rows (ADR 0044, part D).
    pub(crate) validate: Option<&'a crate::ui::RowsValidator>,
}

/// What changed, for the parent's history.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct RowsOutcome {
    pub(crate) created: Vec<String>,
    pub(crate) updated: Vec<String>,
    pub(crate) deleted: Vec<String>,
    pub(crate) reordered: bool,
    /// The media the written rows refer to (after the child's hooks), named
    /// by row (`options.n1.image`, `blanks.n1.choices.n2.image`): checked with
    /// the parent's, last (ADR 0044).
    pub(crate) media: Vec<crate::engine::repeater::MediaReference>,
}

impl RowsOutcome {
    pub(crate) fn is_empty(&self) -> bool {
        self.created.is_empty() && self.updated.is_empty() && self.deleted.is_empty() && !self.reordered
    }
}

/// What every row of one repeater's change is written with, computed once:
/// the rows' conditions and the relation's values (ADR 0044): its filters'
/// and the ancestors' keys it inherits.
struct RowWrite<'a> {
    level: super::input::RowLevel<'a>,
    fixed: Vec<(&'a str, serde_json::Value)>,
}

/// A row written, before its own repeaters, `after_save` and history.
struct WrittenRow<R: Resource> {
    model: Model<R>,
    /// The history event (`created`, `updated`) and the row before it, when
    /// the row itself was written.
    written: Option<(&'static str, Option<serde_json::Map<String, serde_json::Value>>)>,
    media: Vec<crate::engine::repeater::MediaReference>,
    /// The row as its conditions leave it: what its rows' `when_parent` reads.
    values: Data,
    /// The row's inputs hidden before and after the user's changes.
    hidden_before: BTreeSet<String>,
    hidden: BTreeSet<String>,
}

/// Whether `record` is one of the relation's rows: the parent's (`field`
/// holds `parent`), with the filters' and inherited values (`fixed`,
/// canonical) (ADR 0044).
fn in_relation(record: &Data, field: &str, parent: &serde_json::Value, fixed: &[(&str, serde_json::Value)]) -> bool {
    record.get(field) == Some(parent)
        && fixed.iter().all(|(f, v)| record.get(*f).unwrap_or(&serde_json::Value::Null) == v)
}

/// The conditions a row of `target` is written under: the repeater's (ADR 0044).
fn row_level<'a>(target: &RowTarget<'a>) -> super::input::RowLevel<'a> {
    super::input::RowLevel {
        conditions: &target.rules.conditions,
        inputs: crate::engine::repeater::row_inputs(&target.rules.components),
        root: Some(target.root),
        parent: target.parent,
        repeaters: &target.rules.json,
    }
}

/// `errors` of row `id` moved below `relation.id.` (`quantity` → `lines.8.quantity`).
fn below(relation: &str, id: &str, err: AppError) -> AppError {
    match err {
        AppError::Validation(errors) => {
            let mut out = ValidationErrors::new();
            for (field, messages) in errors.errors {
                for message in messages {
                    out.add(format!("{relation}.{id}.{field}"), message);
                }
            }
            for message in errors.form_errors {
                // A write changing too many rows is the form's, whatever the level.
                if message == TOO_MANY_ROWS {
                    out.add_form(message);
                } else {
                    out.add(format!("{relation}.{id}"), message);
                }
            }
            AppError::Validation(out)
        }
        other => other,
    }
}

/// `media` of row `id`, named below `relation.id.`.
fn media_below(
    relation: &str,
    id: &str,
    media: Vec<crate::engine::repeater::MediaReference>,
) -> impl Iterator<Item = crate::engine::repeater::MediaReference> {
    let prefix = format!("{relation}.{id}.");
    media.into_iter().map(move |(at, rule, ids)| (format!("{prefix}{at}"), rule, ids))
}

impl<R: Resource> ResourceAdapter<R> {
    /// Writes `change` to the children of `target`, in `txn` (the parent's
    /// write, `ctx` its context). Rows named by key are locked in key order
    /// through the child's scope and view policy (re-checked under the lock);
    /// one not visible or not a child of this parent is a 422 at
    /// `relation.key`. Then deletes (each row's own rows first), updates and
    /// creates (each row, then its own rows, then its `after_save`), positions.
    pub(super) async fn write_rows_in(
        &self,
        ctx: &Ctx,
        txn: &Arc<sea_orm::DatabaseTransaction>,
        target: RowTarget<'_>,
        change: RowChange,
    ) -> AppResult<RowsOutcome> {
        let relation = target.relation;
        if change.clear {
            return self.clear_rows(ctx, txn, &target, change.strict).await;
        }
        // Rows edit the repeater's inputs only, not every writable field of the child.
        let mut undeclared = ValidationErrors::new();
        let sent = change.update.iter().map(|(k, d, _)| (k, d)).chain(change.create.iter().map(|(r, d, _)| (r, d)));
        for (id, data) in sent {
            for name in data.keys().filter(|n| !target.inputs.contains(n)) {
                undeclared.add(format!("{relation}.{id}.{name}"), "is not allowed");
            }
        }
        undeclared.into_result()?;
        // Declared options, checked after admission and before any key is read.
        if !target.addable && !change.create.is_empty() {
            return Err(ValidationErrors::field(relation, "rows cannot be added here").into());
        }
        if !target.deletable && !change.delete.is_empty() {
            return Err(ValidationErrors::field(relation, "rows cannot be removed here").into());
        }
        let parent_value = self.fk_value(target.field, target.parent_key);
        // Once per repeater: the relation's values (ADR 0044) and the rows' conditions.
        let rows = RowWrite { level: row_level(&target), fixed: self.relation_values(&target)? };
        let parent_filter = self.parent_filter(&target, &parent_value, &rows.fixed)?;
        let fixed = &rows.fixed;
        let mut named: Vec<String> =
            change.update.iter().map(|(k, _, _)| k.clone()).chain(change.delete.clone()).collect();
        named.sort();
        named.dedup();
        // One locking pass over this parent's rows (ADR 0041; ADR 0044: one per
        // parent row): the named rows that are its children, and with an order
        // every live child (bounded first).
        let mut candidates: Vec<String> = if named.is_empty() {
            Vec::new()
        } else {
            Entity::<R>::find()
                .filter(parent_filter.clone())
                .filter(self.pk.is_in(self.decode_keys(&named)?))
                .all(txn.as_ref())
                .await?
                .iter()
                .map(|m| self.key_of(m))
                .collect()
        };
        let ordering = target.order_column.is_some() && change.order.is_some();
        if ordering {
            let live = self
                .live_rows(&parent_filter)
                .order_by_asc(self.pk)
                .limit(MAX_ORDERED_ROWS as u64 + 1)
                .all(txn.as_ref())
                .await?;
            if live.len() > MAX_ORDERED_ROWS {
                return Err(ValidationErrors::field(relation, "has too many rows to order here").into());
            }
            candidates.extend(live.iter().map(|m| self.key_of(m)));
        }
        candidates.sort();
        candidates.dedup();
        let locked = self.locked_visible_rows(ctx, txn, &candidates).await?;
        let mut errors = ValidationErrors::new();
        let mut models = HashMap::new();
        for key in &named {
            let found = if locked.contains(key) {
                // Gone meanwhile: not one of the rows; any other error is the database's.
                match self.find_model_in(ctx, txn.as_ref(), key, Trashed::Without).await {
                    Ok(model) => Some(model),
                    Err(AppError::NotFound) => None,
                    Err(err) => return Err(err),
                }
            } else {
                None
            };
            match found {
                // Rechecked under the lock: still this parent's, and still the relation's.
                Some(model) if in_relation(&self.record(&model), target.field, &parent_value, fixed) => {
                    models.insert(key.clone(), model);
                }
                _ => errors.add(format!("{relation}.{key}"), "is not one of the rows"),
            }
        }
        errors.into_result()?;

        let mut outcome = RowsOutcome::default();
        // Deletes: the child's policy on each locked row; its own rows first (ADR 0044).
        for key in &change.delete {
            let model = models.remove(key).expect("locked above");
            if !self.policy.allows(ctx, ability::DELETE, Some(&self.record(&model))).await {
                return Err(AppError::Forbidden);
            }
            self.remove_row(ctx, txn, &target, model).await.map_err(|e| below(relation, key, e))?;
            outcome.deleted.push(key.clone());
        }
        // Updates: the child's field rules, conditions, hooks, references, history.
        for (key, data, nested) in change.update {
            let model = models.remove(&key).expect("locked above");
            if !self.policy.allows(ctx, ability::UPDATE, Some(&self.record(&model))).await {
                return Err(AppError::Forbidden);
            }
            if data.contains_key(target.field) || target.order_column.is_some_and(|c| data.contains_key(c)) {
                return Err(ValidationErrors::field(format!("{relation}.{key}"), "is not allowed").into());
            }
            let written = async {
                let row = self.update_row(ctx, txn, model, data, target.field, &rows).await?;
                self.finish_row(ctx, txn, &target, Operation::Update, row, nested).await
            }
            .await
            .map_err(|e| below(relation, &key, e))?;
            let (changed, media) = written;
            outcome.media.extend(media_below(relation, &key, media));
            if changed {
                outcome.updated.push(key);
            }
        }
        // Creates, with the parent in context; refs get their keys.
        let mut refs = HashMap::new();
        let mut bound = ctx.clone();
        bound.bound = Some((target.parent_slug.to_owned(), target.field.to_owned(), target.parent_key.to_owned()));
        if !change.create.is_empty() && !self.policy.allows(&bound, ability::CREATE, None).await {
            return Err(AppError::Forbidden);
        }
        // New rows go after every row of the parent (an order may move them next);
        // the largest position is read only when there are rows to place.
        let mut last = match target.order_column {
            Some(column) if !change.create.is_empty() => {
                self.largest_position(txn, &target, column, &parent_value).await?
            }
            _ => 0,
        };
        for (reference, data, nested) in change.create {
            if data.contains_key(target.field) || target.order_column.is_some_and(|c| data.contains_key(c)) {
                return Err(ValidationErrors::field(format!("{relation}.{reference}"), "is not allowed").into());
            }
            let position = match target.order_column {
                Some(column) => {
                    last = self.next_position(relation, column, last)?;
                    Some((column, last))
                }
                None => None,
            };
            let (key, media) = async {
                let parent = (target.field, &parent_value);
                let row = self.create_row(&bound, txn, data, parent, position, &rows).await?;
                let key = self.key_of(&row.model);
                let (_, media) = self.finish_row(&bound, txn, &target, Operation::Create, row, nested).await?;
                Ok::<_, AppError>((key, media))
            }
            .await
            .map_err(|e| below(relation, &reference, e))?;
            outcome.media.extend(media_below(relation, &reference, media));
            refs.insert(reference, key.clone());
            outcome.created.push(key);
        }
        // The repeater's bounds hold on the parent's rows, whatever the client checked.
        let min = target.min_items.filter(|_| !outcome.deleted.is_empty());
        let max = target.max_items.filter(|_| !outcome.created.is_empty());
        if min.is_some() || max.is_some() {
            let count = self.live_rows(&parent_filter).count(txn.as_ref()).await?;
            if let Some(min) = min.filter(|m| count < u64::from(*m)) {
                return Err(ValidationErrors::field(relation, format!("must have at least {min} items")).into());
            }
            if let Some(max) = max.filter(|m| count > u64::from(*m)) {
                return Err(ValidationErrors::field(relation, format!("may not have more than {max} items")).into());
            }
        }
        if let (Some(column), Some(order)) = (target.order_column, change.order) {
            let deleted: HashSet<&String> = outcome.deleted.iter().collect();
            let survivors: HashSet<String> = locked.iter().filter(|k| !deleted.contains(k)).cloned().collect();
            outcome.reordered =
                self.reorder_rows(ctx, txn, &target, column, &parent_value, &order, &refs, &survivors).await?;
        }
        // The rule over the final rows: on a new holder, or when this save changed them (ADR 0044, part D).
        if let Some(rule) = target.validate
            && (target.holder_op == Operation::Create || !outcome.is_empty())
        {
            self.check_rows(ctx, txn, &target, &parent_filter, &refs, rule).await?;
        }
        Ok(outcome)
    }

    /// Runs `rule` on every live row of the relation (filtered, not trashed,
    /// also those the user cannot see), in the order column's order, as the
    /// write left them; each named by its key, or the ref of a row this write
    /// created. Its errors are at the repeater's path.
    async fn check_rows(
        &self,
        ctx: &Ctx,
        txn: &Arc<sea_orm::DatabaseTransaction>,
        target: &RowTarget<'_>,
        parent_filter: &sea_orm::Condition,
        refs: &HashMap<String, String>,
        rule: &crate::ui::RowsValidator,
    ) -> AppResult<()> {
        let mut query = self.live_rows(parent_filter);
        if let Some(column) = target.order_column {
            query = query.order_by_asc(self.column(column).column);
        }
        let rows = query.order_by_asc(self.pk).limit(MAX_ORDERED_ROWS as u64 + 1).all(txn.as_ref()).await?;
        if rows.len() > MAX_ORDERED_ROWS {
            return Err(ValidationErrors::field(target.relation, "has too many rows to check here").into());
        }
        let created: HashMap<&String, &String> = refs.iter().map(|(reference, key)| (key, reference)).collect();
        let rows = rows
            .iter()
            .map(|model| {
                let key = self.key_of(model);
                let id = created.get(&key).map_or(key.clone(), |r| (*r).clone());
                let mut values = self.record(model);
                values.retain(|name, _| self.field(name).is_none_or(|f| f.secret.is_none()));
                crate::ui::CheckedRow { id, values }
            })
            .collect();
        let check = crate::ui::RowsCheck { operation: target.holder_op, parent: target.holder.clone(), rows };
        rule(ctx, &check).map_err(|refused| {
            let mut errors = ValidationErrors::new();
            refused.add_at(target.relation, &mut errors);
            AppError::Validation(errors)
        })
    }

    /// Removes every row of the parent the user sees: the live children (at
    /// most 1,000) locked through the child's scope and view policy, each
    /// deleted under the child's policy (403 for one refused: nothing is
    /// saved), its own rows first. Rows the user cannot see stay (the write
    /// hides the repeater, ADR 0044) or, `strict`, refuse the write (403: the
    /// rows of a removed row, part C). `deletable` and `min_items` do not apply.
    async fn clear_rows(
        &self,
        ctx: &Ctx,
        txn: &Arc<sea_orm::DatabaseTransaction>,
        target: &RowTarget<'_>,
        strict: bool,
    ) -> AppResult<RowsOutcome> {
        let parent_value = self.fk_value(target.field, target.parent_key);
        let fixed = self.relation_values(target)?;
        let parent_filter = self.parent_filter(target, &parent_value, &fixed)?;
        let live = self
            .live_rows(&parent_filter)
            .order_by_asc(self.pk)
            .limit(MAX_ORDERED_ROWS as u64 + 1)
            .all(txn.as_ref())
            .await?;
        if live.len() > MAX_ORDERED_ROWS {
            return Err(ValidationErrors::field(target.relation, "has too many rows to remove here").into());
        }
        let keys: Vec<String> = live.iter().map(|m| self.key_of(m)).collect();
        let mut locked: Vec<String> = self.locked_visible_rows(ctx, txn, &keys).await?.into_iter().collect();
        locked.sort();
        // The framework never removes rows the user cannot see (ADR 0044, part C):
        // one still there, not locked through the user's scope and policy, refuses
        // the write (rows gone meanwhile do not).
        if strict && locked.len() < keys.len() {
            let still: Vec<String> = self
                .live_rows(&parent_filter)
                .order_by_asc(self.pk)
                .limit(MAX_ORDERED_ROWS as u64 + 1)
                .all(txn.as_ref())
                .await?
                .iter()
                .map(|m| self.key_of(m))
                .collect();
            if still.iter().any(|k| !locked.contains(k)) {
                return Err(AppError::Forbidden);
            }
        }
        // Removing rows is the child's `delete`, admitted as a repeater's delete part is.
        if !locked.is_empty() && !self.policy.admits(ctx, ability::DELETE).await {
            return Err(AppError::Forbidden);
        }
        target.budget.spend(locked.len())?;
        let mut outcome = RowsOutcome::default();
        for key in locked {
            // Gone, trashed or moved meanwhile: not one of the rows any more.
            let model = match self.find_model_in(ctx, txn.as_ref(), &key, Trashed::Without).await {
                Ok(model) => model,
                Err(AppError::NotFound) => continue,
                Err(err) => return Err(err),
            };
            let record = self.record(&model);
            if !in_relation(&record, target.field, &parent_value, &fixed) {
                continue;
            }
            if !self.policy.allows(ctx, ability::DELETE, Some(&record)).await {
                return Err(AppError::Forbidden);
            }
            self.remove_row(ctx, txn, target, model).await?;
            outcome.deleted.push(key);
        }
        Ok(outcome)
    }

    /// [`Self::locked_visible`] over `keys` (sorted), in batches of the keys
    /// one request may name: still in key order, up to the 1,000 rows of a
    /// relation a change may lock.
    async fn locked_visible_rows(
        &self,
        ctx: &Ctx,
        txn: &Arc<sea_orm::DatabaseTransaction>,
        keys: &[String],
    ) -> AppResult<HashSet<String>> {
        let limit = self.limits.keys.max(1);
        if keys.len() <= limit {
            return self.locked_visible(ctx, txn, keys, &[]).await;
        }
        // The database's key order, so the batches lock in it.
        let values: Vec<Value> = keys.iter().filter_map(|k| self.pk_kind.decode_key(k).ok()).collect();
        let keys: Vec<String> = Entity::<R>::find()
            .filter(self.pk.is_in(values))
            .order_by_asc(self.pk)
            .all(txn.as_ref())
            .await?
            .iter()
            .map(|m| self.key_of(m))
            .collect();
        let mut out = HashSet::new();
        for batch in keys.chunks(limit) {
            out.extend(self.locked_visible(ctx, txn, batch, &[]).await?);
        }
        Ok(out)
    }

    /// Deletes a locked row of `target`: the rows of its own relationship
    /// repeaters first (every one the user cannot see refuses the write, ADR
    /// 0044 part C), then the row, with its history and hooks.
    async fn remove_row(
        &self,
        ctx: &Ctx,
        txn: &Arc<sea_orm::DatabaseTransaction>,
        target: &RowTarget<'_>,
        model: Model<R>,
    ) -> AppResult<()> {
        if !target.rules.nested.is_empty() {
            let key = self.key_of(&model);
            let changes = (0..target.rules.nested.len()).map(|index| (index, RowChange::removing(true))).collect();
            let mut ancestors = target.ancestors.to_vec();
            ancestors.push(key.clone());
            let level = RowsLevel {
                repeaters: &target.rules.nested,
                key: &key,
                root: target.root,
                parent: None,
                ancestors: &ancestors,
                budget: target.budget,
                // Removals check nothing.
                holder: &Data::new(),
                op: Operation::Update,
            };
            self.write_changes(ctx, txn, level, changes).await?;
        }
        self.delete_locked(ctx, txn, model).await
    }

    /// Deletes a locked record (the trash with soft deletes), with its history and hooks.
    pub(super) async fn delete_locked(
        &self,
        ctx: &Ctx,
        txn: &Arc<sea_orm::DatabaseTransaction>,
        model: Model<R>,
    ) -> AppResult<()> {
        let event = if self.soft.is_some() { "trashed" } else { "deleted" };
        self.audit_write(ctx, txn, event, &self.key_of(&model), None, None).await?;
        if self.soft.is_some() {
            // To the trash: related records are kept (no database cascade runs).
            let mut active = model.into_active_model();
            self.set_marker(&mut active, true)?;
            let model = active.update(txn.as_ref()).await.map_err(|e| self.map_db_err(ctx, e))?;
            R::after_trash(ctx, txn, &model).await
        } else {
            Entity::<R>::delete_many()
                .filter(self.pk.eq(model.get(self.pk)))
                .exec(txn.as_ref())
                .await
                .map_err(|e| self.map_db_err(ctx, e))?;
            R::after_delete(ctx, txn, &model).await
        }
    }

    /// Updates a locked row with client `data` (its own rows, `after_save`
    /// and history come with [`Self::finish_row`]).
    async fn update_row(
        &self,
        ctx: &Ctx,
        txn: &Arc<sea_orm::DatabaseTransaction>,
        model: Model<R>,
        data: Data,
        parent_field: &str,
        rows: &RowWrite<'_>,
    ) -> AppResult<WrittenRow<R>> {
        let (level, fixed) = (&rows.level, rows.fixed.as_slice());
        let mut input = self.accept_input_in(ctx, Operation::Update, data, level.conditions, level.repeaters, &[])?;
        // Rows are locked through the child's view policy: the user views them.
        let record = self.record(&model);
        let (hidden_before, hidden) =
            self.apply_conditions_in(ctx, Operation::Update, &mut input, Some((&record, true)), None, Some(level))?;
        let values = self.row_values(ctx, Operation::Update, Some((&record, true)), &input, level);
        let data = R::mutate_before_update(ctx, txn, &model, input).await?;
        // The row stays one of the relation's (ADR 0044).
        self.check_filters_kept(&data, fixed)?;
        // The row stays the child of the parent that was locked and checked.
        if data.get(parent_field).is_some_and(|v| record.get(parent_field) != Some(v)) {
            return Err(AppError::internal(format!("`{}`: a hook changed the relation key `{parent_field}`", R::SLUG)));
        }
        // Its media are checked with the parent's, last (ADR 0044).
        let media = self.media_references_in("", &data, level.repeaters)?;
        self.check_relations(ctx, txn, &data).await?;
        if data.is_empty() {
            return Ok(WrittenRow { model, written: None, media, values, hidden_before, hidden });
        }
        let before = self.audit_snapshot(&model);
        let stored = model.clone();
        let mut active = model.into_active_model();
        self.apply(ctx, &mut active, &data, Operation::Update, Some(&stored))?;
        let model = active.update(txn.as_ref()).await.map_err(|e| self.map_db_err(ctx, e))?;
        Ok(WrittenRow { model, written: Some(("updated", Some(before))), media, values, hidden_before, hidden })
    }

    /// Inserts a row under `parent` (the child's field and the parent's key);
    /// its own rows, `after_save` and history come with [`Self::finish_row`].
    async fn create_row(
        &self,
        ctx: &Ctx,
        txn: &Arc<sea_orm::DatabaseTransaction>,
        data: Data,
        parent: (&str, &serde_json::Value),
        position: Option<(&str, i64)>,
        rows: &RowWrite<'_>,
    ) -> AppResult<WrittenRow<R>> {
        let (level, fixed) = (&rows.level, rows.fixed.as_slice());
        // The parent key, the position and the relation's values are the server's,
        // written below: never required from the client, and no write rule needed
        // (ADR 0044, part C: a blank's key on its choices).
        let given: Vec<&str> =
            fixed.iter().map(|(f, _)| *f).chain(position.map(|(c, _)| c)).chain([parent.0]).collect();
        let mut input =
            self.accept_input_in(ctx, Operation::Create, data, level.conditions, level.repeaters, &given)?;
        input.insert(parent.0.to_owned(), parent.1.clone());
        // The position is the server's (never client input).
        if let Some((column, value)) = position {
            input.insert(column.to_owned(), self.position_value(column, value));
        }
        // A filtered relation's values and inherited keys are the server's too (ADR 0044).
        for (field, value) in fixed {
            input.insert((*field).to_owned(), value.clone());
        }
        let (_, field, _) = ctx.bound.clone().expect("rows are created with their parent");
        let (hidden_before, hidden) =
            self.apply_conditions_in(ctx, Operation::Create, &mut input, None, Some(field.as_str()), Some(level))?;
        let values = self.row_values(ctx, Operation::Create, None, &input, level);
        let parents = self.lock_parents(ctx, txn, self.parent_of_data(&input).flatten()).await?;
        let parent_value = input.get(&field).cloned();
        let data = R::mutate_before_create(ctx, txn, input).await?;
        self.check_parent_written(ctx, &data, &parents)?;
        self.check_filters_kept(&data, fixed)?;
        if data.get(&field) != parent_value.as_ref() {
            return Err(AppError::internal(format!("`{}`: a hook changed the relation key `{field}`", R::SLUG)));
        }
        // Its media are checked with the parent's, last (ADR 0044).
        let media = self.media_references_in("", &data, level.repeaters)?;
        self.check_relations(ctx, txn, &data).await?;
        let mut active = <<Entity<R> as EntityTrait>::ActiveModel as ActiveModelBehavior>::new();
        self.apply(ctx, &mut active, &data, Operation::Create, None)?;
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
        let model = active.insert(txn.as_ref()).await.map_err(|e| self.map_db_err(ctx, e))?;
        self.check_parent_stored(ctx, &model, &parents)?;
        Ok(WrittenRow { model, written: Some(("created", None)), media, values, hidden_before, hidden })
    }

    /// After a row is written: the changes of its own relationship repeaters
    /// (those its conditions hide dropped, one this write hides cleared, ADR
    /// 0044 part C), then its `after_save` (when it or its rows changed) and
    /// history. Whether anything changed, and the media of the row and its
    /// rows (named from the row: `image`, `choices.n2.image`).
    async fn finish_row(
        &self,
        ctx: &Ctx,
        txn: &Arc<sea_orm::DatabaseTransaction>,
        target: &RowTarget<'_>,
        op: Operation,
        row: WrittenRow<R>,
        nested: Nested,
    ) -> AppResult<(bool, Vec<crate::engine::repeater::MediaReference>)> {
        let WrittenRow { model, written, mut media, values, hidden_before, hidden } = row;
        let mut outcomes = Vec::new();
        if !target.rules.nested.is_empty() {
            let changes = settle(&target.rules.nested, op, nested, &hidden_before, &hidden)?;
            let key = self.key_of(&model);
            let mut ancestors = target.ancestors.to_vec();
            ancestors.push(key.clone());
            let holder = self.record(&model);
            let level = RowsLevel {
                repeaters: &target.rules.nested,
                key: &key,
                root: target.root,
                parent: Some(&values),
                ancestors: &ancestors,
                budget: target.budget,
                holder: &holder,
                op,
            };
            outcomes = self.write_changes(ctx, txn, level, changes).await?;
            for (_, outcome) in &mut outcomes {
                media.append(&mut outcome.media);
            }
        }
        let changed = written.is_some() || outcomes.iter().any(|(_, o)| !o.is_empty());
        // The row's hooks see its rows, as the record's do (ADR 0044, part C).
        if changed {
            R::after_save(ctx, txn, &model).await?;
        }
        if let Some((event, before)) = written {
            self.audit_outcome(ctx, txn, event, before.as_ref(), &model, false, event == "created").await?;
        }
        self.audit_rows(ctx, txn, &model, outcomes).await?;
        Ok((changed, media))
    }

    /// The row as its conditions leave it after the write (hidden inputs
    /// `null`): what the conditions of its own rows read (`when_parent`).
    fn row_values(
        &self,
        ctx: &Ctx,
        op: Operation,
        stored: Option<(&Data, bool)>,
        input: &Data,
        level: &super::input::RowLevel<'_>,
    ) -> Data {
        let mut values = self.condition_values_in(ctx, op, stored, &level.inputs);
        values.extend(input.iter().map(|(k, v)| (k.clone(), v.clone())));
        level
            .conditions
            .shown_values(&values, crate::engine::conditions::OuterValues { parent: level.parent, root: level.root })
    }

    /// The parent's rows not in the trash.
    fn live_rows(&self, parent_filter: &sea_orm::Condition) -> sea_orm::Select<Entity<R>> {
        let mut query = Entity::<R>::find().filter(parent_filter.clone());
        if let Some((marker, _, _)) = &self.soft {
            query = query.filter(marker.is_null());
        }
        query
    }

    /// The values the relation's rows hold besides the parent key, canonical:
    /// its filters' (ADR 0044) and the ancestors' keys it inherits (part C).
    fn relation_values<'a>(&self, target: &RowTarget<'a>) -> AppResult<Vec<(&'a str, serde_json::Value)>> {
        let mut out: Vec<(&'a str, serde_json::Value)> = self.filter_values(target.filters)?;
        for (field, level) in &target.rules.inherit {
            let key = target
                .ancestors
                .get(*level)
                .ok_or_else(|| AppError::internal(format!("`{}`: no ancestor for `{field}`", R::SLUG)))?;
            out.push((field.as_str(), self.fk_value(field, key)));
        }
        Ok(out)
    }

    /// The relation's rows: `target.field` equals the parent's key, and the
    /// relation's values hold (`fixed`, ADR 0044).
    fn parent_filter(
        &self,
        target: &RowTarget<'_>,
        parent_value: &serde_json::Value,
        fixed: &[(&str, serde_json::Value)],
    ) -> AppResult<sea_orm::Condition> {
        let mut condition = self.parent_key_filter(target, parent_value)?;
        for (field, filter) in target.filters {
            let filter = self
                .relation_filter_condition(field, filter)
                .ok_or_else(|| AppError::internal(format!("`{}`: relation filter on `{field}`", R::SLUG)))?;
            condition = condition.add(filter);
        }
        // Inherited keys (filters are above): the ancestor's rows only.
        for (field, value) in fixed.iter().filter(|(f, _)| !target.filters.iter().any(|(g, _)| g == f)) {
            condition = condition.add(self.eq_filter(field, value)?);
        }
        Ok(condition)
    }

    /// `target.field` equals the parent's key (every row of the parent, whatever relation).
    fn parent_key_filter(
        &self,
        target: &RowTarget<'_>,
        parent_value: &serde_json::Value,
    ) -> AppResult<sea_orm::Condition> {
        Ok(sea_orm::Condition::all().add(self.eq_filter(target.field, parent_value)?))
    }

    /// `field` equals `value` (a key, in wire form).
    fn eq_filter(&self, field: &str, value: &serde_json::Value) -> AppResult<sea_orm::Condition> {
        let column = self.column(field);
        let value = column
            .kind
            .decode(value)
            .map_err(|e| AppError::internal(e.0))?
            .into_iter()
            .next()
            .ok_or_else(|| AppError::internal(format!("`{}`: `{field}` without a value", R::SLUG)))?;
        Ok(sea_orm::Condition::all().add(column.column.eq(value)))
    }

    /// The values a filtered relation's rows hold (ADR 0044), in canonical wire form.
    pub(super) fn filter_values(
        &self,
        filters: &[(&'static str, RelationFilter)],
    ) -> AppResult<Vec<(&'static str, serde_json::Value)>> {
        filters.iter().map(|(field, filter)| Ok((*field, self.canonical_filter(field, filter)?))).collect()
    }

    /// The value `filter` on `field` requires, in canonical wire form (`null` for `where_null`).
    pub(super) fn canonical_filter(&self, field: &str, filter: &RelationFilter) -> AppResult<serde_json::Value> {
        let value = filter.value();
        if value.is_null() {
            return Ok(value);
        }
        let column = self.column(field);
        let scale = crate::wire::decimal_scale(column.column.def().get_column_type());
        crate::wire::canonical(column.kind, scale, &value)
            .map_err(|e| AppError::internal(format!("`{}`: relation filter on `{field}`: {}", R::SLUG, e.0)))
    }

    /// Refuses data a hook wrote outside the relation's values (500, as for the parent key).
    pub(super) fn check_filters_kept(&self, data: &Data, filters: &[(&str, serde_json::Value)]) -> AppResult<()> {
        match filters.iter().find(|(field, value)| data.get(*field).is_some_and(|v| v != value)) {
            Some((field, _)) => {
                Err(AppError::internal(format!("`{}`: a hook changed `{field}`, which the relation sets", R::SLUG)))
            }
            None => Ok(()),
        }
    }

    /// The largest position among the parent's rows (trashed ones included:
    /// their places stay taken; every relation of the parent, filtered or not,
    /// so a unique `(parent, position)` holds) and, for a nested repeater, the
    /// rows sharing an ancestor's key it inherits (a unique `(question,
    /// position)` holds across the blanks, ADR 0044 part C), 0 for none: one
    /// `MAX`, read by the column's kind.
    async fn largest_position(
        &self,
        txn: &Arc<sea_orm::DatabaseTransaction>,
        target: &RowTarget<'_>,
        column: &str,
        parent_value: &serde_json::Value,
    ) -> AppResult<i64> {
        let position = self.column(column);
        let mut scope = sea_orm::Condition::any().add(self.parent_key_filter(target, parent_value)?);
        for (field, level) in &target.rules.inherit {
            if let Some(key) = target.ancestors.get(*level) {
                scope = scope.add(self.eq_filter(field, &self.fk_value(field, key))?);
            }
        }
        let query = Entity::<R>::find().filter(scope).select_only().expr_as(
            sea_orm::sea_query::Func::max(sea_orm::sea_query::Expr::col((Entity::<R>::default(), position.column))),
            "largest",
        );
        let largest = match position.kind {
            FieldKind::I64 => query.into_tuple::<Option<i64>>().one(txn.as_ref()).await?.flatten(),
            _ => query.into_tuple::<Option<i32>>().one(txn.as_ref()).await?.flatten().map(i64::from),
        };
        Ok(largest.unwrap_or(0))
    }

    /// The position after `position` in the order column, within its range
    /// (`i32` or `i64`); 422 once the positions are used up.
    fn next_position(&self, relation: &str, column: &str, position: i64) -> AppResult<i64> {
        let limit = match self.column(column).kind {
            FieldKind::I64 => i64::MAX,
            _ => i64::from(i32::MAX),
        };
        position
            .checked_add(1)
            .filter(|next| *next <= limit)
            .ok_or_else(|| ValidationErrors::field(relation, "has no positions left; renumber the rows").into())
    }

    /// A position as the order column's kind travels (an `i64` as a string).
    fn position_value(&self, column: &str, position: i64) -> serde_json::Value {
        match self.column(column).kind {
            FieldKind::I64 => serde_json::Value::String(position.to_string()),
            _ => serde_json::Value::from(position),
        }
    }

    /// The position a stored row holds.
    fn held_position(&self, column: &str, model: &Model<R>) -> Option<i64> {
        let position = self.column(column).column;
        match encode_column(position.def().get_column_type(), &model.get(position)) {
            serde_json::Value::String(text) => text.parse().ok(),
            other => other.as_i64(),
        }
    }

    /// Writes the positions `order` sets (ADR 0040): the rows the user sees
    /// take, in that order, the positions those rows held (new rows the ones
    /// after the largest); rows the user cannot see keep theirs. `survivors`:
    /// the rows locked visible in this write's locking pass and not deleted
    /// (no row is locked here). Two passes, so a unique `(parent, position)`
    /// holds throughout. Whether a position changed.
    #[allow(clippy::too_many_arguments)]
    async fn reorder_rows(
        &self,
        ctx: &Ctx,
        txn: &Arc<sea_orm::DatabaseTransaction>,
        target: &RowTarget<'_>,
        column: &str,
        parent_value: &serde_json::Value,
        order: &[RowId],
        refs: &HashMap<String, String>,
        survivors: &HashSet<String>,
    ) -> AppResult<bool> {
        let relation = target.relation;
        let position = self.column(column).column;
        let decode = |kind: FieldKind, value: &serde_json::Value| -> AppResult<Vec<Value>> {
            kind.decode(value).map_err(|e| AppError::internal(e.0))
        };
        // The rows ordered: the survivors and the rows created by this write.
        let created: HashSet<&String> = refs.values().collect();
        let mut keys: Vec<String> = survivors.iter().cloned().chain(refs.values().cloned()).collect();
        keys.sort();
        keys.dedup();
        let rows: Vec<Model<R>> = if keys.is_empty() {
            Vec::new()
        } else {
            self.live_rows(&self.parent_filter(target, parent_value, &self.relation_values(target)?)?)
                .filter(self.pk.is_in(self.decode_keys(&keys)?))
                .all(txn.as_ref())
                .await?
        };
        let shown: HashSet<String> = rows.iter().map(|m| self.key_of(m)).collect();
        let mut wanted: Vec<String> = Vec::with_capacity(order.len());
        let mut errors = ValidationErrors::new();
        for id in order {
            let key = match id {
                RowId::Key(key) => Some(key.clone()),
                RowId::New(reference) => refs.get(reference).cloned(),
            };
            match key {
                Some(key) if shown.contains(&key) && !wanted.contains(&key) => wanted.push(key),
                _ => {
                    errors.add(relation, "must list every row once");
                    break;
                }
            }
        }
        if wanted.len() != shown.len() {
            errors.add(relation, "must list every row once");
        }
        errors.into_result()?;
        // The places the stored visible rows hold (new rows took the ones after the largest).
        let mut slots: Vec<i64> = rows
            .iter()
            .filter(|m| !created.contains(&self.key_of(m)))
            .filter_map(|m| self.held_position(column, m))
            .collect();
        slots.sort_unstable();
        // Every position in use (hidden, trashed, new rows included) is below this.
        let mut largest = self.largest_position(txn, target, column, parent_value).await?;
        let created_slots: Vec<i64> = rows
            .iter()
            .filter(|m| created.contains(&self.key_of(m)))
            .filter_map(|m| self.held_position(column, m))
            .collect();
        slots.extend(created_slots);
        slots.sort_unstable();
        while slots.len() < wanted.len() {
            largest = self.next_position(relation, column, largest)?;
            slots.push(largest);
        }
        let by_key: HashMap<String, &Model<R>> = rows.iter().map(|m| (self.key_of(m), m)).collect();
        let moves: Vec<(&Model<R>, i64)> = wanted
            .iter()
            .zip(slots)
            .filter_map(|(key, slot)| by_key.get(key).map(|m| (*m, slot)))
            .filter(|(m, slot)| self.held_position(column, m) != Some(*slot))
            .collect();
        if moves.is_empty() {
            return Ok(false);
        }
        // Moving a stored row writes it: `update` on the child and on each row (new rows are the user's).
        let stored: Vec<&Model<R>> =
            moves.iter().map(|(m, _)| *m).filter(|m| !created.contains(&self.key_of(m))).collect();
        if !stored.is_empty() && !self.policy.admits(ctx, ability::UPDATE).await {
            return Err(AppError::Forbidden);
        }
        for model in stored {
            if !self.policy.allows(ctx, ability::UPDATE, Some(&self.record(model))).await {
                return Err(AppError::Forbidden);
            }
        }
        // Out of the way first (above every position in use), then in place.
        let kind = self.column(column).kind;
        let mut offset = largest;
        let mut temporary = Vec::with_capacity(moves.len());
        for _ in &moves {
            offset = self.next_position(relation, column, offset)?;
            temporary.push(offset);
        }
        for pass in [true, false] {
            for (index, (model, slot)) in moves.iter().enumerate() {
                let value = if pass { temporary[index] } else { *slot };
                let mut active = (*model).clone().into_active_model();
                let candidates = decode(kind, &self.position_value(column, value))?;
                if !candidates.into_iter().any(|v| active.try_set(position, v).is_ok()) {
                    return Err(AppError::internal(format!("`{}`: cannot write the order column `{column}`", R::SLUG)));
                }
                active.update(txn.as_ref()).await.map_err(|e| self.map_db_err(ctx, e))?;
            }
        }
        Ok(true)
    }
}

/// Most rows a change may order (the parent's live children, discovered
/// before any lock).
const MAX_ORDERED_ROWS: usize = 1000;

/// Most entries of one part of a relationship repeater's change.
const MAX_ROW_ITEMS: usize = 100;

/// A relationship repeater of a resource's form, or nested in another's rows
/// (ADR 0044, part C), checked at boot.
#[derive(Debug, Clone)]
pub(crate) struct RowRepeater {
    /// The has-many relation (and the input's name).
    pub(crate) name: &'static str,
    pub(crate) target: &'static str,
    /// The child's field holding the parent key.
    pub(crate) field: &'static str,
    pub(crate) order_column: Option<String>,
    pub(crate) spec: crate::ui::RepeaterSpec,
    /// The child fields the rows edit (their JSON repeaters included, not
    /// their relationship repeaters).
    pub(crate) inputs: Vec<String>,
    /// The relation's filters (ADR 0044).
    pub(crate) filters: Vec<(&'static str, RelationFilter)>,
    /// The rows' conditions, checked with the child's kinds (`Registry::finish`).
    pub(crate) rules: std::sync::OnceLock<RowRules>,
}

/// A relationship repeater's rows as the child's kinds read them (ADR 0044).
#[derive(Debug, Clone)]
pub(crate) struct RowRules {
    /// Their conditions (the row's inputs, the record's form with `when_root`,
    /// the enclosing row with `when_parent`).
    pub(crate) conditions: crate::engine::conditions::FormConditions,
    /// The row's components, conditions in canonical form (what schemas send).
    pub(crate) components: Vec<Component>,
    /// The relationship repeaters of the rows (part C), compiled with them.
    pub(crate) nested: Vec<RowRepeater>,
    /// The JSON repeaters of the rows, by input (part C).
    pub(crate) json: HashMap<String, crate::engine::repeater::Rows>,
    /// Child fields the server fills with an ancestor's key (`inherit`): the
    /// field and the level (in [`RowTarget::ancestors`]).
    pub(crate) inherit: Vec<(String, usize)>,
}

impl RowRepeater {
    /// The rows' rules (set at boot).
    pub(crate) fn rules(&self) -> &RowRules {
        self.rules.get().expect("relationship rows are compiled by `Registry::finish`")
    }
}

/// The relationship repeaters of `form`; panics on what cannot hold here
/// (the child's fields are checked by `Registry::finish`).
pub(super) fn row_repeaters(
    slug: &str,
    form: &Form,
    relations: &[Relation],
    form_conditions: &crate::engine::conditions::FormConditions,
) -> Vec<RowRepeater> {
    build(&format!("resource `{slug}`"), form.inputs(), relations, &|name| form_conditions.is_conditional(name), true)
}

/// The relationship repeaters among `inputs` (a form's, `top`, or a
/// relationship row's), over `relations` (of the form's resource, or of the
/// row's).
fn build(
    owner: &str,
    inputs: Vec<&Input>,
    relations: &[Relation],
    conditional: &dyn Fn(&str) -> bool,
    top: bool,
) -> Vec<RowRepeater> {
    let mut out = Vec::new();
    for input in inputs {
        let Some(spec) = input.repeater.as_deref().filter(|s| s.relationship) else { continue };
        let name = input.name.as_str();
        let here = format!("{owner}: repeater `{name}`");
        let relation = relations
            .iter()
            .find(|r| r.name == name && r.pivot.is_none())
            .unwrap_or_else(|| panic!("{here}: `relationship()` needs a has-many relation named `{name}`"));
        crate::engine::repeater::check_spec(&here, spec);
        assert!(
            !top || spec.inherit.is_empty(),
            "{here}: `inherit` is for relationship repeaters nested in rows (a form's rows get the record's key)"
        );
        // Hidden by a write, its rows are removed (ADR 0044): nothing to clear, and
        // never on a repeater whose rows may not be removed.
        assert!(
            !input.clear_when_hidden,
            "{here}: `clear_when_hidden` does not apply (a write that hides a relationship repeater removes its rows)"
        );
        assert!(
            spec.deletable || !conditional(name),
            "{here}: a repeater with `deletable(false)` cannot be under a condition (hiding it removes its rows)"
        );
        let mut inputs = Vec::new();
        // Conditions in rows are checked with the child's kinds (`compile_rules`).
        for row in crate::engine::repeater::row_inputs(&spec.components) {
            let row_name = row.name.as_str();
            assert!(
                !row.clear_when_hidden,
                "{here}: `{row_name}`: `clear_when_hidden` does not apply inside relationship rows"
            );
            assert!(
                !matches!(row.kind, InputType::BelongsToMany | InputType::Secret | InputType::SecretMap),
                "{here}: `{row_name}`: belongs-to-many and secret inputs cannot be in relationship rows"
            );
            assert!(!inputs.iter().any(|n| n == row_name), "{here}: input `{row_name}` appears twice in a row");
            // A nested relationship repeater (part C) is not a field of the child.
            if row.repeater.as_deref().is_some_and(|s| s.relationship) {
                continue;
            }
            assert!(
                row.content.as_deref().is_none_or(|c| !c.has_rules()),
                "{here}: `{row_name}`: in relationship rows the child's field decides `accept`, `max_size`, \
                 `multiple`, `collection` and `nodes`"
            );
            assert!(
                !row.required && row.max_length.is_none() && !row.integer,
                "{here}: `{row_name}`: in relationship rows the child's field decides `required`, `max_length` and \
                 integers"
            );
            // Rows are checked with the child's rules: options and bounds of their own
            // would show rules the server does not hold (ADR 0041).
            assert!(
                row.options.is_empty() && row.number.is_none(),
                "{here}: `{row_name}`: in relationship rows the child's form decides options and number bounds"
            );
            assert!(row.preview, "{here}: input `{row_name}`: only file uploads take `preview`");
            inputs.push(row_name.to_owned());
        }
        if let Some(column) = &spec.order_column {
            assert!(!inputs.contains(column), "{here}: the order column `{column}` cannot be an input");
        }
        assert!(
            !inputs.iter().any(|n| n == relation.field),
            "{here}: the parent key `{}` cannot be an input",
            relation.field
        );
        // The relation's filters are the server's (ADR 0044).
        for (field, _) in &relation.filters {
            assert!(
                !inputs.iter().any(|n| n == field),
                "{here}: `{field}`, which the relation filters, cannot be an input"
            );
            assert!(
                spec.order_column.as_deref() != Some(*field),
                "{here}: `{field}`, which the relation filters, cannot be the order column"
            );
        }
        out.push(RowRepeater {
            name: relation.name,
            target: relation.target,
            field: relation.field,
            order_column: spec.order_column.clone(),
            spec: spec.clone(),
            inputs,
            filters: relation.filters.clone(),
            rules: std::sync::OnceLock::new(),
        });
    }
    out
}

/// Compiles the rows of `repeater` with its child's kinds (ADR 0044): their
/// conditions (reading the record's form through `root`, the enclosing row
/// through `parent`), JSON repeaters, inherited keys, and the relationship
/// repeaters nested in them, each checked and compiled in turn. `ancestors`
/// are the resources of the levels above the rows (the form's first, the
/// parent's last); `depth` is the repeater's (1 on a form).
///
/// # Panics
/// On what cannot hold (ADR 0037, 0040, 0044 boot checks).
#[allow(clippy::too_many_arguments)]
pub(crate) fn compile_rules(
    here: &str,
    repeater: &RowRepeater,
    registry: &crate::Registry,
    nodes: &Arc<crate::rich_text::AppNodes>,
    root: crate::engine::conditions::KindOf<'_>,
    parent: Option<crate::engine::conditions::KindOf<'_>>,
    ancestors: &[&'static str],
    depth: u8,
) {
    use crate::engine::conditions::{FormConditions, Outer};
    let Some(child) = registry.get(repeater.target) else { return };
    let child = child.as_ref();
    let target = repeater.target;
    let row = |name: &str| repeater.inputs.iter().any(|n| n == name).then(|| child.condition_kind(name)).flatten();
    let conditions = FormConditions::new_in(here, &repeater.spec.components, &row, Outer { parent, root: Some(root) });
    for (name, _) in conditions.conditional() {
        // Nested relationship repeaters are checked below.
        if !repeater.inputs.iter().any(|n| n == name) {
            continue;
        }
        assert!(
            child.stores_hidden(name),
            "{here}: row input `{name}` has a condition but `{target}.{name}` is not nullable and has no default (a \
             row created while it is hidden could not be stored): declare a default or make it nullable"
        );
    }
    let mut components = repeater.spec.components.clone();
    let kind_of = |scope: crate::ui::Scope, name: &str| match scope {
        crate::ui::Scope::Row => row(name),
        crate::ui::Scope::Root => root(name),
        crate::ui::Scope::Parent => parent.and_then(|p| p(name)),
    };
    crate::engine::conditions::canonicalize_with(&mut components, &kind_of, root);

    // JSON repeaters in the rows (part C): their own rules, one level down.
    let mut json = HashMap::new();
    for input in crate::engine::repeater::row_inputs(&repeater.spec.components) {
        let Some(spec) = input.repeater.as_deref().filter(|s| !s.relationship) else { continue };
        let name = input.name.as_str();
        assert!(
            child.condition_kind(name).is_some_and(|(kind, _)| kind == FieldKind::Json),
            "{here}: repeater `{name}` needs `{target}.{name}` to be a JSON field"
        );
        let rows = crate::engine::repeater::Rows::compile_at(
            here,
            name,
            spec,
            depth + 1,
            Outer { parent: Some(&row), root: Some(root) },
        );
        rows.attach_nodes(here, nodes);
        json.insert(name.to_owned(), rows);
    }

    // Ancestors' keys the server fills in (part C).
    let mut inherit: Vec<(String, usize)> = Vec::new();
    for field in &repeater.spec.inherit {
        let level = match child.field_link(field) {
            Some((Some(linked), kind)) => ancestors
                .iter()
                .position(|a| *a == linked)
                .filter(|at| registry.get(ancestors[*at]).is_some_and(|r| r.key_kind() == kind)),
            _ => None,
        };
        let level = level.unwrap_or_else(|| {
            panic!(
                "{here}: `inherit(\"{field}\")` needs `{target}.{field}` to be a `belongs_to` of an enclosing level \
                 ({}) with the type of its key",
                ancestors.join(", ")
            )
        });
        assert!(
            field != repeater.field
                && !repeater.filters.iter().any(|(f, _)| f == field)
                && repeater.order_column.as_ref() != Some(field)
                && !repeater.inputs.contains(field)
                && !inherit.iter().any(|(f, _)| f == field),
            "{here}: `{field}` cannot be inherited (the parent key, a filter, the order column, an input, or \
             inherited twice)"
        );
        inherit.push((field.clone(), level));
    }
    // The family serializes on a row every writer locks (part C): the parent,
    // or an ancestor through an inherited key.
    if depth > 1 {
        let parent_slug = ancestors.last().copied().unwrap_or_default();
        let locks = match &child.meta().parent_lock {
            Some((field, locked)) => {
                (*locked == parent_slug && field == repeater.field)
                    || inherit.iter().any(|(f, level)| f == field && ancestors[*level] == *locked)
            }
            None => false,
        };
        assert!(
            locks,
            "{here}: `{target}` must lock its parent (`ResourceMeta::lock_parent(\"{}\", \"{parent_slug}\")`) or an \
             enclosing level through a field it inherits (`inherit`)",
            repeater.field
        );
        check_row_fields(here, repeater, child);
    }

    // Relationship repeaters nested in the rows (part C).
    let nested = build(
        here,
        crate::engine::repeater::row_inputs(&repeater.spec.components),
        child.relations(),
        &|name| conditions.is_conditional(name),
        false,
    );
    let mut below: Vec<&'static str> = ancestors.to_vec();
    below.push(target);
    for inner in &nested {
        let inner_here = format!("{here}: repeater `{}`", inner.name);
        assert!(
            depth < crate::engine::repeater::MAX_DEPTH,
            "{inner_here}: repeaters nest {} deep at most",
            crate::engine::repeater::MAX_DEPTH
        );
        // Settled with its row only (a row the save does not touch keeps its rows):
        // the condition of a nested repeater reads that row only (ADR 0044, part C).
        assert!(
            !conditions.reads_outside_for(inner.name),
            "{inner_here}: the condition of a nested relationship repeater (its own or a layout's) reads its row only \
             (`when`, not `when_root` or `when_parent`)"
        );
        assert!(
            !below.contains(&inner.target),
            "{inner_here}: `{}` is the resource of an enclosing level (a resource cannot edit itself through its rows)",
            inner.target
        );
        assert!(!repeater.spec.table, "{here}: table rows hold no nested repeater (they do not fit a cell)");
        // Two nested repeaters on one child edit relations that cannot share a row.
        for other in nested.iter().filter(|o| o.target == inner.target && o.name != inner.name) {
            let (Some(a), Some(b)) = (
                child.relations().iter().find(|r| r.name == inner.name),
                child.relations().iter().find(|r| r.name == other.name),
            ) else {
                continue;
            };
            let grandchild = registry.get(inner.target);
            let canonical = |field: &str, filter: &RelationFilter| {
                grandchild.and_then(|g| g.relation_filter_value(field, filter).ok()).unwrap_or_else(|| filter.value())
            };
            assert!(
                a.disjoint(b, &canonical),
                "{inner_here}: `{}` is also edited by repeater `{}`, whose relation may hold the same rows (filter them \
                 apart: `where_null` against `where_eq`, or two values)",
                inner.target,
                other.name
            );
        }
        compile_rules(&inner_here, inner, registry, nodes, root, Some(&row), &below, depth + 1);
    }
    let _ = repeater.rules.set(RowRules { conditions, components, nested, json, inherit });
}

/// The fields a nested repeater's rows edit (ADR 0044, part C): as
/// `Registry::finish` checks a form's repeaters.
fn check_row_fields(here: &str, repeater: &RowRepeater, child: &dyn DynResource) {
    for input in &repeater.inputs {
        assert!(
            child.field_link(input).is_some(),
            "{here}: row input `{input}` is not a field of `{}`",
            repeater.target
        );
        assert!(!child.field_is_encrypted(input), "{here}: row input `{input}` is an encrypted field (not in rows)");
    }
    if let Some(column) = &repeater.order_column {
        assert!(
            matches!(child.field_link(column), Some((None, "i32" | "i64"))),
            "{here}: the order column `{column}` must be an integer field of `{}`",
            repeater.target
        );
    }
}

/// Whether a condition of the rows of `repeaters` (any level, their JSON rows
/// too) reads input `name` of the record's form (`when_root`).
pub(crate) fn rows_read_root(repeaters: &[RowRepeater], name: &str) -> bool {
    repeaters.iter().any(|repeater| {
        repeater.rules.get().is_some_and(|rules| {
            rules.conditions.reads(crate::ui::Scope::Root, name)
                || rules.json.values().any(|rows| rows.reads_root(name))
                || rows_read_root(&rules.nested, name)
        })
    })
}

/// The JSON repeaters of the rows of `repeaters` (at every level) whose child
/// is `target` (ADR 0044, part C).
pub(crate) fn json_rows_of<'a>(
    repeaters: &'a [RowRepeater],
    target: &str,
    out: &mut Vec<&'a HashMap<String, crate::engine::repeater::Rows>>,
) {
    for repeater in repeaters {
        let Some(rules) = repeater.rules.get() else { continue };
        if repeater.target == target && !rules.json.is_empty() {
            out.push(&rules.json);
        }
        json_rows_of(&rules.nested, target, out);
    }
}

/// The pairs (outer child, nested child) of the relationship repeaters
/// nested in `repeaters`' rows, at every level: lock order edges (ADR 0044).
pub(crate) fn nested_edges(repeaters: &[RowRepeater], out: &mut Vec<(&'static str, &'static str)>) {
    for repeater in repeaters {
        let Some(rules) = repeater.rules.get() else { continue };
        for inner in &rules.nested {
            out.push((repeater.target, inner.target));
        }
        nested_edges(&rules.nested, out);
    }
}

/// Admits the parts of `value` (a change of `repeater`) and of the changes of
/// its rows' own repeaters: `create`, `update` and `delete` on each child
/// (403), before any key is read.
fn admit_change<'a>(
    ctx: &'a Ctx,
    repeater: &'a RowRepeater,
    value: &'a serde_json::Value,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = AppResult<()>> + Send + 'a>> {
    Box::pin(async move {
        let child = ctx.registry.get(repeater.target).ok_or(AppError::NotFound)?;
        let items = |part: &str| value.get(part).and_then(|v| v.as_array()).map(Vec::as_slice).unwrap_or_default();
        for (part, needed) in [("create", ability::CREATE), ("update", ability::UPDATE), ("delete", ability::DELETE)] {
            if !items(part).is_empty() && !child.admits(ctx, needed).await {
                return Err(AppError::Forbidden);
            }
        }
        let Some(rules) = repeater.rules.get() else { return Ok(()) };
        // Beyond a part's bound the shape check refuses the change: nothing more to admit.
        if items("create").len() > MAX_ROW_ITEMS || items("update").len() > MAX_ROW_ITEMS {
            return Ok(());
        }
        for item in items("create").iter().chain(items("update")) {
            let Some(data) = item.get("data").and_then(|d| d.as_object()) else { continue };
            for inner in &rules.nested {
                if let Some(change) = data.get(inner.name) {
                    admit_change(ctx, inner, change).await?;
                }
            }
        }
        Ok(())
    })
}

/// The changes of `repeaters` once the conditions of their level are known
/// (ADR 0044), from `hidden_before` and `hidden` (the inputs hidden before
/// and after the user's changes): a repeater hidden after the write drops
/// what was sent, and one this write hides (shown before, on update) removes
/// its rows. On create, a shown repeater's minimum holds on the rows it
/// creates, sent or not (as a JSON repeater's).
fn settle(
    repeaters: &[RowRepeater],
    op: Operation,
    changes: Nested,
    hidden_before: &BTreeSet<String>,
    hidden: &BTreeSet<String>,
) -> AppResult<Nested> {
    let mut out: Nested = changes.into_iter().filter(|(index, _)| !hidden.contains(repeaters[*index].name)).collect();
    let mut errors = ValidationErrors::new();
    for (index, repeater) in repeaters.iter().enumerate() {
        let name = repeater.name;
        if hidden.contains(name) {
            if op == Operation::Update && !hidden_before.contains(name) {
                out.push((index, RowChange::removing(false)));
            }
            continue;
        }
        if op == Operation::Create
            && let Some(min) = repeater.spec.min_items.filter(|m| *m > 0)
        {
            let created = out.iter().find(|(i, _)| *i == index).map_or(0, |(_, c)| c.create.len());
            if created < min as usize {
                errors.add(name, format!("must have at least {min} items"));
            }
        }
        // A rule over the rows holds on a new holder's rows, sent or not (ADR 0044, part D).
        if op == Operation::Create && repeater.spec.validate.is_some() && !out.iter().any(|(i, _)| *i == index) {
            out.push((index, RowChange::default()));
        }
    }
    errors.into_result()?;
    Ok(out)
}

/// The relationship repeaters of one level and where their rows go: the
/// form's (the record) or a row's (part C).
pub(super) struct RowsLevel<'a> {
    pub(super) repeaters: &'a [RowRepeater],
    /// The key of the record or row holding them.
    pub(super) key: &'a str,
    pub(super) root: &'a Data,
    /// The row holding them, as its conditions leave it (`when_parent`).
    pub(super) parent: Option<&'a Data>,
    /// The keys of the levels down to `key` (included).
    pub(super) ancestors: &'a [String],
    pub(super) budget: &'a RowBudget,
    /// The record or row holding them, as written, and whether it is created.
    pub(super) holder: &'a Data,
    pub(super) op: Operation,
}

impl<R: Resource> ResourceAdapter<R> {
    /// The relationship repeaters' changes in `input` (removed from it), with
    /// those of their rows' own repeaters: `create`, `update` and `delete` on
    /// each child admitted for the parts sent (403) before any key is read,
    /// then the shape (422 at the repeater's path), then the number of rows
    /// (422 on the form beyond 1,000, ADR 0044).
    pub(super) async fn take_rows(
        &self,
        ctx: &Ctx,
        op: Operation,
        input: &mut Data,
    ) -> AppResult<Vec<(usize, RowChange)>> {
        let mut sent = Vec::new();
        for (index, repeater) in self.row_repeaters.iter().enumerate() {
            if let Some(value) = input.remove(repeater.name) {
                sent.push((index, value));
            }
        }
        for (index, value) in &sent {
            admit_change(ctx, &self.row_repeaters[*index], value).await?;
        }
        let mut errors = ValidationErrors::new();
        let mut out = Vec::new();
        for (index, value) in sent {
            let repeater = &self.row_repeaters[index];
            match parse_change(value, op, repeater, &ctx.registry, repeater.name) {
                Ok(change) => out.push((index, change)),
                Err(path) => errors.add(path, "is invalid"),
            }
        }
        errors.into_result()?;
        if out.iter().map(|(_, c)| c.size()).sum::<usize>() > MAX_WRITE_ROWS {
            return Err(too_many_rows());
        }
        Ok(out)
    }

    /// The record's form as the conditions of relationship rows read it
    /// (`when_root`, ADR 0044): what the user's form holds (ADR 0037) with the
    /// accepted `input` over it, as its own conditions leave it (hidden inputs
    /// `null`). Empty when no row reads it.
    pub(super) fn row_root(&self, ctx: &Ctx, op: Operation, stored: Option<(&Data, bool)>, input: &Data) -> Data {
        if !self.row_repeaters.iter().any(reads_outside) {
            return Data::new();
        }
        let mut values = self.condition_values(ctx, op, stored);
        values.extend(input.iter().map(|(k, v)| (k.clone(), v.clone())));
        self.conditions.shown_values(&values, Default::default())
    }

    /// The form's relationship repeaters' changes once its conditions are
    /// known (ADR 0044, see [`settle`]).
    pub(super) fn settle_rows(
        &self,
        op: Operation,
        changes: Vec<(usize, RowChange)>,
        hidden_before: &BTreeSet<String>,
        hidden: &BTreeSet<String>,
    ) -> AppResult<Vec<(usize, RowChange)>> {
        settle(&self.row_repeaters, op, changes, hidden_before, hidden)
    }

    /// Writes the rows' changes after the parent's row (`model`) is written.
    pub(super) async fn write_row_changes(
        &self,
        ctx: &Ctx,
        txn: &Arc<sea_orm::DatabaseTransaction>,
        model: &Model<R>,
        op: Operation,
        changes: Vec<(usize, RowChange)>,
        root: &Data,
    ) -> AppResult<Vec<(&'static str, RowsOutcome)>> {
        let key = self.key_of(model);
        let budget = RowBudget::new(changes.iter().map(|(_, c)| c.size()).sum());
        let ancestors = [key.clone()];
        let holder = self.record(model);
        let level = RowsLevel {
            repeaters: &self.row_repeaters,
            key: &key,
            root,
            parent: None,
            ancestors: &ancestors,
            budget: &budget,
            holder: &holder,
            op,
        };
        self.write_changes(ctx, txn, level, changes).await
    }

    /// Writes the changes of one level's repeaters (the record's, or a row's
    /// of this resource) to their children.
    pub(super) async fn write_changes(
        &self,
        ctx: &Ctx,
        txn: &Arc<sea_orm::DatabaseTransaction>,
        level: RowsLevel<'_>,
        changes: Vec<(usize, RowChange)>,
    ) -> AppResult<Vec<(&'static str, RowsOutcome)>> {
        let mut out = Vec::new();
        // One phase per child resource, in lock order (ADR 0041).
        let mut changes = changes;
        // Two repeaters on one child (filtered relations, ADR 0044) in the order
        // their relations are declared: both lock rows of the parent they each locked first.
        changes.sort_by_cached_key(|(index, _)| {
            let repeater = &level.repeaters[*index];
            let declared = self.relations.iter().position(|r| r.name == repeater.name);
            (ctx.registry.lock_rank(repeater.target), declared)
        });
        for (index, change) in changes {
            let repeater = &level.repeaters[index];
            let child = ctx.registry.get(repeater.target).ok_or(AppError::NotFound)?;
            let target = RowTarget {
                parent_slug: R::SLUG,
                relation: repeater.name,
                field: repeater.field,
                parent_key: level.key,
                order_column: repeater.order_column.as_deref(),
                inputs: &repeater.inputs,
                min_items: repeater.spec.min_items,
                max_items: repeater.spec.max_items,
                addable: repeater.spec.addable,
                deletable: repeater.spec.deletable,
                rules: repeater.rules(),
                root: level.root,
                parent: level.parent,
                ancestors: level.ancestors,
                filters: &repeater.filters,
                budget: level.budget,
                holder: level.holder,
                holder_op: level.op,
                validate: repeater.spec.validate.as_ref().map(|r| &r.0),
            };
            out.push((repeater.name, child.write_rows(ctx, txn, target, change).await?));
        }
        Ok(out)
    }

    /// The parent's history entries of its rows' changes, one per relation
    /// (each child has its own entries).
    pub(super) async fn audit_rows(
        &self,
        ctx: &Ctx,
        txn: &sea_orm::DatabaseTransaction,
        model: &Model<R>,
        outcomes: Vec<(&'static str, RowsOutcome)>,
    ) -> AppResult<()> {
        if !self.meta.audit {
            return Ok(());
        }
        let key = self.key_of(model);
        let entries: Vec<_> = outcomes
            .into_iter()
            .filter(|(_, o)| !o.is_empty())
            .map(|(relation, o)| {
                rustcms_core::audit::AuditEntry::new("rows_changed", R::SLUG).key(&key).meta(serde_json::json!({
                    "relation": relation,
                    // Counts only (ADR 0041): which child rows changed is the child's
                    // own history, read with the child's permissions.
                    "created": o.created.len(),
                    "updated": o.updated.len(),
                    "deleted": o.deleted.len(),
                    "reordered": o.reordered,
                }))
            })
            .collect();
        rustcms_core::audit::record_all(txn, &ctx.audit_context, entries).await
    }
}

/// Whether the conditions of `repeater`'s rows, or of rows nested in them,
/// read the record's form or an enclosing row.
fn reads_outside(repeater: &RowRepeater) -> bool {
    let rules = repeater.rules();
    rules.conditions.reads_outside()
        || rules.json.values().any(crate::engine::repeater::Rows::reads_outside)
        || rules.nested.iter().any(reads_outside)
}

/// A relationship repeater's change, its shape checked, with the changes of
/// its rows' own repeaters (taken out of their data); `Err` names the change
/// that is invalid (`blanks`, `blanks.n1.choices`).
fn parse_change(
    value: serde_json::Value,
    op: Operation,
    repeater: &RowRepeater,
    registry: &crate::Registry,
    path: &str,
) -> Result<RowChange, String> {
    let invalid = || path.to_owned();
    let child = registry.get(repeater.target).ok_or_else(invalid)?;
    let object = value.as_object().ok_or_else(invalid)?;
    if !object.keys().all(|k| matches!(k.as_str(), "create" | "update" | "delete" | "order")) {
        return Err(invalid());
    }
    let list = |part: &str| -> Result<Vec<serde_json::Value>, String> {
        match object.get(part) {
            None => Ok(Vec::new()),
            Some(serde_json::Value::Array(items)) if items.len() <= MAX_ROW_ITEMS => Ok(items.clone()),
            Some(_) => Err(invalid()),
        }
    };
    let (create, update, delete) = (list("create")?, list("update")?, list("delete")?);
    if op == Operation::Create && (!update.is_empty() || !delete.is_empty()) {
        return Err(invalid());
    }
    let valid_ref = |r: &str| {
        (1..=16).contains(&r.len()) && r.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    };
    // The changes of the row's own repeaters (part C): a new row's create its rows.
    let nested = |id: &str, data: &mut Data, op: Operation| -> Result<Nested, String> {
        let Some(rules) = repeater.rules.get() else { return Ok(Vec::new()) };
        let mut out = Vec::new();
        for (index, inner) in rules.nested.iter().enumerate() {
            if let Some(change) = data.remove(inner.name) {
                out.push((index, parse_change(change, op, inner, registry, &format!("{path}.{id}.{}", inner.name))?));
            }
        }
        Ok(out)
    };
    let mut change = RowChange::default();
    let mut refs = HashSet::new();
    for item in create {
        let reference = item.get("ref").and_then(|r| r.as_str()).filter(|r| valid_ref(r)).ok_or_else(invalid)?;
        let mut data = item.get("data").and_then(|d| d.as_object()).ok_or_else(invalid)?.clone();
        if !refs.insert(reference.to_owned()) {
            return Err(invalid());
        }
        let rows = nested(reference, &mut data, Operation::Create)?;
        change.create.push((reference.to_owned(), data, rows));
    }
    let mut keys = HashSet::new();
    for item in update {
        let key = item.get("key").and_then(|k| k.as_str()).and_then(|k| child.canonical_key(k)).ok_or_else(invalid)?;
        let mut data = item.get("data").and_then(|d| d.as_object()).ok_or_else(invalid)?.clone();
        if !keys.insert(key.clone()) {
            return Err(invalid());
        }
        let rows = nested(&key, &mut data, Operation::Update)?;
        change.update.push((key, data, rows));
    }
    for item in delete {
        let key = item.as_str().and_then(|k| child.canonical_key(k)).ok_or_else(invalid)?;
        if !keys.insert(key.clone()) {
            return Err(invalid());
        }
        change.delete.push(key);
    }
    if let Some(order) = object.get("order") {
        let items = order.as_array().filter(|a| a.len() <= MAX_ORDERED_ROWS).ok_or_else(invalid)?;
        let mut ids = Vec::with_capacity(items.len());
        let mut seen = HashSet::new();
        for item in items {
            // A stored row by its key; a new row as `{"new": ref}`, a ref of this change.
            let id = match item {
                serde_json::Value::String(key) => RowId::Key(child.canonical_key(key).ok_or_else(invalid)?),
                serde_json::Value::Object(tag) if tag.len() == 1 => {
                    let reference = tag.get("new").and_then(|r| r.as_str()).ok_or_else(invalid)?;
                    if !refs.contains(reference) {
                        return Err(invalid());
                    }
                    RowId::New(reference.to_owned())
                }
                _ => return Err(invalid()),
            };
            if !seen.insert(id.clone()) {
                return Err(invalid());
            }
            ids.push(id);
        }
        change.order = Some(ids);
    }
    Ok(change)
}
