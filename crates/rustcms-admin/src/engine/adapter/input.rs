//! Accepting input: field rules, values written to the model, idempotency
//! claims and database errors mapped to field errors.

use super::*;

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
            options: None,
        }
    }

    /// The static options client input of field `name` must match, if any.
    pub(super) fn input_options(&self, name: &str) -> Option<Options<'_>> {
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
            // Static options bound client input only: hooks may write any value.
            let options = self.input_options(name);
            let rule = InputRule { options, ..self.rule(name, field.required) };
            if check_input(name, &rule, value, &mut errors)
                && !value.is_null()
                && let Some(Options::One { scale, .. }) = options
                && let Ok(value) = crate::wire::canonical(rule.kind, scale, value)
            {
                canonical.push((name.clone(), value));
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
        let mut input = input;
        input.extend(canonical);
        Ok(input)
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
            if !check_written(name, &self.rule(name, false), value, &mut errors) {
                continue;
            }
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
            _ => err.into(),
        }
    }
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
            let scale = crate::wire::decimal_scale(column.column.def().get_column_type());
            for option in options.iter_mut() {
                let canonical = crate::wire::canonical(column.kind, scale, &option.value).unwrap_or_else(|e| {
                    panic!(
                        "resource `{slug}`: option {} of select `{name}` is not a value of its column ({}): {e}",
                        option.value,
                        column.kind.name()
                    )
                });
                let same = column.kind.decode(&canonical).ok().and_then(|c| c.into_iter().next())
                    == column.kind.decode(&option.value).ok().and_then(|c| c.into_iter().next());
                assert!(
                    same,
                    "resource `{slug}`: option {} of select `{name}` would be stored as {canonical}",
                    option.value
                );
                if let Err(problem) = storable(column.column.def().get_column_type(), column.kind, &canonical) {
                    panic!("resource `{slug}`: option {} of select `{name}` {problem}", option.value);
                }
                option.value = canonical;
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

/// Whether the column stores `canonical` (a select option in canonical form)
/// exactly, so that the value read back is still that option: an `f32`
/// keeps few digits, timestamps keep microseconds (PostgreSQL, MySQL), a
/// decimal at most its precision.
fn storable(
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
