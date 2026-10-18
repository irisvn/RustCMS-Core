//! Imports (ADR 0021): rows of a CSV file through the usual create and
//! update paths. Here: who may import, which fields, how a cell becomes a
//! value, and the record a row's import key names.

use super::*;

/// A field an import may set.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ImportField {
    /// Field name (also what a file's column is matched with).
    pub name: String,
    /// Label in the user's language.
    pub label: String,
    /// Required when creating.
    pub required: bool,
    /// The import key (finds the record to update).
    pub key: bool,
}

impl<R: Resource> ResourceAdapter<R> {
    /// Refuses an import the user may not make (403), or of a resource that
    /// is not importable (404): `import`, and `create` (or `update` with an
    /// import key).
    pub(super) async fn admit_import(&self, ctx: &Ctx) -> AppResult<()> {
        if !self.meta.importable {
            return Err(AppError::NotFound);
        }
        let writes = self.policy.allows(ctx, ability::CREATE, None).await
            || (self.meta.import_key.is_some() && self.policy.admits(ctx, ability::UPDATE).await);
        if !self.policy.allows(ctx, ability::IMPORT, None).await || !writes {
            return Err(AppError::Forbidden);
        }
        Ok(())
    }

    /// Fields the user may set by import: writable on create (or update,
    /// with an import key), and the import key itself.
    pub(super) fn import_fields(&self, ctx: &Ctx) -> Vec<ImportField> {
        let natural = matches!(self.meta.key, KeyStrategy::Natural);
        self.fields
            .iter()
            .filter(|f| f.name != self.pk_name || natural)
            // Encrypted fields are never imported (nor exported).
            .filter(|f| f.secret.is_none())
            .filter(|f| {
                f.can_write(Operation::Create, ctx)
                    || (self.meta.import_key.is_some() && f.can_write(Operation::Update, ctx))
            })
            .map(|f| ImportField {
                name: f.name.clone(),
                label: ctx.t_in(R::SLUG, &f.display_label()),
                required: f.required,
                key: self.meta.import_key.as_deref() == Some(f.name.as_str()),
            })
            .collect()
    }

    /// The value of a cell for field `name`, as exports write them: an empty
    /// cell is `null` (for an optional text field that may not be null, an
    /// empty text), a quoted empty cell (`""`) of an optional text field the
    /// empty text (of a required field: `null`, refused); otherwise the text read by the field's type, as query strings
    /// are (`7`, `true`).
    pub(super) fn import_value(&self, name: &str, text: &str, quoted: bool) -> serde_json::Value {
        let Some(column) = self.columns.get(name) else { return serde_json::Value::String(text.to_owned()) };
        let required = self.field(name).is_some_and(|f| f.required);
        // A required field is never set empty, quoted or not (it is refused as missing).
        if text.is_empty() && quoted && column.kind == FieldKind::String && !required {
            return serde_json::Value::String(String::new());
        }
        if text.is_empty() {
            return if column.kind == FieldKind::String && !column.nullable && !required {
                serde_json::Value::String(String::new())
            } else {
                serde_json::Value::Null
            };
        }
        column.kind.from_query(&serde_json::Value::String(text.to_owned()))
    }

    /// Checks a row as the create (or update) path would before running any
    /// hook: field rules, types, required fields on create. No database.
    pub(super) fn check_import_row(&self, ctx: &Ctx, op: Operation, data: Data) -> AppResult<Data> {
        self.accept_input(ctx, op, data)
    }

    /// Keys of the records in the user's scope (active ones) whose import key
    /// equals `value`, at most two (compared by the database).
    pub(super) async fn import_matches(&self, ctx: &Ctx, value: &serde_json::Value) -> AppResult<Vec<String>> {
        let Some(name) = &self.meta.import_key else { return Ok(Vec::new()) };
        let column = self.column(name);
        let Ok(mut candidates) = column.kind.decode(value) else { return Ok(Vec::new()) };
        if candidates.is_empty() || value.is_null() {
            return Ok(Vec::new());
        }
        let models = self
            .base(ctx, Trashed::Without)
            .filter(column.column.eq(candidates.swap_remove(0)))
            .limit(2)
            .all(&ctx.db)
            .await?;
        Ok(models.iter().map(|m| self.key_of(m)).collect())
    }
}
