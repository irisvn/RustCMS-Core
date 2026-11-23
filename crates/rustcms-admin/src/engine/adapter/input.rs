//! Accepting input: field rules, values written to the model, idempotency
//! claims and database errors mapped to field errors.

use super::*;
use crate::engine::conditions::OuterValues;
use crate::engine::validate::Bounds;
use crate::ui::NumberBounds;

impl<R: Resource> ResourceAdapter<R> {
    /// The idempotency claim of a create (through a relation: of that parent).
    pub(super) fn create_claim(&self, ctx: &Ctx, input: &Data) -> Option<crate::idempotency::Claim> {
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
                &serde_json::Value::Object(self.claim_input(ctx, input)),
            )
        })
    }

    /// The wire value of field `field` for the parent key `key`.
    pub(super) fn fk_value(&self, field: &str, key: &str) -> serde_json::Value {
        self.column(field).kind.from_query(&serde_json::Value::String(key.to_owned()))
    }

    /// The rule a field's values satisfy (`required` only for client input).
    pub(super) fn rule(&self, name: &str, required: bool) -> InputRule<'_> {
        let column = self.column(name);
        let field = self.field(name);
        InputRule {
            kind: column.kind,
            nullable: column.nullable,
            required,
            max_length: field.and_then(|f| f.max_length),
            key_value: self.key_values.get(name).copied(),
            rich_text: field.is_some_and(|f| f.rich_text),
            rich_nodes: field.and_then(|f| self.accepts(f)),
            options: None,
            bounds: None,
        }
    }

    /// The application nodes rich text field `field` accepts, if any.
    pub(super) fn accepts<'a>(&'a self, field: &'a crate::field::Field) -> Option<crate::rich_text::Accepts<'a>> {
        let nodes = self.rich_nodes.get()?;
        (field.rich_text && !field.rich_nodes.is_empty())
            .then(|| crate::rich_text::Accepts { nodes, only: Some(&field.rich_nodes) })
    }

    /// The static options client input of field `name` must match, if any.
    /// Not for an input whose options are computed (ADR 0044, part E): its
    /// answer is checked instead, before the save.
    pub(super) fn input_options(&self, name: &str) -> Option<Options<'_>> {
        if self.computes_options(name) {
            return None;
        }
        self.static_options.get(name).map(|options| {
            if options.many {
                Options::Many(&options.values)
            } else {
                let scale = crate::wire::decimal_scale(self.column(name).column.def().get_column_type());
                Options::One { values: &options.values, scale }
            }
        })
    }

    /// Validates client input against field permissions and types.
    pub(super) fn accept_input(&self, ctx: &Ctx, op: Operation, input: Data) -> AppResult<Data> {
        self.accept_input_in(ctx, op, input, &self.conditions)
    }

    /// As [`Self::accept_input`], `conditions` deciding which inputs are
    /// conditional (a relationship row's, ADR 0044).
    pub(super) fn accept_input_in(
        &self,
        ctx: &Ctx,
        op: Operation,
        input: Data,
        conditions: &crate::engine::conditions::FormConditions,
    ) -> AppResult<Data> {
        let mut errors = ValidationErrors::new();
        let natural_create = op == Operation::Create && matches!(self.meta.key, KeyStrategy::Natural);
        // Select values as stored: their canonical form (a decimal rounded to
        // the column's scale is the option it matched, not what was sent).
        let mut canonical = Vec::new();
        for (name, value) in &input {
            let allowed =
                self.field(name).filter(|f| f.can_write(op, ctx)).filter(|_| *name != self.pk_name || natural_create);
            let Some(field) = allowed else {
                errors.add(name.as_str(), "is not allowed");
                continue;
            };
            if let Some(kind) = field.secret {
                let nullable = self.column(name).nullable && !field.required;
                secrets::check_operation(name, kind, op, nullable, value, &mut errors);
                continue;
            }
            // Static options and number bounds bound client input only: hooks may write any value.
            let options = self.input_options(name);
            let bounds = self.number_bounds.get(name).copied();
            // Under a condition, `required` (and a non-nullable column) means
            // "when shown": checked once visibility is known (`apply_conditions`).
            let conditional = conditions.is_conditional(name);
            let mut rule = InputRule { options, bounds, ..self.rule(name, field.required && !conditional) };
            rule.nullable |= conditional;
            let mut accepted = check_input(name, &rule, value, &mut errors);
            // Computed options (ADR 0044, part E): a checkbox list keeps its
            // shape here, a select's value is stored canonical; membership is
            // checked against the answer before the save.
            if accepted && let Some(problem) = self.computed_list_problem(name, value) {
                errors.add(name.as_str(), problem);
                accepted = false;
            }
            if accepted
                && !value.is_null()
                && rule.kind != FieldKind::Json
                && self.computes_options(name)
                && let Ok(value) = crate::wire::canonical(
                    rule.kind,
                    crate::wire::decimal_scale(self.column(name).column.def().get_column_type()),
                    value,
                )
            {
                canonical.push((name.clone(), value));
            }
            // A JSON repeater: each row checked, stored without its hidden inputs.
            if accepted && let Some(rows) = self.repeaters.get(name.as_str()) {
                if let Some(list) = rows.check(name, value, &mut errors) {
                    canonical.push((name.clone(), list));
                }
                continue;
            }
            if accepted
                && !value.is_null()
                && let Some(Options::One { scale, .. }) = options
                && let Ok(value) = crate::wire::canonical(rule.kind, scale, value)
            {
                canonical.push((name.clone(), value));
            }
        }
        if op == Operation::Create {
            // A repeater with `min_items` left out of a create has no rows.
            for (name, rows) in &self.repeaters {
                let writable = self.field(name).is_some_and(|f| f.can_write(op, ctx));
                if let Some(min) = rows.min_items()
                    && writable
                    && !input.contains_key(name)
                    && !conditions.is_conditional(name)
                {
                    errors.add(name.as_str(), format!("must have at least {min} items"));
                }
            }
            for field in
                self.fields.iter().filter(|f| f.required && f.can_write(op, ctx) && !conditions.is_conditional(&f.name))
            {
                if !input.contains_key(&field.name) {
                    errors.add(field.name.as_str(), "is required");
                }
            }
        }
        errors.into_result()?;
        let mut input = input;
        input.extend(canonical);
        Ok(input)
    }

    /// Applies the form's conditions (ADR 0037) to accepted `input`. They are
    /// evaluated on what the user's form holds ([`Self::condition_values`]),
    /// never on values the user may not see, so the outcome tells nothing
    /// about them. Values sent for hidden inputs are dropped (never stored),
    /// except the key bound by a relation (`bound`); a `clear_when_hidden`
    /// field this write hides takes its cleared value (update, users who may
    /// write it); a shown field is "required" when it is required (or not
    /// nullable) and holds `null`: sent as `null`, on create, or when the
    /// write shows it. Runs before the hooks, which write what they decide.
    /// Returns the inputs hidden before and after the user's changes.
    pub(super) fn apply_conditions(
        &self,
        ctx: &Ctx,
        op: Operation,
        input: &mut Data,
        stored: Option<(&Data, bool)>,
        bound: Option<&str>,
    ) -> AppResult<(BTreeSet<String>, BTreeSet<String>)> {
        self.apply_conditions_in(ctx, op, input, stored, bound, None)
    }

    /// As [`Self::apply_conditions`], with the conditions of a relationship
    /// row (`row`, ADR 0044) rather than the form's.
    pub(super) fn apply_conditions_in(
        &self,
        ctx: &Ctx,
        op: Operation,
        input: &mut Data,
        stored: Option<(&Data, bool)>,
        bound: Option<&str>,
        row: Option<&RowLevel<'_>>,
    ) -> AppResult<(BTreeSet<String>, BTreeSet<String>)> {
        let (conditions, inputs, outer) = match row {
            Some(row) => (row.conditions, row.inputs.clone(), OuterValues { parent: None, root: row.root }),
            None => (&self.conditions, self.form.inputs(), OuterValues::default()),
        };
        let mut conditional = conditions.conditional().peekable();
        // JSON rows whose conditions read the form wait for its values (ADR 0044).
        let waiting: Vec<&str> = self
            .repeaters
            .iter()
            .filter(|(name, rows)| row.is_none() && rows.reads_outside() && input.contains_key(name.as_str()))
            .map(|(name, _)| name.as_str())
            .collect();
        if conditional.peek().is_none() && waiting.is_empty() {
            return Ok(Default::default());
        }
        let before = self.condition_values_of(ctx, op, stored, &inputs);
        let mut after = before.clone();
        after.extend(input.iter().map(|(k, v)| (k.clone(), v.clone())));
        let hidden_before = conditions.hidden_in(&before, outer);
        let hidden = conditions.hidden_in(&after, outer);
        let mut errors = ValidationErrors::new();
        for (name, clear) in conditional {
            let Some(field) = self.field(name) else { continue };
            let writable = field.can_write(op, ctx);
            if hidden.contains(name) {
                if Some(name) != bound {
                    input.remove(name);
                }
                // Cleared when this write hides it, for a user who may write the
                // field, whatever it holds (comparing would tell what an
                // unreadable field holds). A field hidden before stays as it is:
                // for a user who cannot see what shows it, every edit would clear it.
                if clear
                    && op == Operation::Update
                    && writable
                    && !hidden_before.contains(name)
                    && let Some(value) = self.columns.get(name).and_then(cleared_value)
                {
                    input.insert(name.to_owned(), value);
                }
                continue;
            }
            let needed = field.required || !self.column(name).nullable;
            let missing = after.get(name).is_none_or(serde_json::Value::is_null);
            let checked = input.get(name).is_some_and(serde_json::Value::is_null)
                || (field.required
                    && (op == Operation::Create || (hidden_before.contains(name) && !hidden.contains(name))));
            if writable && needed && missing && checked {
                errors.add(name, "is required");
            }
        }
        if !waiting.is_empty() {
            // The form as its conditions leave it: hidden inputs read as null.
            let root = conditions.shown_values(&after, outer);
            for name in waiting {
                // Hidden repeaters were dropped above.
                let Some(list) = input.get(name) else { continue };
                if let Some(list) = self.repeaters[name].check_in(name, list, &mut errors, &root) {
                    input.insert(name.to_owned(), list);
                }
            }
        }
        errors.into_result()?;
        Ok((hidden_before, hidden))
    }

    /// The values conditions read (ADR 0037): what the user's form for `op`
    /// holds before their changes, as the panel builds it. Only fields with an
    /// input of that form; on create, the declared defaults of fields the user
    /// may read; on update, the stored values of readable fields when the user
    /// may view the record (`stored`: the locked record and that decision);
    /// anything else is the form's empty value (`false` for a toggle, `""` for
    /// required text, `[]` for lists, else `null`).
    pub(super) fn condition_values(&self, ctx: &Ctx, op: Operation, stored: Option<(&Data, bool)>) -> Data {
        self.condition_values_of(ctx, op, stored, &self.form.inputs())
    }

    /// As [`Self::condition_values`], for `inputs` (a relationship row's).
    pub(super) fn condition_values_of(
        &self,
        ctx: &Ctx,
        op: Operation,
        stored: Option<(&Data, bool)>,
        inputs: &[&Input],
    ) -> Data {
        let mut out = Data::new();
        for input in inputs.iter().copied() {
            let name = &input.name;
            // A field with several inputs holds what its last input holds (as
            // the panel's form values do).
            let Some(field) = self.field(name) else { continue };
            let is_key = *name == self.pk_name;
            let included = match op {
                Operation::Create => {
                    field.can_write(op, ctx) && (!is_key || matches!(self.meta.key, KeyStrategy::Natural))
                }
                Operation::Update => field.can_read(ctx) || (!is_key && field.can_write(op, ctx)),
            };
            if !included {
                continue;
            }
            let column = self.column(name);
            let readable = field.can_read(ctx) && field.secret.is_none();
            let value = match (op, stored) {
                (Operation::Create, _) if readable => column.default.clone(),
                (Operation::Update, Some((record, true))) if readable => record.get(name).cloned(),
                _ => None,
            };
            let value = match value {
                Some(value) if !value.is_null() && !form_blank(input.kind, column.kind, column.nullable, &value) => {
                    value
                }
                _ => form_empty(input.kind, column.kind, column.nullable),
            };
            out.insert(name.clone(), value);
        }
        out
    }

    /// Writes trusted data (validated input, possibly changed by hooks) into
    /// the active model; encrypted fields are resolved against `stored` (the
    /// locked record of an update) and sealed here.
    pub(super) fn apply(
        &self,
        ctx: &Ctx,
        model: &mut <Entity<R> as EntityTrait>::ActiveModel,
        data: &Data,
        op: Operation,
        stored: Option<&Model<R>>,
    ) -> AppResult<()> {
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
            if let Some(secret) = self.secret(name) {
                let required = self.field(name).is_some_and(|f| f.required);
                let nullable = column.nullable && !required;
                if !secrets::check_operation(name, secret.kind, op, nullable, value, &mut errors) {
                    continue;
                }
                if let Some(sealed) = self.seal_operation(ctx, name, value, op, stored, &mut errors)? {
                    model.try_set(column.column, sealed).map_err(|e| AppError::internal(format!("`{name}`: {e}")))?;
                }
                continue;
            }
            // Hooks may change values: limits hold for what is actually written.
            let rule = self.rule(name, false);
            if !check_written(name, &rule, value, &mut errors) {
                continue;
            }
            // Rich text stored complete (application nodes' defaults filled in) and in
            // the lowest version it needs (ADR 0008), whoever wrote it (client input and
            // hooks alike); copied only when something changes.
            let filled = if rule.rich_text && value.is_object() {
                crate::rich_text::parse_needed(value, rule.rich_nodes).ok().and_then(|(_, needed)| {
                    let fill = rule.rich_nodes.filter(|a| a.nodes.holds(value));
                    let lower = value.get("version").and_then(serde_json::Value::as_u64).is_some_and(|v| v > needed);
                    (fill.is_some() || lower).then(|| {
                        let mut filled = value.clone();
                        if let Some(accepts) = fill {
                            accepts.nodes.fill(&mut filled);
                        }
                        crate::rich_text::lower_version(&mut filled, needed);
                        filled
                    })
                })
            } else {
                None
            };
            let value = filled.as_ref().unwrap_or(value);
            let Ok(candidates) = column.kind.decode(value) else { continue };
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

    /// A database error as the user sees it: constraint messages declared with
    /// [`ResourceMeta::unique`] in the request's language (like hook messages).
    pub(super) fn map_db_err(&self, ctx: &Ctx, err: DbErr) -> AppError {
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
                        Some(field) => errors.add(field.as_str(), ctx.t_in(R::SLUG, &c.message)),
                        None => errors.add_form(ctx.t_in(R::SLUG, &c.message)),
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
            _ => match not_null_column(&err.to_string(), &self.table) {
                // A column that may not be null left out (not declared required, no
                // default): the field's "is required", never a server error.
                Some(column) => {
                    let mut errors = ValidationErrors::new();
                    if self.field(&column).is_some() {
                        errors.add(column.as_str(), "is required");
                    } else {
                        tracing::warn!(resource = R::SLUG, column, "a column outside the fields is not set");
                        errors.add_form("is incomplete");
                    }
                    errors.into()
                }
                None => err.into(),
            },
        }
    }
}

/// The column a NOT NULL violation names, from the database's message:
/// SQLite `NOT NULL constraint failed: table.column`, PostgreSQL `null value
/// in column "column" … violates not-null constraint`, MySQL `Field 'column'
/// doesn't have a default value` or `Column 'column' cannot be null`.
pub(crate) fn not_null_column(message: &str, table: &str) -> Option<String> {
    let between = |text: &str, start: &str, end: &str| -> Option<String> {
        let from = text.find(start)? + start.len();
        let to = text[from..].find(end)? + from;
        Some(text[from..to].to_owned())
    };
    if let Some(rest) = message.split("NOT NULL constraint failed: ").nth(1) {
        let column = rest.split(|c: char| c.is_whitespace() || c == ',').next()?;
        return Some(column.strip_prefix(&format!("{table}.")).unwrap_or(column).to_owned());
    }
    if message.contains("violates not-null constraint") {
        return between(message, "null value in column \"", "\"");
    }
    if message.contains("doesn't have a default value") {
        return between(message, "Field '", "'");
    }
    if message.contains("cannot be null") {
        return between(message, "Column '", "'");
    }
    None
}

/// The static options of a field: one of them (a select) or a list of them
/// (a checkbox list), in canonical wire form.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct FieldOptions {
    pub(super) many: bool,
    pub(super) values: Vec<serde_json::Value>,
}

impl FieldOptions {
    /// The same choice, whatever the order of the options or their labels.
    fn same(&self, other: &Self) -> bool {
        self.many == other.many
            && self.values.iter().all(|v| other.values.contains(v))
            && other.values.iter().all(|v| self.values.contains(v))
    }
}

/// The static options client writes must match, by field: from the selects
/// and checkbox lists of `form` with options, canonicalised in place (the
/// schema shows the values compared). Fields with `belongs_to` keep their
/// relation's check. Panics on declarations that cannot hold: an option that
/// is not a value of the column (or would be stored as another value), a
/// checkbox list on a non-JSON column, or a field whose inputs disagree
/// (other options, or an input without them).
pub(super) fn static_options<C: ColumnTrait>(
    slug: &str,
    form: &mut Form,
    fields: &[Field],
    columns: &HashMap<String, ColumnInfo<C>>,
) -> HashMap<String, FieldOptions> {
    // Per field: the declarations of its inputs (`None`: an input without options).
    let mut declared: HashMap<String, Vec<Option<FieldOptions>>> = HashMap::new();
    for input in form.inputs_mut() {
        let name = input.name.clone();
        let Some(field) = fields.iter().find(|f| f.name == name) else { continue };
        if field.belongs_to.is_some() || field.secret.is_some() {
            continue;
        }
        let column = &columns[&name];
        if input.kind == InputType::CheckboxList {
            assert!(
                column.kind == FieldKind::Json,
                "resource `{slug}`: checkbox list `{name}` needs a JSON column (its value is a list)"
            );
        }
        let options = match input.kind {
            InputType::Select | InputType::CheckboxList if !input.options.is_empty() => input.options.as_mut_slice(),
            _ => {
                declared.entry(name).or_default().push(None);
                continue;
            }
        };
        let many = input.kind == InputType::CheckboxList;
        if !many {
            for option in options.iter_mut() {
                option.value = canonical_option(
                    &format!("resource `{slug}`"),
                    "option",
                    &format!("select `{name}`"),
                    column,
                    &option.value,
                );
            }
        }
        let values = options.iter().map(|o| o.value.clone()).collect();
        declared.entry(name).or_default().push(Some(FieldOptions { many, values }));
    }
    let mut out = HashMap::new();
    for (name, inputs) in declared {
        let Some(first) = inputs.iter().flatten().next().cloned() else { continue };
        assert!(
            inputs.iter().all(|i| i.as_ref().is_some_and(|i| i.same(&first))),
            "resource `{slug}`: field `{name}` appears in several inputs that do not offer the same options"
        );
        out.insert(name, first);
    }
    out
}

/// `value`, an option (or a badge color's value: `noun` names which) of `what`
/// on `column`, in canonical wire form: the form clients write and schemas
/// show. An `i64` or decimal may be written as a JSON integer. Panics when
/// the column cannot hold it as it is (not a value of its kind, stored as
/// another value, or finer than the column keeps).
pub(super) fn canonical_option<C: ColumnTrait>(
    owner: &str,
    noun: &str,
    what: &str,
    column: &ColumnInfo<C>,
    value: &serde_json::Value,
) -> serde_json::Value {
    // An `i64` or a decimal written as a JSON integer reads as the string the
    // wire sends (other numbers are refused: a float may not be exact).
    let value = &match (column.kind, value) {
        (FieldKind::I64 | FieldKind::Decimal, serde_json::Value::Number(n)) if n.is_i64() || n.is_u64() => {
            serde_json::Value::String(n.to_string())
        }
        _ => value.clone(),
    };
    let scale = crate::wire::decimal_scale(column.column.def().get_column_type());
    let canonical = crate::wire::canonical(column.kind, scale, value).unwrap_or_else(|e| {
        panic!("{owner}: {noun} {value} of {what} is not a value of its column ({}): {e}", column.kind.name())
    });
    let same = column.kind.decode(&canonical).ok().and_then(|c| c.into_iter().next())
        == column.kind.decode(value).ok().and_then(|c| c.into_iter().next());
    assert!(same, "{owner}: {noun} {value} of {what} would be stored as {canonical}");
    if let Err(problem) = storable(column.column.def().get_column_type(), column.kind, &canonical) {
        panic!("{owner}: {noun} {value} of {what} {problem}");
    }
    canonical
}

/// The bounds client writes must respect, by field: from the number inputs of
/// `form` with `min`, `max` or `step`, with the scale of decimal columns.
/// Panics on declarations that cannot hold: bounds on another input or a
/// non-number column, `min` above `max`, a step that is not positive, or a
/// field whose inputs disagree.
pub(super) fn number_bounds<C: ColumnTrait>(
    slug: &str,
    form: &Form,
    columns: &HashMap<String, ColumnInfo<C>>,
) -> HashMap<String, Bounds> {
    let mut out: HashMap<String, Bounds> = HashMap::new();
    let context = format!("resource `{slug}`");
    // Every input of a field shows the same bounds, none included.
    let mut declared: HashMap<&str, Option<NumberBounds>> = HashMap::new();
    for input in form.inputs() {
        let range = input.number.as_deref().copied();
        if let Some(previous) = declared.insert(input.name.as_str(), range) {
            assert!(
                previous == range,
                "resource `{slug}`: field `{}` appears in several inputs that do not have the same bounds",
                input.name
            );
        }
    }
    for input in form.inputs() {
        let Some(range) = input.number.as_deref().copied() else { continue };
        let name = &input.name;
        assert!(
            input.kind == InputType::NumberInput,
            "resource `{slug}`: `min`, `max` and `step` are for number inputs (`{name}`)"
        );
        let column = columns.get(name);
        assert!(
            column.is_some_and(|c| matches!(
                c.kind,
                FieldKind::I32 | FieldKind::I64 | FieldKind::F64 | FieldKind::Decimal
            )),
            "resource `{slug}`: number input `{name}` with bounds needs a number column"
        );
        let column = column.expect("a number column");
        crate::engine::validate::check_bounds(&context, name, &range, column.kind);
        let def = column.column.def();
        let column_type = def.get_column_type();
        let single = matches!(column_type, sea_orm::sea_query::ColumnType::Float);
        if single {
            crate::engine::validate::check_single_bounds(&context, name, &range);
        }
        let scale = crate::wire::decimal_scale(column_type);
        out.insert(name.clone(), Bounds::new(range, scale).single(single));
    }
    out
}

/// How conditions read field `name` (ADR 0037): its column's kind and decimal
/// scale; `None` for an encrypted field (records hold it as `null`) or no column.
pub(super) fn condition_kind<C: ColumnTrait>(
    columns: &HashMap<String, ColumnInfo<C>>,
    secrets: &HashMap<String, super::secrets::SecretField>,
    name: &str,
) -> Option<(FieldKind, Option<u32>)> {
    let column = columns.get(name).filter(|_| !secrets.contains_key(name))?;
    Some((column.kind, crate::wire::decimal_scale(column.column.def().get_column_type())))
}

/// The conditions of `form` (ADR 0037). Panics, besides the checks of
/// [`FormConditions::new`](crate::engine::conditions::FormConditions::new),
/// on a condition reading an encrypted field, an input under a condition
/// whose column is not nullable and has no default (a create could not store
/// it while hidden), or a `clear_when_hidden` input without a storable
/// cleared value.
pub(super) fn form_conditions<C: ColumnTrait>(
    slug: &str,
    form: &Form,
    columns: &HashMap<String, ColumnInfo<C>>,
    secrets: &HashMap<String, super::secrets::SecretField>,
    fields: &[Field],
) -> crate::engine::conditions::FormConditions {
    let kind_of = |name: &str| condition_kind(columns, secrets, name);
    let conditions =
        crate::engine::conditions::FormConditions::new(&format!("resource `{slug}`"), &form.components, &kind_of);
    for (name, clear) in conditions.conditional() {
        let Some(column) = columns.get(name) else { continue };
        // Encrypted values never reach the conditions (records hold them as null).
        assert!(!secrets.contains_key(name), "resource `{slug}`: encrypted field `{name}` cannot have a condition");
        // Clearing a key would move the record (scope, parent) after those were checked and locked.
        assert!(
            !clear || fields.iter().find(|f| f.name == name).is_none_or(|f| f.belongs_to.is_none()),
            "resource `{slug}`: relationship field `{name}` cannot have `clear_when_hidden`"
        );
        assert!(
            column.nullable || column.has_default,
            "resource `{slug}`: input `{name}` has a condition but its column is not nullable and has no default \
             (a record created while it is hidden could not be stored): declare a default or make it nullable"
        );
        assert!(
            !clear || cleared_value(column).is_some(),
            "resource `{slug}`: input `{name}` has `clear_when_hidden` but its column (not nullable, no declared \
             default) has no empty value"
        );
    }
    conditions
}

/// The JSON repeaters of the form (ADR 0040), checked at boot; panics on a
/// repeater whose column is not JSON, rules meant for JSON rows on other
/// inputs, or repeater options on an input that is not one.
pub(super) fn json_repeaters<C: ColumnTrait>(
    slug: &str,
    form: &Form,
    columns: &HashMap<String, ColumnInfo<C>>,
    secrets: &HashMap<String, super::secrets::SecretField>,
) -> HashMap<String, crate::engine::repeater::Rows> {
    // Rows may read the form's inputs (`when_root`), as the form's own conditions do.
    let names: HashSet<&str> = form.inputs().into_iter().map(|i| i.name.as_str()).collect();
    let kind_of = |name: &str| names.contains(name).then(|| condition_kind(columns, secrets, name)).flatten();
    let mut out = HashMap::new();
    for input in form.inputs() {
        let name = input.name.as_str();
        assert!(
            !input.required && input.max_length.is_none() && !input.integer,
            "resource `{slug}`: input `{name}`: `required`, `max_length` and `integer` are for inputs of JSON \
             repeater rows (elsewhere the field decides)"
        );
        assert!(
            input.content.as_deref().is_none_or(|c| !c.has_rules() && !c.full),
            "resource `{slug}`: input `{name}`: `accept`, `max_size`, `multiple`, `collection`, `nodes` and `full` \
             are for inputs of repeater rows (elsewhere the field decides: `Field::accept`, …)"
        );
        assert!(
            (input.kind == InputType::Repeater) == input.repeater.is_some(),
            "resource `{slug}`: input `{name}`: repeater options (`schema`, `min_items`, …) are for repeaters"
        );
        let Some(spec) = input.repeater.as_deref().filter(|s| !s.relationship) else { continue };
        let column = columns.get(name).unwrap_or_else(|| panic!("resource `{slug}`: repeater `{name}` is not a field"));
        assert!(column.kind == FieldKind::Json, "resource `{slug}`: repeater `{name}` needs a JSON column");
        assert!(
            spec.order_column.is_none(),
            "resource `{slug}`: repeater `{name}`: `order_column` is for relationship repeaters"
        );
        out.insert(
            name.to_owned(),
            crate::engine::repeater::Rows::compile(&format!("resource `{slug}`"), name, spec, Some(&kind_of)),
        );
    }
    out
}

/// The conditions a relationship row's write applies (ADR 0044): the
/// repeater's, on the row's inputs, reading the record's form as `root`.
pub(crate) struct RowLevel<'a> {
    pub(crate) conditions: &'a crate::engine::conditions::FormConditions,
    pub(crate) inputs: Vec<&'a Input>,
    pub(crate) root: Option<&'a Data>,
}

/// What `clear_when_hidden` writes: the declared default, else `null`, else
/// an empty text or list.
/// Whether the panel's form reads `value` as no value: `""` in a text input
/// whose column is nullable (or not text) is sent as `null`.
pub(crate) fn form_blank(input: InputType, kind: FieldKind, nullable: bool, value: &serde_json::Value) -> bool {
    value.as_str() == Some("")
        && matches!(
            input,
            InputType::TextInput
                | InputType::Textarea
                | InputType::NumberInput
                | InputType::DatePicker
                | InputType::DateTimePicker
                | InputType::Hidden
        )
        && (nullable || kind != FieldKind::String)
}

/// What the panel's form holds for an input without a value, on the wire
/// (`admin-ui/src/schema/form.ts`, `toFormValue` then `fromFormValue`).
pub(crate) fn form_empty(input: InputType, kind: FieldKind, nullable: bool) -> serde_json::Value {
    match input {
        InputType::Toggle | InputType::Checkbox if !nullable => serde_json::Value::Bool(false),
        InputType::CheckboxList => serde_json::Value::Array(Vec::new()),
        InputType::FileUpload if kind == FieldKind::Json => serde_json::Value::Array(Vec::new()),
        InputType::TextInput
        | InputType::Textarea
        | InputType::NumberInput
        | InputType::DatePicker
        | InputType::DateTimePicker
        | InputType::Hidden
            if !nullable && kind == FieldKind::String =>
        {
            serde_json::Value::String(String::new())
        }
        _ => serde_json::Value::Null,
    }
}

pub(super) fn cleared_value<C>(column: &ColumnInfo<C>) -> Option<serde_json::Value> {
    if let Some(default) = &column.default {
        Some(default.clone())
    } else if column.nullable {
        Some(serde_json::Value::Null)
    } else {
        match column.kind {
            FieldKind::String => Some(serde_json::Value::String(String::new())),
            FieldKind::Json => Some(serde_json::Value::Array(Vec::new())),
            _ => None,
        }
    }
}

/// Whether the column stores `canonical` (a select option in canonical form)
/// exactly, so that the value read back is still that option: an `f32`
/// keeps few digits, timestamps keep microseconds (PostgreSQL, MySQL), a
/// decimal at most its precision.
pub(super) fn storable(
    column_type: &sea_orm::sea_query::ColumnType,
    kind: FieldKind,
    canonical: &serde_json::Value,
) -> Result<(), String> {
    use chrono::Timelike;
    use sea_orm::sea_query::ColumnType;
    let Some(value) = kind.decode(canonical).ok().and_then(|c| c.into_iter().next()) else { return Ok(()) };
    match (column_type, value) {
        (ColumnType::Float, Value::Double(Some(n))) => {
            let narrow = f64::from(n as f32);
            if narrow == n { Ok(()) } else { Err(format!("would be stored as {narrow} (an f32 column)")) }
        }
        (_, Value::ChronoDateTime(Some(d))) if d.nanosecond() % 1_000 != 0 => {
            Err("has a fraction of a second finer than microseconds".to_owned())
        }
        (_, Value::ChronoDateTimeUtc(Some(d))) if d.nanosecond() % 1_000 != 0 => {
            Err("has a fraction of a second finer than microseconds".to_owned())
        }
        (
            ColumnType::Decimal(Some((precision, _))) | ColumnType::Money(Some((precision, _))),
            Value::Decimal(Some(d)),
        ) if d.mantissa().unsigned_abs() >= 10u128.pow((*precision).min(38)) => {
            Err(format!("has more than {precision} digits"))
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod not_null_tests {
    use super::not_null_column;

    #[test]
    fn reads_the_column_of_each_backend_s_not_null_violation() {
        let cases = [
            ("error returned from database: (code: 1299) NOT NULL constraint failed: courses.published", "published"),
            (
                "error returned from database: null value in column \"published\" of relation \"courses\" violates \
                 not-null constraint",
                "published",
            ),
            ("error returned from database: 1364 (HY000): Field 'published' doesn't have a default value", "published"),
            ("error returned from database: 1048 (23000): Column 'published' cannot be null", "published"),
        ];
        for (message, column) in cases {
            assert_eq!(not_null_column(message, "courses").as_deref(), Some(column), "{message}");
        }
        assert_eq!(not_null_column("UNIQUE constraint failed: courses.code", "courses"), None);
    }
}
