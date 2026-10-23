//! Belongs-to-many relations (ADR 0020): lists through the pivot, attach,
//! detach and pivot updates. One lock order for every writer: the parent
//! (after its own `lock_parent`), then the targets in key order, then pivot
//! rows by both keys. Every pivot write of a parent holds that parent's lock,
//! so links are read then inserted without upserts.

use serde::Serialize;
use serde_json::{Map, Value as Json};

use rustcms_core::audit::{self, AuditEntry};

use super::*;
use crate::links::{LinkOp, LinkOption, LinkOutcome, LinkRequest, RelationChange};
use crate::resource::Through;

/// Most targets one attach or detach names.
pub(crate) const MAX_LINK_KEYS: usize = 100;

/// What a link write changed, on the parent's audit entry.
#[derive(Serialize)]
struct LinkMeta<'a> {
    relation: &'a str,
    targets: &'a [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    pivot: Option<Map<String, Json>>,
}

impl<R: Resource> ResourceAdapter<R> {
    /// The belongs-to-many relation `name` (404 for any other).
    pub(super) fn many(&self, name: &str) -> AppResult<(&Relation, &Arc<dyn DynPivot>)> {
        let relation = self.relations.iter().find(|r| r.name == name).ok_or(AppError::NotFound)?;
        let pivot = relation.pivot.as_ref().ok_or(AppError::NotFound)?;
        Ok((relation, pivot))
    }

    /// The target resource of `relation`.
    fn many_target(ctx: &Ctx, relation: &Relation) -> AppResult<Arc<dyn DynResource>> {
        ctx.registry
            .get(relation.target)
            .cloned()
            .ok_or_else(|| AppError::internal(format!("unknown resource `{}`", relation.target)))
    }

    /// The pivot value of a target key (canonical text).
    fn target_value(pivot: &dyn DynPivot, key: &str) -> AppResult<Value> {
        let kind = pivot.column(pivot.target_column()).map(|c| c.kind).ok_or_else(|| AppError::internal("pivot"))?;
        kind.decode_key(key).map_err(|_| AppError::NotFound)
    }

    /// Refuses a link write the policy does not admit at all, before anything
    /// is read (403 whatever the body, the record or the targets).
    pub(super) async fn admit_many(&self, ctx: &Ctx, name: &str, op: LinkOp) -> AppResult<()> {
        self.many(name)?;
        self.admit(ctx, &op.ability(name)).await
    }

    /// The canonical key of the visible record `key` and the condition
    /// selecting its targets.
    pub(super) async fn many_through(&self, ctx: &Ctx, name: &str, key: &str) -> AppResult<(String, Through)> {
        let (_, pivot) = self.many(name)?;
        let key = DynResource::visible_key(self, ctx, key).await?;
        let value = self.pk_kind.decode_key(&key).map_err(|_| AppError::NotFound)?;
        Ok((key, Through(pivot.targets_of(value))))
    }

    /// The pivot columns the user may read, of the links of the record `key`
    /// (canonical, already checked visible) to `targets`.
    pub(super) async fn many_values(
        &self,
        ctx: &Ctx,
        name: &str,
        key: &str,
        targets: &[String],
    ) -> AppResult<BTreeMap<String, Data>> {
        let (relation, pivot) = self.many(name)?;
        let readable: Vec<&Field> = relation.pivot_fields.iter().filter(|f| f.can_read(ctx)).collect();
        if readable.is_empty() || targets.is_empty() {
            return Ok(BTreeMap::new());
        }
        let parent = self.pk_kind.decode_key(key).map_err(|_| AppError::NotFound)?;
        let values = targets.iter().filter_map(|k| Self::target_value(pivot.as_ref(), k).ok()).collect();
        let rows = pivot.rows(DbConn::Pool(&ctx.db), parent, values).await?;
        Ok(rows
            .into_iter()
            .map(|(target, row)| {
                let data = readable.iter().filter_map(|f| Some((f.name.clone(), row.get(&f.name)?.clone()))).collect();
                (target, data)
            })
            .collect())
    }

    /// Validates pivot columns sent by the client for `op` (types, rules, `required` on attach).
    fn pivot_input(&self, ctx: &Ctx, relation: &Relation, op: Operation, input: Data) -> AppResult<Data> {
        let pivot = relation.pivot.as_ref().ok_or(AppError::NotFound)?;
        let mut errors = ValidationErrors::new();
        for (name, value) in &input {
            let field = relation.pivot_fields.iter().find(|f| f.name == *name).filter(|f| f.can_write(op, ctx));
            let (Some(field), Some(column)) = (field, pivot.column(name)) else {
                errors.add(name.as_str(), "is not allowed");
                continue;
            };
            let rule = InputRule {
                kind: column.kind,
                nullable: column.nullable,
                required: field.required,
                max_length: field.max_length,
                key_value: None,
                rich_text: false,
                options: None,
            };
            check_input(name, &rule, value, &mut errors);
        }
        if op == Operation::Create {
            for field in relation.pivot_fields.iter().filter(|f| f.required) {
                // A mandatory column the user may not write: no link they could make.
                let mandatory = pivot.column(&field.name).is_some_and(|c| !c.nullable && !c.has_default);
                if !field.can_write(op, ctx) {
                    if mandatory {
                        return Err(AppError::Forbidden);
                    }
                    continue;
                }
                if !input.contains_key(&field.name) {
                    errors.add(field.name.as_str(), "is required");
                }
            }
        }
        errors.into_result()?;
        Ok(input)
    }

    /// Target keys from the client: 1 to [`MAX_LINK_KEYS`], each a valid key
    /// of the target (canonical, deduplicated, in order).
    fn link_keys(target: &dyn DynResource, keys: &[String]) -> AppResult<Vec<String>> {
        if keys.is_empty() {
            return Err(ValidationErrors::field("keys", "is required").into());
        }
        if keys.len() > MAX_LINK_KEYS {
            return Err(ValidationErrors::field("keys", format!("may not have more than {MAX_LINK_KEYS} items")).into());
        }
        let mut out = BTreeSet::new();
        for key in keys {
            let Some(canonical) = target.canonical_key(key) else {
                return Err(ValidationErrors::field("keys", "is invalid").into());
            };
            out.insert(canonical);
        }
        Ok(out.into_iter().collect())
    }

    /// Pivot values for the audit log: declared columns only, redacted as
    /// resource fields are (a `readable` rule, guarded, `audit_redact`).
    fn pivot_audit(relation: &Relation, values: &Data) -> Option<Map<String, Json>> {
        let out: Map<String, Json> = values
            .iter()
            .filter_map(|(name, value)| {
                let field = relation.pivot_fields.iter().find(|f| f.name == *name)?;
                let redacted = field.audit_redact || field.readable.is_some() || field.guarded;
                Some((name.clone(), if redacted { Json::String(audit::REDACTED.to_owned()) } else { value.clone() }))
            })
            .collect();
        (!out.is_empty()).then_some(out)
    }

    async fn audit_link(
        &self,
        ctx: &Ctx,
        txn: &DatabaseTransaction,
        event: &str,
        key: &str,
        meta: LinkMeta<'_>,
    ) -> AppResult<()> {
        if !self.meta.audit || meta.targets.is_empty() {
            return Ok(());
        }
        audit::record(txn, &ctx.audit_context, AuditEntry::new(event, R::SLUG).key(key).meta(meta)).await
    }

    /// Runs a link write on the record `key` (admission is checked first, see
    /// [`Self::admit_many`]).
    pub(super) async fn link(&self, ctx: &Ctx, name: &str, key: &str, request: LinkRequest) -> AppResult<LinkOutcome> {
        let (relation, pivot) = self.many(name)?;
        let op = match &request {
            LinkRequest::Attach { .. } => LinkOp::Attach,
            LinkRequest::Detach { .. } => LinkOp::Detach,
            LinkRequest::UpdatePivot { .. } => LinkOp::UpdatePivot,
        };
        let ability = op.ability(name);
        self.admit(ctx, &ability).await?;
        let target = Self::many_target(ctx, relation)?;
        // Everything checkable without the database, before any lock.
        let (keys, values) = match request {
            LinkRequest::Attach { keys, values } => {
                (Self::link_keys(target.as_ref(), &keys)?, self.pivot_input(ctx, relation, Operation::Create, values)?)
            }
            LinkRequest::Detach { keys } => (Self::link_keys(target.as_ref(), &keys)?, Data::new()),
            LinkRequest::UpdatePivot { key: target_key, values } => {
                let canonical = target.canonical_key(&target_key).ok_or(AppError::NotFound)?;
                (vec![canonical], self.pivot_input(ctx, relation, Operation::Update, values)?)
            }
        };
        let (txn, wctx) = self.begin(ctx).await?;
        // The parent: in scope, allowed, then locked and checked again.
        let parent = self.find_authorized_locked(&wctx, &txn, key, &ability, Trashed::Without).await?;
        let parent_key = self.key_of(&parent);
        let parent_value = parent.get(self.pk);
        // The targets the user may view, locked in key order and checked again.
        let locked = target.lock_visible(&wctx, &txn, &keys).await?;
        let mut processed: Vec<String> = keys.iter().filter(|k| locked.contains(*k)).cloned().collect();
        let missing: Vec<String> = keys.iter().filter(|k| !locked.contains(*k)).cloned().collect();
        let target_values: Vec<Value> =
            processed.iter().map(|k| Self::target_value(pivot.as_ref(), k)).collect::<AppResult<_>>()?;
        let linked = pivot.rows(DbConn::Txn(&txn), parent_value.clone(), target_values).await?;
        let mut change = RelationChange::new(relation.name);
        match op {
            LinkOp::Attach => {
                // Links already there keep their pivot values.
                let mut added = Vec::new();
                for key in processed.iter().filter(|k| !linked.contains_key(*k)) {
                    let value = Self::target_value(pivot.as_ref(), key)?;
                    pivot.insert(&txn, parent_value.clone(), value, &values).await?;
                    added.push(key.clone());
                }
                let meta = LinkMeta { relation: name, targets: &added, pivot: Self::pivot_audit(relation, &values) };
                self.audit_link(&wctx, &txn, "attached", &parent_key, meta).await?;
                change.attached = added;
            }
            LinkOp::Detach => {
                let removed: Vec<String> = processed.iter().filter(|k| linked.contains_key(*k)).cloned().collect();
                let removing =
                    removed.iter().map(|k| Self::target_value(pivot.as_ref(), k)).collect::<AppResult<_>>()?;
                pivot.delete(&txn, parent_value.clone(), removing).await?;
                let meta = LinkMeta { relation: name, targets: &removed, pivot: None };
                self.audit_link(&wctx, &txn, "detached", &parent_key, meta).await?;
                change.detached = removed;
            }
            LinkOp::UpdatePivot => {
                // A target not visible, or not linked: as missing.
                let Some(target_key) = processed.first().filter(|k| linked.contains_key(*k)).cloned() else {
                    Self::abort(txn, wctx).await?;
                    return Err(AppError::NotFound);
                };
                // Only what differs from the stored link (canonical values): nothing to
                // change, nothing written or recorded.
                let stored = &linked[&target_key];
                let changed: Data = values
                    .iter()
                    .filter(|(name, value)| {
                        let canonical = pivot
                            .column(name)
                            .and_then(|c| c.kind.decode(value).ok())
                            .and_then(|v| v.first().map(encode))
                            .unwrap_or_else(|| (*value).clone());
                        stored.get(*name) != Some(&canonical)
                    })
                    .map(|(name, value)| (name.clone(), value.clone()))
                    .collect();
                if !changed.is_empty() {
                    let value = Self::target_value(pivot.as_ref(), &target_key)?;
                    pivot.update(&txn, parent_value.clone(), value, &changed).await?;
                    let targets = [target_key];
                    let meta =
                        LinkMeta { relation: name, targets: &targets, pivot: Self::pivot_audit(relation, &changed) };
                    self.audit_link(&wctx, &txn, "pivot_updated", &parent_key, meta).await?;
                    change.pivot_updated = targets.into();
                }
            }
        }
        if !change.is_empty() {
            R::after_relation_change(&wctx, &txn, &parent, &change).await?;
        }
        Self::commit(txn, wctx).await?;
        processed.sort();
        Ok(LinkOutcome { processed, missing })
    }

    /// Targets offered for attaching to the record `key`: the target's lookup
    /// (scope, view policy, title), each marked when already linked. The
    /// record must allow attaching (as the write would).
    pub(super) async fn link_options(
        &self,
        ctx: &Ctx,
        name: &str,
        key: &str,
        search: Option<&str>,
        keys: &[String],
    ) -> AppResult<Vec<LinkOption>> {
        let (relation, pivot) = self.many(name)?;
        let ability = LinkOp::Attach.ability(name);
        self.admit(ctx, &ability).await?;
        let parent = self.find_model(ctx, &ctx.db, key).await?;
        if !self.policy.allows(ctx, &ability, Some(&self.record(&parent))).await {
            return Err(AppError::Forbidden);
        }
        let target = Self::many_target(ctx, relation)?;
        let found = target.lookup(ctx, search, keys).await?;
        let values = found.iter().filter_map(|(k, _)| Self::target_value(pivot.as_ref(), k).ok()).collect();
        let linked = pivot.rows(DbConn::Pool(&ctx.db), parent.get(self.pk), values).await?;
        Ok(found
            .into_iter()
            .map(|(value, label)| LinkOption { attached: linked.contains_key(&value), value, label })
            .collect())
    }

    /// The records of `keys` the user may view, locked in key order and
    /// checked again under the lock: only these may be linked or unlinked.
    pub(super) async fn locked_visible(
        &self,
        ctx: &Ctx,
        txn: &DatabaseTransaction,
        keys: &[String],
    ) -> AppResult<HashSet<String>> {
        if !self.policy.allows(ctx, ability::VIEW_ANY, None).await {
            return Ok(HashSet::new());
        }
        let mut candidates = Vec::new();
        for (model, record) in self.models_in_scope(ctx, txn, keys).await? {
            if self.policy.allows(ctx, ability::VIEW, Some(&record)).await {
                candidates.push(self.key_of(&model));
            }
        }
        let locked = self.lock_rows(txn, &candidates, Trashed::Without).await?;
        candidates.retain(|k| locked.contains(k));
        let mut out = HashSet::new();
        for (model, record) in self.models_in_scope(ctx, txn, &candidates).await? {
            if self.policy.allows(ctx, ability::VIEW, Some(&record)).await {
                out.insert(self.key_of(&model));
            }
        }
        Ok(out)
    }
}
