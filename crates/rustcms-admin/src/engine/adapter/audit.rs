//! Audit entries of the engine's writes (ADR 0014): recorded in the write's
//! transaction, from the declared fields, redacted by their declaration.

use serde::Serialize;
use serde_json::{Map, Value as Json, json};

use rustcms_core::audit::{self, AuditEntry};

use super::*;

/// Where a record created through a relation belongs.
#[derive(Serialize)]
struct RelationMeta<'a> {
    relation: &'a str,
    parent: &'a str,
}

/// What an action did to a record beyond its fields.
#[derive(Serialize)]
struct ActionMeta {
    result: &'static str,
}

impl<R: Resource> ResourceAdapter<R> {
    /// The declared fields of a record as stored (wire values), before
    /// redaction; rich text documents as their size and digest.
    pub(super) fn audit_snapshot(&self, model: &Model<R>) -> Map<String, Json> {
        let record = self.record(model);
        self.fields
            .iter()
            .filter_map(|field| {
                let value = record.get(&field.name)?.clone();
                let value = if field.secret.is_some() {
                    // Compared to find changes, then redacted: never stored.
                    self.secret_digest(&field.name, model)
                } else if field.rich_text && !value.is_null() {
                    let text = value.to_string();
                    json!({ "document": true, "bytes": text.len(), "sha256": audit::summary(&text)["sha256"] })
                } else {
                    value
                };
                Some((field.name.clone(), value))
            })
            .collect()
    }

    /// Whether a field's values never reach the audit log: decided by the
    /// declaration only (never by who is asking).
    pub(super) fn audit_redacted(&self, name: &str) -> bool {
        let Some(field) = self.field(name) else { return true };
        let password = self.form.inputs().iter().any(|i| i.name == name && i.mode == Some("password"));
        field.audit_redact || field.readable.is_some() || field.guarded || password
    }

    /// The record as stored now, by key (whatever the scope or the trash).
    pub(super) async fn reload(&self, txn: &DatabaseTransaction, model: &Model<R>) -> AppResult<Option<Model<R>>> {
        Ok(Entity::<R>::find().filter(self.pk.eq(model.get(self.pk))).one(txn).await?)
    }

    /// Whether a record is in the trash.
    pub(super) fn trashed(&self, model: &Model<R>) -> bool {
        self.soft
            .as_ref()
            .is_some_and(|(column, _, _)| !encode_column(column.def().get_column_type(), &model.get(*column)).is_null())
    }

    /// Records a write's outcome from the record as stored once its hooks ran:
    /// `event` with the changes from `before` (always when `always`, else only
    /// if something changed), then `deleted` if the hooks removed the record,
    /// or `trashed` if they moved it to the trash.
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn audit_outcome(
        &self,
        ctx: &Ctx,
        txn: &DatabaseTransaction,
        event: &str,
        before: Option<&Map<String, Json>>,
        written: &Model<R>,
        was_trashed: bool,
        always: bool,
    ) -> AppResult<()> {
        let key = self.key_of(written);
        let stored = self.reload(txn, written).await?;
        let after = self.audit_snapshot(stored.as_ref().unwrap_or(written));
        if always || before != Some(&after) {
            self.audit_write(ctx, txn, event, &key, before, Some(&after)).await?;
        }
        match stored {
            None => self.audit_write(ctx, txn, "deleted", &key, None, None).await,
            Some(model) if self.trashed(&model) && !was_trashed => {
                self.audit_write(ctx, txn, "trashed", &key, None, None).await
            }
            Some(_) => Ok(()),
        }
    }

    /// Records `event` on the record `key`, with the changes from `before` to `after`.
    pub(super) async fn audit_write(
        &self,
        ctx: &Ctx,
        txn: &DatabaseTransaction,
        event: &str,
        key: &str,
        before: Option<&Map<String, Json>>,
        after: Option<&Map<String, Json>>,
    ) -> AppResult<()> {
        if !self.meta.audit {
            return Ok(());
        }
        audit::record(txn, &ctx.audit_context, self.audit_entry(ctx, event, key, before, after)).await
    }

    fn audit_entry(
        &self,
        ctx: &Ctx,
        event: &str,
        key: &str,
        before: Option<&Map<String, Json>>,
        after: Option<&Map<String, Json>>,
    ) -> AuditEntry {
        let changes = audit::diff(before, after, &|field| self.audit_redacted(field));
        let mut entry = AuditEntry::new(event, R::SLUG).key(key);
        if changes.as_object().is_some_and(|c| !c.is_empty()) {
            entry = entry.changes(changes);
        }
        if let Some((relation, field, parent)) = &ctx.bound {
            // The parent key is the record's foreign key: redacted like it.
            let parent = if self.audit_redacted(field) { audit::REDACTED } else { parent.as_str() };
            entry = entry.meta(RelationMeta { relation, parent });
        }
        entry
    }

    /// Records what an action did to each record it processed: the changes of
    /// its declared fields, or that it trashed or deleted it. One read of the
    /// records after the action, one batch of entries.
    pub(super) async fn audit_action(
        &self,
        ctx: &Ctx,
        txn: &DatabaseTransaction,
        name: &str,
        before: Vec<(String, Map<String, Json>, bool)>,
    ) -> AppResult<()> {
        if !self.meta.audit || before.is_empty() {
            return Ok(());
        }
        let keys = self.decode_keys(&before.iter().map(|(key, _, _)| key.clone()).collect::<Vec<_>>())?;
        let after: HashMap<String, Model<R>> = Entity::<R>::find()
            .filter(self.pk.is_in(keys))
            .all(txn)
            .await?
            .into_iter()
            .map(|m| (self.key_of(&m), m))
            .collect();
        let event = format!("action:{name}");
        let entries = before
            .into_iter()
            .map(|(key, snapshot, was_trashed)| match after.get(&key) {
                // Gone: its former values are not kept, as for a delete.
                None => self.audit_entry(ctx, &event, &key, None, None).meta(ActionMeta { result: "deleted" }),
                Some(model) => {
                    let entry = self.audit_entry(ctx, &event, &key, Some(&snapshot), Some(&self.audit_snapshot(model)));
                    if self.trashed(model) && !was_trashed {
                        entry.meta(ActionMeta { result: "trashed" })
                    } else {
                        entry
                    }
                }
            })
            .collect();
        audit::record_all(txn, &ctx.audit_context, entries).await
    }
}
