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

/// The link changes sent with a record's create or update, by relation, and
/// (once locked) the visible targets of each target resource.
#[derive(Default)]
pub(super) struct FormLinks(Vec<FormLink>, HashMap<&'static str, HashSet<String>>);

pub(super) struct FormLink {
    relation: &'static str,
    /// Canonical target keys, sorted.
    attach: Vec<String>,
    detach: Vec<String>,
    /// Pivot columns of new links (validated; empty: database defaults).
    values: Data,
}

/// A link entry for the parent's audit log, recorded after the write.
pub(super) struct LinkAudit {
    event: &'static str,
    relation: &'static str,
    targets: Vec<String>,
    pivot: Option<Map<String, Json>>,
}

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
            LinkOp::Attach | LinkOp::Detach => {
                let (attach, detach) = if op == LinkOp::Attach {
                    (processed.as_slice(), &[][..])
                } else {
                    (&[][..], processed.as_slice())
                };
                let written =
                    Self::write_pivot(&txn, relation, pivot, &parent_value, attach, detach, &values, &linked).await?;
                for entry in &written.1 {
                    self.audit_link_entry(&wctx, &txn, &parent_key, entry).await?;
                }
                change = written.0;
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

    /// Inserts the links to `attach` not in `linked` (with the pivot
    /// `values`) and deletes those to `detach` in `linked`: targets already
    /// checked visible and locked. Returns what changed and its audit entries.
    #[allow(clippy::too_many_arguments)]
    async fn write_pivot(
        txn: &DatabaseTransaction,
        relation: &Relation,
        pivot: &Arc<dyn DynPivot>,
        parent: &Value,
        attach: &[String],
        detach: &[String],
        values: &Data,
        linked: &BTreeMap<String, Data>,
    ) -> AppResult<(RelationChange, Vec<LinkAudit>)> {
        let mut change = RelationChange::new(relation.name);
        let mut entries = Vec::new();
        if !attach.is_empty() {
            // Links already there keep their pivot values.
            for key in attach.iter().filter(|k| !linked.contains_key(*k)) {
                let value = Self::target_value(pivot.as_ref(), key)?;
                pivot.insert(txn, parent.clone(), value, values).await?;
                change.attached.push(key.clone());
            }
            entries.push(LinkAudit {
                event: "attached",
                relation: relation.name,
                targets: change.attached.clone(),
                pivot: Self::pivot_audit(relation, values),
            });
        }
        if !detach.is_empty() {
            change.detached = detach.iter().filter(|k| linked.contains_key(*k)).cloned().collect();
            let removing =
                change.detached.iter().map(|k| Self::target_value(pivot.as_ref(), k)).collect::<AppResult<_>>()?;
            pivot.delete(txn, parent.clone(), removing).await?;
            entries.push(LinkAudit {
                event: "detached",
                relation: relation.name,
                targets: change.detached.clone(),
                pivot: None,
            });
        }
        change.attached.sort();
        change.detached.sort();
        Ok((change, entries))
    }

    async fn audit_link_entry(
        &self,
        ctx: &Ctx,
        txn: &DatabaseTransaction,
        key: &str,
        entry: &LinkAudit,
    ) -> AppResult<()> {
        let meta = LinkMeta { relation: entry.relation, targets: &entry.targets, pivot: entry.pivot.clone() };
        self.audit_link(ctx, txn, entry.event, key, meta).await
    }

    /// The link changes of a record's create or update (`BelongsToManySelect`
    /// inputs, ADR 0035), taken out of `input`: each `{attach?, detach?}` of
    /// target keys. Admission of the abilities they need first (403), then
    /// their shape (422). Detaching on create is refused.
    pub(super) async fn take_links(&self, ctx: &Ctx, op: Operation, input: &mut Data) -> AppResult<FormLinks> {
        let names: Vec<&'static str> = self
            .relations
            .iter()
            .filter(|r| self.form.inputs().iter().any(|i| i.kind == InputType::BelongsToMany && i.name == r.name))
            .map(|r| r.name)
            .collect();
        let mut sent = Vec::new();
        for name in names {
            if let Some(value) = input.remove(name) {
                sent.push((name, value));
            }
        }
        // What each change needs, before its keys are looked at.
        let list = |value: &serde_json::Value, part: &str| {
            value.get(part).and_then(|v| v.as_array()).is_some_and(|items| !items.is_empty())
        };
        for (name, value) in &sent {
            if list(value, "attach") {
                self.admit(ctx, &LinkOp::Attach.ability(name)).await?;
            }
            if list(value, "detach") && op == Operation::Update {
                self.admit(ctx, &LinkOp::Detach.ability(name)).await?;
            }
        }
        let mut errors = ValidationErrors::new();
        let mut out = FormLinks::default();
        for (name, value) in sent {
            let Some(object) = value.as_object().filter(|o| o.keys().all(|k| k == "attach" || k == "detach")) else {
                errors.add(name, "is invalid");
                continue;
            };
            let (relation, _) = self.many(name)?;
            let target = Self::many_target(ctx, relation)?;
            let mut parts: [BTreeSet<String>; 2] = Default::default();
            let mut valid = true;
            for (index, part) in ["attach", "detach"].into_iter().enumerate() {
                let Some(items) = object.get(part) else { continue };
                let Some(items) = items.as_array() else {
                    valid = false;
                    continue;
                };
                if items.len() > MAX_LINK_KEYS {
                    errors.add(name, format!("may not have more than {MAX_LINK_KEYS} items"));
                    valid = false;
                    continue;
                }
                for item in items {
                    match item.as_str().and_then(|key| target.canonical_key(key)) {
                        Some(key) => {
                            parts[index].insert(key);
                        }
                        None => valid = false,
                    }
                }
            }
            let [attach, detach] = parts;
            if !valid || attach.intersection(&detach).next().is_some() {
                errors.add(name, "is invalid");
                continue;
            }
            if op == Operation::Create && !detach.is_empty() {
                errors.add(name, "is not allowed");
                continue;
            }
            // New links take the pivot columns' database defaults.
            let values = if attach.is_empty() {
                Data::new()
            } else {
                self.pivot_input(ctx, relation, Operation::Create, Data::new())?
            };
            if !attach.is_empty() || !detach.is_empty() {
                out.0.push(FormLink {
                    relation: name,
                    attach: attach.into_iter().collect(),
                    detach: detach.into_iter().collect(),
                    values,
                });
            }
        }
        // The targets of one resource are locked in one lookup, which takes at
        // most `Limits::keys` keys: relations sharing a target share that bound.
        let most = crate::resource::Limits::default().keys;
        let mut per_target: HashMap<&'static str, BTreeSet<&String>> = HashMap::new();
        for link in &out.0 {
            let (relation, _) = self.many(link.relation)?;
            let keys = per_target.entry(relation.target).or_default();
            keys.extend(link.attach.iter().chain(&link.detach));
            if keys.len() > most {
                errors.add(link.relation, format!("may not have more than {most} items"));
            }
        }
        errors.into_result()?;
        Ok(out)
    }

    /// Locks the targets of the link changes of a create or update, before
    /// the record is written (its foreign keys take shared locks on what they
    /// reference: locking the same rows exclusively afterwards would upgrade
    /// those locks and could deadlock with another such write). Every target
    /// of every relation sent is locked once per target resource through
    /// `lock_visible` (keys in key order), resources in the registry's lock
    /// order (a resource before the targets of its own relations), so two
    /// relations to one resource never lock its rows in different orders and
    /// other writers never lock them the other way round. The records the
    /// row's own `belongs_to` keys in `data` reference are locked in the same
    /// pass when they are rows of a target resource (keys in key order):
    /// otherwise the row's write would take their shared locks after the
    /// targets' exclusive ones, and two such writes crossing (a post's primary
    /// tag one way, its tags the other) could wait on each other.
    pub(super) async fn lock_links(
        &self,
        ctx: &Ctx,
        txn: &DatabaseTransaction,
        links: FormLinks,
        data: &Data,
    ) -> AppResult<FormLinks> {
        let mut links = links;
        if links.0.is_empty() {
            return Ok(links);
        }
        let mut wanted: BTreeMap<&'static str, (BTreeSet<String>, BTreeSet<String>)> = BTreeMap::new();
        for link in &links.0 {
            let (relation, _) = self.many(link.relation)?;
            wanted.entry(relation.target).or_default().0.extend(link.attach.iter().chain(&link.detach).cloned());
        }
        for field in &self.fields {
            let (Some(slug), Some(value)) = (field.belongs_to, data.get(&field.name)) else { continue };
            // Only into resources the links lock: other references keep
            // their shared locks (locking them here would invert the order
            // of writers that lock them first, e.g. a delete cascading here).
            let Some((_, referenced)) = wanted.get_mut(slug) else { continue };
            let key = match value {
                Json::String(key) => key.clone(),
                Json::Number(key) => key.to_string(),
                _ => continue,
            };
            let target =
                ctx.registry.get(slug).ok_or_else(|| AppError::internal(format!("unknown resource `{slug}`")))?;
            if let Some(key) = target.canonical_key(&key) {
                referenced.insert(key);
            }
        }
        let mut wanted: Vec<_> = wanted.into_iter().collect();
        wanted.sort_by_cached_key(|(slug, _)| ctx.registry.lock_rank(slug));
        for (slug, (keys, referenced)) in wanted {
            let target =
                ctx.registry.get(slug).ok_or_else(|| AppError::internal(format!("unknown resource `{slug}`")))?;
            let keys: Vec<String> = keys.into_iter().collect();
            let referenced: Vec<String> = referenced.into_iter().collect();
            links.1.insert(slug, target.lock_visible_with(ctx, txn, &keys, &referenced).await?);
        }
        Ok(links)
    }

    /// Writes the link changes of a create or update to `model` (the record
    /// as stored, locked by the write; targets locked by [`Self::lock_links`]),
    /// in its transaction: the abilities on the record, then the targets the
    /// user may view (attaching one that is not is a 422; detaching one is
    /// ignored, as in the relation manager), the pivot rows and the hook.
    /// Audit entries are returned, for after the record's own.
    pub(super) async fn write_links(
        &self,
        ctx: &Ctx,
        txn: &DatabaseTransaction,
        model: &Model<R>,
        links: FormLinks,
    ) -> AppResult<Vec<LinkAudit>> {
        let mut audits = Vec::new();
        if links.0.is_empty() {
            return Ok(audits);
        }
        let record = self.record(model);
        for link in &links.0 {
            for (keys, op) in [(&link.attach, LinkOp::Attach), (&link.detach, LinkOp::Detach)] {
                if !keys.is_empty() && !self.policy.allows(ctx, &op.ability(link.relation), Some(&record)).await {
                    return Err(AppError::Forbidden);
                }
            }
        }
        let locked = links.1;
        let parent = model.get(self.pk);
        for link in links.0 {
            let (relation, pivot) = self.many(link.relation)?;
            let visible = locked
                .get(relation.target)
                .ok_or_else(|| AppError::internal("form links written before their targets were locked"))?;
            let keys: Vec<String> =
                link.attach.iter().chain(&link.detach).cloned().collect::<BTreeSet<_>>().into_iter().collect();
            if link.attach.iter().any(|k| !visible.contains(k)) {
                return Err(ValidationErrors::field(link.relation, "is not one of the options").into());
            }
            let detach: Vec<String> = link.detach.iter().filter(|k| visible.contains(*k)).cloned().collect();
            let values: Vec<Value> = keys
                .iter()
                .filter(|k| visible.contains(*k))
                .map(|k| Self::target_value(pivot.as_ref(), k))
                .collect::<AppResult<_>>()?;
            let linked = pivot.rows(DbConn::Txn(txn), parent.clone(), values).await?;
            let (change, entries) =
                Self::write_pivot(txn, relation, pivot, &parent, &link.attach, &detach, &link.values, &linked).await?;
            audits.extend(entries);
            if !change.is_empty() {
                R::after_relation_change(ctx, txn, model, &change).await?;
            }
        }
        Ok(audits)
    }

    /// Records the link entries of a create or update, after the record's own.
    pub(super) async fn audit_links(
        &self,
        ctx: &Ctx,
        txn: &DatabaseTransaction,
        model: &Model<R>,
        audits: Vec<LinkAudit>,
    ) -> AppResult<()> {
        let key = self.key_of(model);
        for entry in &audits {
            self.audit_link_entry(ctx, txn, &key, entry).await?;
        }
        Ok(())
    }

    /// Targets offered for attaching in a create form (no record yet): the
    /// target's lookup, when the user may create records and attach.
    pub(super) async fn link_options_new(
        &self,
        ctx: &Ctx,
        name: &str,
        search: Option<&str>,
        keys: &[String],
    ) -> AppResult<Vec<LinkOption>> {
        let (relation, _) = self.many(name)?;
        // Only for a relation the create form links (deny by default).
        if !self.form.inputs().iter().any(|i| i.kind == InputType::BelongsToMany && i.name == name) {
            return Err(AppError::NotFound);
        }
        let ability = LinkOp::Attach.ability(name);
        self.admit(ctx, &ability).await?;
        if !self.policy.allows(ctx, ability::CREATE, None).await || !self.policy.allows(ctx, &ability, None).await {
            return Err(AppError::Forbidden);
        }
        let target = Self::many_target(ctx, relation)?;
        let found = target.lookup(ctx, search, keys).await?;
        Ok(found.into_iter().map(|(value, label)| LinkOption { value, label, attached: false }).collect())
    }

    /// The records of `keys` the user may view, locked in key order and
    /// checked again under the lock: only these may be linked or unlinked.
    /// The records of `also` (referenced by the writing row's own foreign
    /// keys, already checked) are locked in the same pass, whoever may view
    /// them; records of `keys` the user may not view are never locked.
    pub(super) async fn locked_visible(
        &self,
        ctx: &Ctx,
        txn: &DatabaseTransaction,
        keys: &[String],
        also: &[String],
    ) -> AppResult<HashSet<String>> {
        let mut candidates = Vec::new();
        if self.policy.allows(ctx, ability::VIEW_ANY, None).await {
            for (model, record) in self.models_in_scope(ctx, txn, keys).await? {
                if self.policy.allows(ctx, ability::VIEW, Some(&record)).await {
                    candidates.push(self.key_of(&model));
                }
            }
        }
        let wanted: Vec<String> = candidates.iter().chain(also).cloned().collect::<BTreeSet<_>>().into_iter().collect();
        if wanted.is_empty() {
            return Ok(HashSet::new());
        }
        // Trashed rows too (a reference may still name one): the check below
        // leaves out what is trashed, gone or hidden now.
        let locked = self.lock_rows(txn, &wanted, Trashed::With).await?;
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
