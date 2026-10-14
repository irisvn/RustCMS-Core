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
                &serde_json::Value::Object(input.clone()),
            )
        })
    }

    /// The wire value of field `field` for the parent key `key`.
    pub(super) fn fk_value(&self, field: &str, key: &str) -> serde_json::Value {
        self.column(field).kind.from_query(&serde_json::Value::String(key.to_owned()))
    }

    /// A rich text field holds a valid document.
    pub(super) fn check_rich_text(&self, name: &str, value: &serde_json::Value, errors: &mut ValidationErrors) -> bool {
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
    pub(super) fn check_key_value(&self, name: &str, value: &serde_json::Value, errors: &mut ValidationErrors) -> bool {
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

    /// Writes trusted data (validated input, possibly changed by hooks) into the active model.
    pub(super) fn apply(
        &self,
        model: &mut <Entity<R> as EntityTrait>::ActiveModel,
        data: &Data,
        op: Operation,
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

    pub(super) fn map_db_err(&self, err: DbErr) -> AppError {
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
}
