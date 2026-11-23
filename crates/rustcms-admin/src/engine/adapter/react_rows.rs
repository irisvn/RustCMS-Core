//! Form reactions on relationship rows (ADR 0044, part D): the rows of the
//! form's top-level relationship repeaters in a reaction's state, and the
//! row patches kept only when the user could make the change.
//!
//! The state's rows are the client's: tagged identities (`RowId::Key` for a
//! stored row, `RowId::Ref` for a new one), and only the repeater's inputs
//! the user may read. A patch naming a stored row is checked against the
//! record read (without a lock) through the child's scope, view policy,
//! parent membership and the relation's filters; its values against the
//! child's field rules for the operation the save would run; the row's
//! conditions on authorized values. Nothing is written: the save checks
//! everything again.

use serde_json::Value;

use super::rows::{RowRepeater, RowRules, in_relation};
use super::*;
use crate::engine::conditions::OuterValues;
use crate::forms::valid_row_ref;
use crate::ui::{FormPatch, RowId, StateRow};

/// A relationship repeater's relation as a reaction's checks read it.
pub(crate) struct RowsParent<'a> {
    /// The child's field holding the parent key.
    pub(crate) field: &'a str,
    /// The parent record's key (`None` on a parent create: no key yet).
    pub(crate) parent_key: Option<&'a str>,
    pub(crate) order_column: Option<&'a str>,
    /// The child fields the rows edit.
    pub(crate) inputs: &'a [String],
    pub(crate) filters: &'a [(&'static str, RelationFilter)],
    pub(crate) rules: &'a RowRules,
}

/// A stored row a keyed patch names, read for the user (ADR 0044, part D).
#[derive(Debug, Clone)]
pub(crate) struct ReactionRow {
    /// The values of the fields the user may read.
    pub(crate) values: Data,
    /// Whether the user may update it (`update` admitted and allowed on it).
    pub(crate) update: bool,
    /// Whether the user may delete it.
    pub(crate) delete: bool,
}

/// A row patch's values to check, as the save would write them.
pub(crate) struct RowPatch<'a> {
    pub(crate) parent: &'a RowsParent<'a>,
    /// `Create` for a new row, `Update` for a stored one.
    pub(crate) op: Operation,
    /// A stored row's readable values (`ReactionRow::values`).
    pub(crate) stored: Option<&'a Data>,
    /// The row as the state holds it (after the previous patches).
    pub(crate) row: &'a Data,
    /// The patch's values.
    pub(crate) data: &'a Data,
    /// The record's form as its conditions leave it (`when_root`).
    pub(crate) root: &'a Data,
}

/// What the user may do with a relationship repeater's rows.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct RowsAbility {
    pub(super) create: bool,
    pub(super) update: bool,
    pub(super) delete: bool,
}

impl RowsAbility {
    pub(super) fn any(self) -> bool {
        self.create || self.update || self.delete
    }
}

/// The relationship rows a reaction reads and its patches change.
#[derive(Default)]
pub(super) struct ReactionRows {
    /// The repeaters the user's form holds (index in `row_repeaters`), with
    /// what the user may do with their rows.
    pub(super) held: Vec<(usize, RowsAbility)>,
    /// Their rows, as the state sent them and the patches leave them.
    pub(super) rows: HashMap<String, Vec<StateRow>>,
    /// The stored rows keyed patches name, by repeater and key.
    loaded: HashMap<&'static str, HashMap<String, ReactionRow>>,
    /// The parent record's key (edit form).
    key: Option<String>,
}

/// Why a patch is dropped.
type Dropped = (String, &'static str);

impl<R: Resource> ResourceAdapter<R> {
    // ----- The child's side -----

    /// The value of a state row's input `name`, as a reaction reads it: a
    /// field the user may read (not encrypted) whose value decodes, canonical.
    pub(super) fn state_row_value_in(&self, ctx: &Ctx, name: &str, value: &Value) -> Option<Value> {
        if !self.state_field(ctx, name) {
            return None;
        }
        if value.is_null() {
            return Some(Value::Null);
        }
        self.column(name).kind.decode(value).ok()?;
        Some(self.canonical_value(name, value))
    }

    /// A row patch's value of field `name` in `op` (ADR 0044, part D): a
    /// field the user may read and write in `op`, not encrypted, a value the
    /// input accepts (the child's options and bounds; `null` under a
    /// condition); canonical. `None` otherwise.
    pub(super) fn accept_row_value(
        &self,
        ctx: &Ctx,
        op: Operation,
        name: &str,
        value: &Value,
        conditional: bool,
    ) -> Option<Value> {
        let field = self.field(name)?;
        if field.secret.is_some() || !field.can_read(ctx) || !self.writable(ctx, op, name) {
            return None;
        }
        let mut rule = InputRule {
            options: self.input_options(name),
            bounds: self.number_bounds.get(name).copied(),
            ..self.rule(name, false)
        };
        rule.nullable |= conditional;
        let mut errors = ValidationErrors::new();
        if !check_input(name, &rule, value, &mut errors) {
            return None;
        }
        // Computed options: not the declared ones, but a checkbox list's shape holds.
        if self.computed_list_problem(name, value).is_some() {
            return None;
        }
        if value.is_null() {
            return Some(Value::Null);
        }
        match self.repeaters.get(name) {
            Some(rows) => rows.check(name, value, &mut errors),
            None => Some(self.canonical_value(name, value)),
        }
    }

    /// A row patch's values as the save would write them, or why it is
    /// dropped: each an input of the rows (never the parent key, a filtered
    /// field or the order column) the user may read and write in `op`, with
    /// a value its input accepts; then the inputs hidden by the rows'
    /// conditions left out. The conditions read authorized values (the
    /// stored row's readable values, or the declared defaults of a new row),
    /// the state's row and the patch over them, then the server's values
    /// (the bound parent key, `null` on a parent create, and the relation's
    /// fixed values), as the save does before conditions; legacy rows (no
    /// conditions of their own) the child form's conditions over its inputs,
    /// explicit rows theirs with the record's form as `root`.
    pub(super) fn clean_row_patch_in(&self, ctx: &Ctx, patch: RowPatch<'_>) -> Result<Data, &'static str> {
        let parent = patch.parent;
        let legacy = parent.rules.legacy;
        let conditions = if legacy { &self.conditions } else { &parent.rules.conditions };
        let mut out = Data::new();
        for (name, value) in patch.data {
            if !parent.inputs.contains(name) {
                return Err("an input the rows do not edit");
            }
            if name == parent.field
                || parent.order_column == Some(name.as_str())
                || parent.filters.iter().any(|(f, _)| f == name)
            {
                return Err("a value the relation or the order sets");
            }
            let value = self
                .accept_row_value(ctx, patch.op, name, value, conditions.is_conditional(name))
                .ok_or("not readable and writable by the user, or a value its input refuses")?;
            out.insert(name.clone(), value);
        }
        let fixed = self.filter_values(parent.filters).map_err(|_| "a relation filter")?;
        let inputs =
            if legacy { self.form.inputs() } else { crate::engine::repeater::row_inputs(&parent.rules.components) };
        let mut values = self.condition_values_of(ctx, patch.op, patch.stored.map(|s| (s, true)), &inputs);
        values.extend(patch.row.iter().map(|(k, v)| (k.clone(), v.clone())));
        values.extend(out.iter().map(|(k, v)| (k.clone(), v.clone())));
        let parent_value = parent.parent_key.map_or(Value::Null, |key| self.fk_value(parent.field, key));
        values.insert(parent.field.to_owned(), parent_value);
        values.extend(fixed.into_iter().map(|(f, v)| (f.to_owned(), v)));
        let hidden = if legacy {
            self.conditions.hidden(&values)
        } else {
            conditions.hidden_in(&values, OuterValues { parent: None, root: Some(patch.root) })
        };
        out.retain(|name, _| !hidden.contains(name));
        Ok(out)
    }

    /// The stored rows of `keys` a keyed patch may name (ADR 0044, part D):
    /// read without a lock, in one query in key order, through the child's
    /// scope (not trashed), the parent's rows of the relation (its key and
    /// filters), and the view policy on each; with what the user may do on
    /// each. Keys not found are left out.
    pub(super) async fn reaction_rows_in(
        &self,
        ctx: &Ctx,
        parent: &RowsParent<'_>,
        keys: &[String],
    ) -> AppResult<HashMap<String, ReactionRow>> {
        let mut out = HashMap::new();
        let Some(parent_key) = parent.parent_key else { return Ok(out) };
        let values = self.decode_keys(keys)?;
        if values.is_empty() {
            return Ok(out);
        }
        let parent_value = self.fk_value(parent.field, parent_key);
        let models = self
            .base(ctx, Trashed::Without)
            .filter(self.relation_condition(parent.field, parent.filters, &parent_value)?)
            .filter(self.pk.is_in(values))
            .order_by_asc(self.pk)
            .all(&ctx.db)
            .await?;
        let fixed = self.filter_values(parent.filters)?;
        let may_update = self.policy.admits(ctx, ability::UPDATE).await;
        let may_delete = self.policy.admits(ctx, ability::DELETE).await;
        for model in models {
            let record = self.record(&model);
            if !in_relation(&record, parent.field, &parent_value, &fixed)
                || !self.policy.allows(ctx, ability::VIEW, Some(&record)).await
            {
                continue;
            }
            let update = may_update && self.policy.allows(ctx, ability::UPDATE, Some(&record)).await;
            let delete = may_delete && self.policy.allows(ctx, ability::DELETE, Some(&record)).await;
            let values = record.iter().filter(|(n, _)| self.state_field(ctx, n)).map(|(k, v)| (k.clone(), v.clone()));
            out.insert(self.key_of(&model), ReactionRow { values: values.collect(), update, delete });
        }
        Ok(out)
    }

    // ----- The parent's side -----

    /// The relationship repeaters the user's form for `op` holds (relation
    /// readable, child listable: `rows_shown`), with what the user may do
    /// with their rows as the schema and the save decide: add (the relation
    /// `creatable`, the child's `create` admitted and allowed with the parent
    /// bound, or without a parent on a create), update and delete (edit form,
    /// admitted; each row is checked on its own).
    pub(super) async fn held_rows(&self, ctx: &Ctx, op: Operation, key: Option<&str>) -> ReactionRows {
        let mut out = ReactionRows { key: key.map(str::to_owned), ..Default::default() };
        for (index, repeater) in self.row_repeaters.iter().enumerate() {
            if !self.rows_shown(ctx, repeater).await {
                continue;
            }
            let Some(ability) = self.rows_ability(ctx, repeater, op == Operation::Update, key).await else { continue };
            out.held.push((index, ability));
        }
        out
    }

    /// What the user may do with the rows of `repeater` (the schema and
    /// reactions decide alike, as the save does): add (the relation
    /// `creatable`, the child's `create` admitted and allowed with the parent
    /// `key` bound on an edit form, without a parent on a create), update and
    /// delete (`edit`, admitted; each row is checked on its own).
    pub(super) async fn rows_ability(
        &self,
        ctx: &Ctx,
        repeater: &RowRepeater,
        edit: bool,
        key: Option<&str>,
    ) -> Option<RowsAbility> {
        let child = ctx.registry.get(repeater.target)?;
        let creatable = self.relation_of_rows(ctx, repeater).is_some_and(|a| a.creatable);
        let mut bound = ctx.clone();
        bound.bound = key.filter(|_| edit).map(|key| (R::SLUG.to_owned(), repeater.field.to_owned(), key.to_owned()));
        let create =
            creatable && child.admits(ctx, ability::CREATE).await && child.allows(&bound, ability::CREATE).await;
        let update = edit && child.admits(ctx, ability::UPDATE).await;
        let delete = edit && child.admits(ctx, ability::DELETE).await;
        Some(RowsAbility { create, update, delete })
    }

    /// The held repeater `name`, if the user's form holds it.
    pub(super) fn held_repeater(&self, held: &ReactionRows, name: &str) -> Option<(&RowRepeater, RowsAbility)> {
        held.held
            .iter()
            .map(|(index, ability)| (&self.row_repeaters[*index], *ability))
            .find(|(repeater, _)| repeater.name == name)
    }

    /// The rows sent for relationship repeater `repeater` as a reaction reads
    /// them, or `None` (left out of the state): a list of at most
    /// `[admin] repeater_rows` rows, each `{"key": …}` (a stored row, edit
    /// form only) or `{"ref": "n1"}` with its `data`, no row twice; in each,
    /// the repeater's inputs the user may read whose values decode, canonical.
    pub(super) fn state_rows(
        &self,
        ctx: &Ctx,
        op: Operation,
        repeater: &RowRepeater,
        value: &Value,
    ) -> Option<Vec<StateRow>> {
        let items = value.as_array()?;
        if items.len() > ctx.registry.repeater_rows() {
            return None;
        }
        let child = ctx.registry.get(repeater.target)?;
        let mut out = Vec::with_capacity(items.len());
        let mut seen = HashSet::new();
        for item in items {
            let object = item.as_object()?;
            if !object.keys().all(|k| matches!(k.as_str(), "key" | "ref" | "data")) {
                return None;
            }
            let id = match (object.get("key"), object.get("ref")) {
                (Some(Value::String(key)), None) if op == Operation::Update => RowId::Key(child.canonical_key(key)?),
                (None, Some(Value::String(reference))) if valid_row_ref(reference) => RowId::Ref(reference.clone()),
                _ => return None,
            };
            if !seen.insert(id.clone()) {
                return None;
            }
            let data = match object.get("data") {
                None => Data::new(),
                Some(Value::Object(data)) => data
                    .iter()
                    .filter(|(name, _)| repeater.inputs.contains(name))
                    .filter_map(|(name, value)| child.state_row_value(ctx, name, value).map(|v| (name.clone(), v)))
                    .collect(),
                Some(_) => return None,
            };
            out.push(StateRow { id, data });
        }
        Some(out)
    }

    /// Reads the stored rows the keyed patches among `patches` name (rows of
    /// the state only; updates, removals and flags), one query per repeater
    /// (ADR 0044, part D). `patches` are those an answer may hold.
    pub(super) async fn load_patched_rows(
        &self,
        ctx: &Ctx,
        work: &mut ReactionRows,
        patches: &[FormPatch],
    ) -> AppResult<()> {
        let Some(key) = work.key.clone() else { return Ok(()) };
        for (index, ability) in work.held.clone() {
            let repeater = &self.row_repeaters[index];
            let (Some(rows), Some(child)) = (work.rows.get(repeater.name), ctx.registry.get(repeater.target)) else {
                continue;
            };
            if !ability.any() {
                continue;
            }
            let mut keys: Vec<String> = patches
                .iter()
                .filter_map(|patch| match patch {
                    FormPatch::UpdateRelationRow { field, id: RowId::Key(key), .. }
                    | FormPatch::RemoveRelationRow { field, id: RowId::Key(key) }
                    | FormPatch::FlagRelationRow { field, id: RowId::Key(key), .. }
                        if field == repeater.name =>
                    {
                        child.canonical_key(key)
                    }
                    _ => None,
                })
                .filter(|key| rows.iter().any(|row| row.key() == Some(key.as_str())))
                .collect();
            keys.sort();
            keys.dedup();
            if keys.is_empty() {
                continue;
            }
            let parent = self.rows_parent(repeater, Some(&key));
            let loaded = child.reaction_rows(ctx, &parent, &keys).await?;
            work.loaded.insert(repeater.name, loaded);
        }
        Ok(())
    }

    fn rows_parent<'a>(&self, repeater: &'a RowRepeater, key: Option<&'a str>) -> RowsParent<'a> {
        RowsParent {
            field: repeater.field,
            parent_key: key,
            order_column: repeater.order_column.as_deref(),
            inputs: &repeater.inputs,
            filters: &repeater.filters,
            rules: repeater.rules(),
        }
    }

    /// A row patch (ADR 0044, part D) as the panel applies it (`None`: it
    /// changes nothing), applied to `work`'s rows; the field and why it is
    /// dropped otherwise. The repeater must be one the user's form holds,
    /// whose rows the user may change, shown on `values` (the form as the
    /// previous patches left it); the row a patch names, one of the state's.
    pub(super) fn keep_relation_patch(
        &self,
        ctx: &Ctx,
        op: Operation,
        values: &Data,
        work: &mut ReactionRows,
        patch: FormPatch,
    ) -> Result<Option<FormPatch>, Dropped> {
        let field = patch.field().unwrap_or_default().to_owned();
        let fail = |reason: &'static str| (field.clone(), reason);
        let (repeater, ability) = self
            .held_repeater(work, &field)
            .ok_or_else(|| fail("not a relationship repeater the user's form holds"))?;
        if !ability.any() {
            return Err(fail("rows the user may not change"));
        }
        if self.conditions.hidden(values).contains(&field) {
            return Err(fail("hidden by its conditions"));
        }
        let child = ctx.registry.get(repeater.target).ok_or_else(|| fail("an unknown child resource"))?;
        let ReactionRows { rows, loaded, key, .. } = work;
        let list = rows.get_mut(&field).ok_or_else(|| fail("rows the form did not send"))?;
        let parent = self.rows_parent(repeater, key.as_deref());
        let root = self.conditions.shown_values(values, Default::default());
        let clean = |op: Operation, stored: Option<&Data>, row: &Data, data: &Data| {
            child.clean_row_patch(ctx, RowPatch { parent: &parent, op, stored, row, data, root: &root })
        };
        // A row of the state, by its identity (a key canonical, a ref valid).
        let at = |list: &[StateRow], id: &RowId| -> Result<(RowId, usize), Dropped> {
            let id = match id {
                RowId::Key(key) if op == Operation::Update => child.canonical_key(key).map(RowId::Key),
                RowId::Key(_) => None,
                RowId::Ref(reference) => valid_row_ref(reference).then(|| RowId::Ref(reference.clone())),
            }
            .ok_or_else(|| fail("a row id that names no row"))?;
            let index = list.iter().position(|r| r.id == id).ok_or_else(|| fail("a row that is not in the form"))?;
            Ok((id, index))
        };
        let stored = |id: &RowId| -> Result<&ReactionRow, Dropped> {
            id.key()
                .and_then(|k| loaded.get(repeater.name).and_then(|rows| rows.get(k)))
                .ok_or_else(|| fail("a stored row not found among the user's rows of the relation"))
        };
        Ok(Some(match patch {
            FormPatch::AddRelationRow { reference, data, .. } => {
                if !ability.create || !repeater.spec.addable {
                    return Err(fail("rows the user may not add"));
                }
                let max = repeater.spec.max_items.map_or(usize::MAX, |m| m as usize).min(ctx.registry.repeater_rows());
                if list.len() >= max {
                    return Err(fail("more rows than the repeater allows"));
                }
                if let Some(reference) = &reference
                    && (!valid_row_ref(reference) || list.iter().any(|r| r.reference() == Some(reference.as_str())))
                {
                    return Err(fail("a reference that is not valid or names a row of the state"));
                }
                let data = clean(Operation::Create, None, &Data::new(), &data).map_err(fail)?;
                // A row added without a reference cannot be named by later patches.
                let id = RowId::Ref(reference.clone().unwrap_or_default());
                list.push(StateRow { id, data: data.clone() });
                FormPatch::AddRelationRow { field, reference, data }
            }
            FormPatch::UpdateRelationRow { id, data, .. } => {
                let (id, index) = at(list, &id)?;
                let (row_op, values) = match &id {
                    RowId::Ref(_) if ability.create => (Operation::Create, None),
                    RowId::Ref(_) => return Err(fail("rows the user may not add")),
                    RowId::Key(_) => {
                        let row = stored(&id)?;
                        if !ability.update || !row.update {
                            return Err(fail("a row the user may not change"));
                        }
                        (Operation::Update, Some(&row.values))
                    }
                };
                let row = &mut list[index];
                let cleaned = clean(row_op, values, &row.data, &data).map_err(fail)?;
                let changed: Data = cleaned.into_iter().filter(|(n, v)| row.data.get(n) != Some(v)).collect();
                if changed.is_empty() {
                    return Ok(None);
                }
                row.data.extend(changed.iter().map(|(k, v)| (k.clone(), v.clone())));
                FormPatch::UpdateRelationRow { field, id, data: changed }
            }
            FormPatch::RemoveRelationRow { id, .. } => {
                let (id, index) = at(list, &id)?;
                if id.key().is_some() {
                    let row = stored(&id)?;
                    if !ability.delete || !row.delete {
                        return Err(fail("a row the user may not remove"));
                    }
                }
                if !repeater.spec.deletable {
                    return Err(fail("rows the repeater does not let remove"));
                }
                if repeater.spec.min_items.is_some_and(|min| list.len() <= min as usize) {
                    return Err(fail("fewer rows than the repeater needs"));
                }
                list.remove(index);
                FormPatch::RemoveRelationRow { field, id }
            }
            FormPatch::FlagRelationRow { id, message, .. } => {
                let (id, _) = at(list, &id)?;
                // A stored row is one of the user's rows of the relation, as for an update.
                if id.key().is_some() {
                    stored(&id)?;
                }
                let message = ctx.t_in(R::SLUG, &message);
                if message.chars().count() > super::react::MAX_MESSAGE {
                    return Err(fail("a message longer than 500 characters"));
                }
                FormPatch::FlagRelationRow { field, id, message }
            }
            _ => unreachable!("only relationship row patches"),
        }))
    }
}
