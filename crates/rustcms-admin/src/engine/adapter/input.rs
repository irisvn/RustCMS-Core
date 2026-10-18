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

    /// Validates client input against field permissions and types.
    pub(super) fn accept_input(&self, ctx: &Ctx, op: Operation, input: Data) -> AppResult<Data> {
        let mut errors = ValidationErrors::new();
        let natural_create = op == Operation::Create && matches!(self.meta.key, KeyStrategy::Natural);
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
            check_input(name, &self.rule(name, field.required), value, &mut errors);
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
