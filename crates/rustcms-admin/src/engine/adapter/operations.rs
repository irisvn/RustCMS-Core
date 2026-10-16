//! The operations every resource offers (`DynResource`): the entry points of
//! the HTTP handlers and of other resources.

use super::*;

#[async_trait]
impl<R: Resource> DynResource for ResourceAdapter<R> {
    fn slug(&self) -> &'static str {
        R::SLUG
    }

    fn meta(&self) -> &ResourceMeta {
        &self.meta
    }

    fn info(&self) -> ResourceInfo {
        ResourceInfo::clone(&ResourceAdapter::info(self))
    }

    fn display_strings(&self) -> Vec<(bool, String)> {
        ResourceAdapter::display_strings(self)
    }

    async fn allows(&self, ctx: &Ctx, ability: &str) -> bool {
        self.policy.allows(ctx, ability, None).await
    }

    async fn admits(&self, ctx: &Ctx, ability: &str) -> bool {
        self.policy.admits(ctx, ability).await
    }

    fn relations(&self) -> &[Relation] {
        &self.relations
    }

    fn field_link(&self, name: &str) -> Option<(Option<&'static str>, &'static str)> {
        self.field(name).map(|f| (f.belongs_to, self.column(name).kind.name()))
    }

    fn key_kind(&self) -> &'static str {
        self.pk_kind.name()
    }

    fn key_name(&self) -> &str {
        &self.pk_name
    }

    fn canonical_key(&self, key: &str) -> Option<String> {
        self.pk_kind.decode_key(key).ok().map(|value| encode_key(&value))
    }

    fn field_access(&self, ctx: &Ctx, name: &str) -> (bool, bool) {
        self.field(name).map_or((false, false), |f| (f.can_read(ctx), f.can_write(Operation::Create, ctx)))
    }

    async fn visible_key(&self, ctx: &Ctx, key: &str) -> AppResult<String> {
        match self.find(ctx, key).await {
            Ok(envelope) => Ok(envelope.key),
            // Denied or hidden: indistinguishable from missing.
            Err(AppError::Forbidden) => Err(AppError::NotFound),
            Err(err) => Err(err),
        }
    }

    async fn replay_create(&self, ctx: &Ctx, input: &Data) -> AppResult<Option<Envelope>> {
        let Some(claim) = self.create_claim(ctx, input) else { return Ok(None) };
        let Some(receipt) = crate::idempotency::existing(&ctx.db, &claim).await? else { return Ok(None) };
        let key = receipt["key"].as_str().ok_or_else(|| AppError::internal("create receipt without a key"))?;
        Ok(Some(self.committed_key_envelope(ctx, key.to_owned()).await))
    }

    fn permissions(&self) -> Vec<String> {
        let mut abilities: Vec<&str> =
            vec![ability::VIEW_ANY, ability::VIEW, ability::CREATE, ability::UPDATE, ability::DELETE];
        if self.soft.is_some() {
            abilities.extend([ability::RESTORE, ability::FORCE_DELETE]);
        }
        if self.meta.exportable {
            abilities.push(ability::EXPORT);
        }
        if self.meta.importable {
            abilities.push(ability::IMPORT);
        }
        abilities.extend(self.actions.iter().map(|a| a.ability));
        let mut permissions: Vec<String> = abilities.into_iter().map(|a| format!("{}.{a}", R::SLUG)).collect();
        // Writes to belongs-to-many relations.
        for relation in self.relations.iter().filter(|r| r.is_many()) {
            use crate::links::LinkOp;
            let ops: &[LinkOp] = if relation.pivot_fields.is_empty() {
                &[LinkOp::Attach, LinkOp::Detach]
            } else {
                &[LinkOp::Attach, LinkOp::Detach, LinkOp::UpdatePivot]
            };
            permissions.extend(ops.iter().map(|op| format!("{}.{}", R::SLUG, op.ability(relation.name))));
        }
        // Explicit admission permissions are checked verbatim: declare them too.
        permissions.extend(self.actions.iter().filter_map(|a| a.required_permission).map(str::to_owned));
        permissions.sort_unstable();
        permissions.dedup();
        permissions
    }

    fn permission_uses(&self) -> Vec<(String, Option<String>)> {
        let mut abilities: Vec<String> =
            [ability::VIEW_ANY, ability::VIEW, ability::CREATE, ability::UPDATE, ability::DELETE]
                .map(str::to_owned)
                .into();
        if self.soft.is_some() {
            abilities.extend([ability::RESTORE, ability::FORCE_DELETE].map(str::to_owned));
        }
        if self.meta.exportable {
            abilities.push(ability::EXPORT.to_owned());
        }
        if self.meta.importable {
            abilities.push(ability::IMPORT.to_owned());
        }
        abilities.extend(self.actions.iter().map(|a| a.ability.to_owned()));
        for relation in self.relations.iter().filter(|r| r.is_many()) {
            use crate::links::LinkOp;
            abilities.extend([LinkOp::Attach, LinkOp::Detach].map(|op| op.ability(relation.name)));
            if !relation.pivot_fields.is_empty() {
                abilities.push(LinkOp::UpdatePivot.ability(relation.name));
            }
        }
        abilities.sort_unstable();
        abilities.dedup();
        let mut uses: Vec<(String, Option<String>)> =
            abilities.into_iter().map(|a| (format!("{}.{a}", R::SLUG), Some(a))).collect();
        uses.extend(self.actions.iter().filter_map(|a| a.required_permission).map(|p| (p.to_owned(), None)));
        uses
    }

    async fn schema(&self, ctx: &Ctx, op: SchemaOperation, key: Option<&str>) -> AppResult<ResourceSchema> {
        let mut abilities = BTreeMap::new();
        abilities.insert("viewAny", self.policy.allows(ctx, ability::VIEW_ANY, None).await);
        abilities.insert("create", self.policy.allows(ctx, ability::CREATE, None).await);
        // The record history (`audit.view`; each record is checked when it is opened).
        abilities.insert("history", self.meta.audit && ctx.can(crate::audit::VIEW));
        if self.meta.exportable {
            abilities.insert("export", ResourceAdapter::admit_export(self, ctx).await.is_ok());
        }
        if self.meta.importable {
            abilities.insert("import", ResourceAdapter::admit_import(self, ctx).await.is_ok());
        }
        if self.soft.is_some() {
            // Whether the trash can be listed; each record still decides.
            abilities.insert("restore", self.policy.admits(ctx, ability::RESTORE).await);
            abilities.insert("forceDelete", self.policy.admits(ctx, ability::FORCE_DELETE).await);
        }
        let mut viewed = None;
        if let Some(key) = key {
            let admission = if op == SchemaOperation::Edit { ability::UPDATE } else { ability::VIEW };
            self.admit(ctx, admission).await?;
            let record = Self::record(&self.find_model(ctx, &ctx.db, key).await?);
            for name in [ability::VIEW, ability::UPDATE, ability::DELETE] {
                abilities.insert(name, self.policy.allows(ctx, name, Some(&record)).await);
            }
            viewed = Some(record);
        }
        let allowed = match (op, key) {
            (SchemaOperation::Create, _) => abilities["create"],
            (SchemaOperation::Edit, Some(_)) => abilities[ability::UPDATE],
            (SchemaOperation::View, Some(_)) => abilities[ability::VIEW],
            (_, None) => abilities["viewAny"],
        };
        if !allowed {
            return Err(AppError::Forbidden);
        }
        Ok(ResourceSchema {
            schema_version: SCHEMA_VERSION,
            vocabulary_version: VOCABULARY_VERSION,
            resource: ResourceAdapter::info(self).translated(ctx),
            operation: op,
            abilities,
            form: FormSchema {
                columns: self.form.columns,
                components: self.components(ctx, &self.form.components, op),
            },
            table: self.table_schema(ctx).await,
            relations: match (&viewed, op) {
                (Some(record), SchemaOperation::View) => self.relation_schemas(ctx, record).await,
                _ => Vec::new(),
            },
        })
    }

    async fn lookup(&self, ctx: &Ctx, search: Option<&str>, keys: &[String]) -> AppResult<Vec<(String, String)>> {
        let Some(title) = &self.meta.title_field else { return Ok(Vec::new()) };
        if !self.field(title).is_some_and(|f| f.can_read(ctx))
            || !self.policy.allows(ctx, ability::VIEW_ANY, None).await
        {
            return Ok(Vec::new());
        }
        let title_column = self.column(title).column;
        let mut select = self.base(ctx, Trashed::Without);
        if !keys.is_empty() {
            select = select.filter(self.pk.is_in(self.decode_keys(keys)?));
        } else if let Some(search) = search.filter(|s| !s.is_empty()) {
            if search.chars().count() > self.limits.search_len {
                return Err(ValidationErrors::field("search", "is too long").into());
            }
            let pattern = format!("%{}%", escape_like(search));
            let kind = self.column(title).kind;
            select = select.filter(search_match(ctx.db.get_database_backend(), title_column, kind, &pattern));
        }
        // The policy may deny records the scope lets through: read in stable
        // batches until enough authorized options are found (bounded work).
        let select = select.order_by(title_column, Order::Asc).order_by(self.pk, Order::Asc);
        let (keys, complete) = self.read_candidate_keys(&ctx.db, &select, MAX_OPTIONS, MAX_OPTION_BATCHES).await?;
        let mut out = Vec::new();
        'chunks: for chunk in keys.chunks(LOAD_CHUNK) {
            for (model, record) in self.load_ordered(ctx, &select, chunk).await? {
                if self.policy.allows(ctx, ability::VIEW, Some(&record)).await {
                    out.push((self.key_of(&model), title_text(&record[title.as_str()])));
                    if out.len() as u64 == MAX_OPTIONS {
                        break 'chunks;
                    }
                }
            }
        }
        if out.len() as u64 == MAX_OPTIONS || complete {
            Ok(out)
        } else {
            // Budget spent before the candidates ran out: say so instead of
            // returning a list that silently misses valid choices.
            Err(ValidationErrors::field("search", "matches too many records to check; refine the search").into())
        }
    }

    async fn options(
        &self,
        ctx: &Ctx,
        field: &str,
        search: Option<&str>,
        keys: &[String],
        record: Option<&str>,
    ) -> AppResult<Vec<SelectOption>> {
        let Some(field) = self.field(field) else { return Err(AppError::NotFound) };
        let Some(target) = field.belongs_to else { return Err(AppError::NotFound) };
        // Options exist to show or fill the field: allowed for any operation the
        // user may perform with it. With `record` (an edit or view form), that
        // record is loaded in scope and the policy decides on it, exactly as
        // the form and the update itself do. The target's scope and policy
        // still decide which records appear.
        let usable = match record {
            Some(key) => {
                if !self.policy.admits(ctx, ability::UPDATE).await && !self.policy.admits(ctx, ability::VIEW).await {
                    return Err(AppError::Forbidden);
                }
                let record = Self::record(&self.find_model(ctx, &ctx.db, key).await?);
                (field.can_write(Operation::Update, ctx)
                    && self.policy.allows(ctx, ability::UPDATE, Some(&record)).await)
                    || (field.can_read(ctx) && self.policy.allows(ctx, ability::VIEW, Some(&record)).await)
            }
            None => {
                (field.can_read(ctx) && self.policy.allows(ctx, ability::VIEW_ANY, None).await)
                    || (field.can_write(Operation::Create, ctx) && self.policy.allows(ctx, ability::CREATE, None).await)
                    || (field.can_write(Operation::Update, ctx) && self.policy.allows(ctx, ability::UPDATE, None).await)
            }
        };
        if !usable {
            return Err(AppError::Forbidden);
        }
        if keys.len() > 100 {
            return Err(ValidationErrors::field("keys", "may not have more than 100 items").into());
        }
        let target =
            ctx.registry.get(target).ok_or_else(|| AppError::internal(format!("unknown resource `{target}`")))?;
        let kind = self.column(&field.name).kind;
        Ok(target
            .lookup(ctx, search, keys)
            .await?
            .into_iter()
            .map(|(key, label)| SelectOption { value: kind.from_query(&serde_json::Value::String(key)), label })
            .collect())
    }

    fn targets(&self) -> Vec<&'static str> {
        let forms = self.actions.iter().flat_map(|a| a.form.iter().filter_map(|f| f.belongs_to));
        self.fields.iter().filter_map(|f| f.belongs_to).chain(forms).collect()
    }

    async fn admit_list(&self, ctx: &Ctx, query: &ListQuery) -> AppResult<()> {
        if !self.policy.allows(ctx, ability::VIEW_ANY, None).await {
            return Err(AppError::Forbidden);
        }
        let per_page = query.per_page.unwrap_or(DEFAULT_PER_PAGE).clamp(1, self.limits.per_page);
        let page = query.page.unwrap_or(1).max(1);
        // The SQL offset must fit (databases take a signed 64-bit offset).
        let offset_fits = (page - 1).checked_mul(per_page).is_some_and(|offset| i64::try_from(offset).is_ok());
        if !offset_fits {
            return Err(ValidationErrors::field("page", "is too large").into());
        }

        if let Some((field, _)) = &query.parent
            && !self.field(field).is_some_and(|f| f.can_read(ctx))
        {
            // The association itself is data the user may not read.
            return Err(AppError::Forbidden);
        }
        if query.trashed != Trashed::Without {
            if self.soft.is_none() {
                return Err(ValidationErrors::field("trashed", "is not supported by this resource").into());
            }
            if !self.policy.admits(ctx, ability::RESTORE).await && !self.policy.admits(ctx, ability::FORCE_DELETE).await
            {
                return Err(AppError::Forbidden);
            }
        }
        Ok(())
    }

    async fn list(&self, ctx: &Ctx, query: &ListQuery) -> AppResult<Page> {
        self.admit_list(ctx, query).await?;
        let per_page = query.per_page.unwrap_or(DEFAULT_PER_PAGE).clamp(1, self.limits.per_page);
        let page = query.page.unwrap_or(1).max(1);
        let select = self.list_select(ctx, query)?;
        if !self.policy.view_depends_on_record() && query.trashed == Trashed::Without {
            // The same decision holds for every record: deny once, show nothing.
            if !self.policy.allows(ctx, ability::VIEW, None).await {
                return Ok(Page { data: Vec::new(), meta: PageMeta { page, per_page, total: 0, last_page: 1 } });
            }
            let paginator = select.paginate(&ctx.db, per_page);
            let totals = paginator.num_items_and_pages().await?;
            let models = paginator.fetch_page(page - 1).await?;
            return Ok(Page {
                data: self.envelopes(ctx, models).await?,
                meta: PageMeta {
                    page,
                    per_page,
                    total: totals.number_of_items,
                    last_page: totals.number_of_pages.max(1),
                },
            });
        }
        // Record-dependent visibility: check every matching record, so denied
        // ones are neither shown nor counted (bounded scan, fail closed).
        let (keys, complete) =
            self.read_candidate_keys(&ctx.db, &select, LIST_SCAN_BATCH, MAX_LIST_SCAN / LIST_SCAN_BATCH).await?;
        if !complete {
            let mut errors = ValidationErrors::new();
            errors.add_form("Too many matching records to check individually; narrow the search or filters.");
            return Err(errors.into());
        }
        let mut visible = Vec::new();
        let mut total = 0u64;
        let skip = (page - 1) * per_page;
        // Only the page's rows are kept; the rest are counted and dropped.
        for chunk in keys.chunks(LOAD_CHUNK) {
            for (model, record) in self.load_ordered(ctx, &select, chunk).await? {
                if self.listable(ctx, &record).await {
                    if total >= skip && (visible.len() as u64) < per_page {
                        visible.push(model);
                    }
                    total += 1;
                }
            }
        }
        Ok(Page {
            data: self.envelopes(ctx, visible).await?,
            meta: PageMeta { page, per_page, total, last_page: total.div_ceil(per_page).max(1) },
        })
    }

    async fn find(&self, ctx: &Ctx, key: &str) -> AppResult<Envelope> {
        self.admit(ctx, ability::VIEW).await?;
        let model = self.find_model(ctx, &ctx.db, key).await?;
        if !self.policy.allows(ctx, ability::VIEW, Some(&Self::record(&model))).await {
            return Err(AppError::Forbidden);
        }
        Ok(self.envelopes(ctx, vec![model]).await?.remove(0))
    }

    async fn create(&self, ctx: &Ctx, input: Data) -> AppResult<Envelope> {
        if !self.policy.allows(ctx, ability::CREATE, None).await {
            return Err(AppError::Forbidden);
        }
        // Through a relation, the foreign key is the relation's: never the client's.
        if let Some((_, field, _)) = &ctx.bound
            && input.contains_key(field)
        {
            return Err(ValidationErrors::field(field.as_str(), "is set by the relation").into());
        }
        // The request as sent is what a retry must repeat: a receipt is found
        // before validating, which may have changed (field permissions) since.
        let claim = self.create_claim(ctx, &input);
        let (txn, wctx) = self.begin(ctx).await?;
        if let Some(claim) = &claim
            && !crate::idempotency::acquire(txn.as_ref(), claim).await?
        {
            // Committed earlier: answer with that record, as the user sees it now.
            Self::abort(txn, wctx).await?;
            let receipt = crate::idempotency::receipt(&ctx.db, claim).await?;
            let key = receipt["key"].as_str().ok_or_else(|| AppError::internal("create receipt without a key"))?;
            return Ok(self.committed_key_envelope(ctx, key.to_owned()).await);
        }
        let mut input = input;
        let bound = ctx.bound.as_ref().map(|(_, field, parent)| (field.clone(), self.fk_value(field, parent)));
        if let Some((field, value)) = &bound {
            input.insert(field.clone(), value.clone());
        }
        // The usual rules apply to the bound key too (writable, typed, required).
        let input = self.accept_input(&wctx, Operation::Create, input)?;
        let parents = self.lock_parents(&wctx, &txn, self.parent_of_data(&input).flatten()).await?;
        let data = R::mutate_before_create(&wctx, &txn, input).await?;
        self.check_parent_written(&wctx, &data, &parents)?;
        if let Some((field, value)) = &bound
            && data.get(field) != Some(value)
        {
            return Err(AppError::internal(format!("`{}`: a hook changed the relation key `{field}`", R::SLUG)));
        }
        // What is written, hooks included: references are checked last.
        self.check_written(&wctx, &txn, &data).await?;

        let mut active = <<Entity<R> as EntityTrait>::ActiveModel as ActiveModelBehavior>::new();
        self.apply(&mut active, &data, Operation::Create)?;
        // New records are active, whatever the model's or the column's default.
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
        let model = active.insert(txn.as_ref()).await.map_err(|e| self.map_db_err(&wctx, e))?;
        self.check_parent_stored(&wctx, &model, &parents)?;
        R::after_save(&wctx, &txn, &model).await?;
        // As stored once the hooks ran.
        self.audit_outcome(&wctx, &txn, "created", None, &model, false, true).await?;
        if let Some(row) = &ctx.import_row {
            crate::imports::record_row(txn.as_ref(), row, "created", &self.key_of(&model)).await?;
        }
        if let Some(claim) = &claim {
            crate::idempotency::complete(txn.as_ref(), claim, &serde_json::json!({ "key": self.key_of(&model) }))
                .await?;
        }
        Self::commit(txn, wctx).await?;
        Ok(self.committed_envelope(ctx, model).await)
    }

    async fn update(&self, ctx: &Ctx, key: &str, input: Data) -> AppResult<Envelope> {
        self.admit(ctx, ability::UPDATE).await?;
        // With an idempotency key: applied once, a retry answers with the record.
        let claim = ctx.idempotency_key.as_deref().map(|k| {
            crate::idempotency::Claim::new(
                &ctx.principal,
                R::SLUG,
                &format!("update:{key}"),
                k,
                &serde_json::Value::Object(input.clone()),
            )
        });
        let input = self.accept_input(ctx, Operation::Update, input)?;
        let (txn, wctx) = self.begin(ctx).await?;
        if let Some(claim) = &claim
            && !crate::idempotency::acquire(txn.as_ref(), claim).await?
        {
            Self::abort(txn, wctx).await?;
            let receipt = crate::idempotency::receipt(&ctx.db, claim).await?;
            let key = receipt["key"].as_str().ok_or_else(|| AppError::internal("update receipt without a key"))?;
            return Ok(self.committed_key_envelope(ctx, key.to_owned()).await);
        }
        // An import row: the field rules of the user as they are now.
        let input = if ctx.import_row.is_some() { self.accept_input(&wctx, Operation::Update, input)? } else { input };
        let (model, parents) = self
            .find_authorized_locked_moving(&wctx, &txn, key, ability::UPDATE, Trashed::Without, Some(&input))
            .await?;
        let before = self.audit_snapshot(&model);
        let data = R::mutate_before_update(&wctx, &txn, &model, input).await?;
        self.check_parent_written(&wctx, &data, &parents)?;
        self.check_written(&wctx, &txn, &data).await?;
        let model = if data.is_empty() {
            model
        } else {
            let mut active = model.into_active_model();
            self.apply(&mut active, &data, Operation::Update)?;
            active.update(txn.as_ref()).await.map_err(|e| self.map_db_err(&wctx, e))?
        };
        self.check_parent_stored(&wctx, &model, &parents)?;
        R::after_save(&wctx, &txn, &model).await?;
        // Nothing changed: nothing to record.
        self.audit_outcome(&wctx, &txn, "updated", Some(&before), &model, false, false).await?;
        if let Some(row) = &ctx.import_row {
            crate::imports::record_row(txn.as_ref(), row, "updated", &self.key_of(&model)).await?;
        }
        if let Some(claim) = &claim {
            crate::idempotency::complete(txn.as_ref(), claim, &serde_json::json!({ "key": self.key_of(&model) }))
                .await?;
        }
        Self::commit(txn, wctx).await?;
        Ok(self.committed_envelope(ctx, model).await)
    }

    async fn delete(&self, ctx: &Ctx, key: &str) -> AppResult<()> {
        self.admit(ctx, ability::DELETE).await?;
        let (txn, wctx) = self.begin(ctx).await?;
        let model = self.find_authorized_locked(&wctx, &txn, key, ability::DELETE, Trashed::Without).await?;
        let event = if self.soft.is_some() { "trashed" } else { "deleted" };
        self.audit_write(&wctx, &txn, event, &self.key_of(&model), None, None).await?;
        if self.soft.is_some() {
            // To the trash: related records are kept (no database cascade runs).
            let mut active = model.into_active_model();
            self.set_marker(&mut active, true)?;
            active.update(txn.as_ref()).await.map_err(|e| self.map_db_err(&wctx, e))?;
        } else {
            Entity::<R>::delete_many()
                .filter(self.pk.eq(model.get(self.pk)))
                .exec(txn.as_ref())
                .await
                .map_err(|e| self.map_db_err(&wctx, e))?;
        }
        Self::commit(txn, wctx).await?;
        Ok(())
    }

    async fn restore(&self, ctx: &Ctx, key: &str) -> AppResult<Envelope> {
        if self.soft.is_none() {
            return Err(AppError::NotFound);
        }
        self.admit(ctx, ability::RESTORE).await?;
        let (txn, wctx) = self.begin(ctx).await?;
        let model = self.find_authorized_locked(&wctx, &txn, key, ability::RESTORE, Trashed::Only).await?;
        // Back among active records, its relationships must point to records
        // the user can see, as when it was written.
        let record = Self::record(&model);
        let relations: Data = self
            .fields
            .iter()
            .filter(|f| f.belongs_to.is_some() || f.media.is_some() || f.rich_text)
            .filter_map(|f| record.get(&f.name).map(|v| (f.name.clone(), v.clone())))
            .collect();
        self.check_written(&wctx, &txn, &relations).await?;
        let before = self.audit_snapshot(&model);
        let mut active = model.into_active_model();
        self.set_marker(&mut active, false)?;
        let model = active.update(txn.as_ref()).await.map_err(|e| self.map_db_err(&wctx, e))?;
        R::after_save(&wctx, &txn, &model).await?;
        self.audit_outcome(&wctx, &txn, "restored", Some(&before), &model, false, true).await?;
        Self::commit(txn, wctx).await?;
        Ok(self.committed_envelope(ctx, model).await)
    }

    async fn force_delete(&self, ctx: &Ctx, key: &str) -> AppResult<()> {
        if self.soft.is_none() {
            return Err(AppError::NotFound);
        }
        self.admit(ctx, ability::FORCE_DELETE).await?;
        let (txn, wctx) = self.begin(ctx).await?;
        let model = self.find_authorized_locked(&wctx, &txn, key, ability::FORCE_DELETE, Trashed::Only).await?;
        Entity::<R>::delete_many()
            .filter(self.pk.eq(model.get(self.pk)))
            .exec(txn.as_ref())
            .await
            .map_err(|e| self.map_db_err(&wctx, e))?;
        R::after_force_delete(&wctx, &txn, &model).await?;
        self.audit_write(&wctx, &txn, "force_deleted", &self.key_of(&model), None, None).await?;
        Self::commit(txn, wctx).await?;
        Ok(())
    }

    async fn run_action(&self, ctx: &Ctx, name: &str, keys: &[String], data: Data) -> AppResult<ActionOutcome> {
        let action = self.admitted_action(ctx, name).await?;
        // Checked and canonical before the claim: equivalent requests share it.
        let data = self.action_data(ctx, action, data)?;
        let claim = ctx.idempotency_key.as_deref().map(|key| {
            let unique: BTreeSet<&String> = keys.iter().collect();
            let request = serde_json::json!({ "keys": unique, "data": data });
            crate::idempotency::Claim::new(&ctx.principal, R::SLUG, &format!("action:{name}"), key, &request)
        });
        let (txn, wctx) = self.begin(ctx).await?;
        if let Some(claim) = &claim
            && !crate::idempotency::acquire(txn.as_ref(), claim).await?
        {
            // The action ran with this key before: report that run, never run it again.
            Self::abort(txn, wctx).await?;
            let receipt = crate::idempotency::receipt(&ctx.db, claim).await?;
            let mut outcome: ActionOutcome = serde_json::from_value(receipt).map_err(AppError::internal)?;
            outcome.replayed = true;
            return self.disclosed(ctx, outcome).await;
        }
        // References are checked when the action runs (a replay above reports
        // the committed run, whatever they became).
        self.check_action_references(&wctx, txn.as_ref(), action, &data).await?;
        // Lock only records the user may act on, then re-read just those under
        // the lock: a record that became visible meanwhile is never processed
        // unlocked.
        let mut outcome = ActionOutcome::default();
        let mut candidates = Vec::new();
        let mut seen = HashSet::new();
        let first_pass = self.models_in_scope(&wctx, txn.as_ref(), keys).await?;
        for (model, record) in &first_pass {
            let key = self.key_of(model);
            seen.insert(key.clone());
            if self.policy.allows(&wctx, action.ability, Some(record)).await {
                candidates.push(key);
            } else {
                outcome.denied.push(key);
            }
        }
        // Candidates are re-counted below; any not locked (deleted meanwhile)
        // or gone on reload end up in `missing`.
        seen.retain(|k| !candidates.contains(k));
        // Parents of the whole batch first, then the records (one order for every writer).
        let read_parents: HashMap<String, Option<String>> = first_pass
            .iter()
            .filter(|(model, _)| candidates.contains(&self.key_of(model)))
            .map(|(model, _)| (self.key_of(model), self.parent_of_model(model)))
            .collect();
        let parents = self.lock_parents(&wctx, &txn, read_parents.values().flatten().cloned()).await?;
        let locked = self.lock_rows(&txn, &candidates, Trashed::Without).await?;
        candidates.retain(|k| locked.contains(k));
        let mut allowed = Vec::new();
        for (model, record) in self.models_in_scope(&wctx, txn.as_ref(), &candidates).await? {
            let key = self.key_of(&model);
            if read_parents.get(&key).is_some_and(|parent| *parent != self.parent_of_model(&model)) {
                return Err(super::write::parent_moved());
            }
            seen.insert(key.clone());
            if self.policy.allows(&wctx, action.ability, Some(&record)).await {
                outcome.processed.push(key);
                allowed.push(model);
            } else {
                outcome.denied.push(key);
            }
        }
        outcome.missing =
            keys.iter().filter(|k| !seen.contains(*k)).cloned().collect::<BTreeSet<_>>().into_iter().collect();
        if !allowed.is_empty() {
            let before: Vec<_> =
                allowed.iter().map(|m| (self.key_of(m), self.audit_snapshot(m), self.trashed(m))).collect();
            let processed: Vec<String> = allowed.iter().map(|m| self.key_of(m)).collect();
            (action.handler)(&wctx, txn.as_ref(), allowed, data).await?;
            // Handlers write freely: none may move a record to a parent not held.
            if self.meta.parent_lock.is_some() {
                for model in self.models_by_keys(txn.as_ref(), &processed).await? {
                    self.check_parent_stored(&wctx, &model, &parents)?;
                }
            }
            self.audit_action(&wctx, &txn, name, before).await?;
        }
        outcome.processed.sort();
        outcome.denied.sort();
        if let Some(claim) = &claim {
            let result = serde_json::to_value(&outcome).map_err(AppError::internal)?;
            crate::idempotency::complete(txn.as_ref(), claim, &result).await?;
        }
        Self::commit(txn, wctx).await?;
        self.disclosed(ctx, outcome).await
    }

    async fn titles(&self, ctx: &Ctx, keys: &[String]) -> AppResult<BTreeMap<String, String>> {
        let Some(title) = &self.meta.title_field else {
            return Ok(BTreeMap::new());
        };
        if !self.field(title).is_some_and(|f| f.can_read(ctx))
            || !self.policy.allows(ctx, ability::VIEW_ANY, None).await
        {
            return Ok(BTreeMap::new());
        }
        let mut out = BTreeMap::new();
        for (model, record) in self.models_in_scope(ctx, &ctx.db, keys).await? {
            if self.policy.allows(ctx, ability::VIEW, Some(&record)).await {
                out.insert(self.key_of(&model), title_text(&record[title.as_str()]));
            }
        }
        Ok(out)
    }

    async fn lock_keys(&self, txn: &DatabaseTransaction, keys: &[String]) -> AppResult<()> {
        self.lock_rows(txn, keys, Trashed::With).await?;
        Ok(())
    }

    async fn admit_import(&self, ctx: &Ctx) -> AppResult<()> {
        ResourceAdapter::admit_import(self, ctx).await
    }

    fn widget_problem(&self, query: &ListQuery, date: Option<&str>, columns: &[String]) -> Option<String> {
        ResourceAdapter::widget_problem(self, query, date, columns)
    }

    fn reads_all(&self, ctx: &Ctx, names: &[&str]) -> bool {
        ResourceAdapter::reads_all(self, ctx, names)
    }

    fn field_label(&self, ctx: &Ctx, name: &str) -> String {
        self.field(name).map_or_else(|| name.to_owned(), |f| ctx.t_in(R::SLUG, &f.display_label()))
    }

    async fn counted(
        &self,
        ctx: &Ctx,
        query: &ListQuery,
        date: Option<&str>,
        ranges: &[(chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)],
    ) -> AppResult<crate::registry::Counted> {
        ResourceAdapter::counted(self, ctx, query, date, ranges).await
    }

    fn import_fields(&self, ctx: &Ctx) -> Vec<crate::ImportField> {
        ResourceAdapter::import_fields(self, ctx)
    }

    fn import_key(&self) -> Option<&str> {
        self.meta.import_key.as_deref()
    }

    fn import_value(&self, name: &str, text: &str, quoted: bool) -> serde_json::Value {
        ResourceAdapter::import_value(self, name, text, quoted)
    }

    fn check_import_row(&self, ctx: &Ctx, op: Operation, data: Data) -> AppResult<Data> {
        ResourceAdapter::check_import_row(self, ctx, op, data)
    }

    async fn import_matches(&self, ctx: &Ctx, value: &serde_json::Value) -> AppResult<Vec<String>> {
        ResourceAdapter::import_matches(self, ctx, value).await
    }

    async fn admit_export(&self, ctx: &Ctx) -> AppResult<()> {
        ResourceAdapter::admit_export(self, ctx).await
    }

    fn export_columns(&self, ctx: &Ctx) -> Vec<(String, crate::exports::CellKind)> {
        ResourceAdapter::export_columns(self, ctx)
    }

    async fn canonical_query(&self, ctx: &Ctx, query: &ListQuery) -> AppResult<ListQuery> {
        ResourceAdapter::canonical_query(self, ctx, query).await
    }

    async fn export_keys(&self, ctx: &Ctx, query: &ListQuery, max: usize) -> AppResult<Vec<String>> {
        ResourceAdapter::export_keys(self, ctx, query, max).await
    }

    async fn export_rows(
        &self,
        ctx: &Ctx,
        query: &ListQuery,
        keys: &[String],
        columns: &[String],
    ) -> AppResult<Vec<(String, Data)>> {
        ResourceAdapter::export_rows(self, ctx, query, keys, columns).await
    }

    async fn admit_action(&self, ctx: &Ctx, name: &str) -> AppResult<()> {
        self.admitted_action(ctx, name).await.map(|_| ())
    }

    async fn action_options(
        &self,
        ctx: &Ctx,
        name: &str,
        field: &str,
        search: Option<&str>,
        keys: &[String],
    ) -> AppResult<Vec<SelectOption>> {
        self.action_field_options(ctx, name, field, search, keys).await
    }

    async fn lock_visible(&self, ctx: &Ctx, txn: &DatabaseTransaction, keys: &[String]) -> AppResult<HashSet<String>> {
        self.locked_visible(ctx, txn, keys).await
    }

    async fn admit_link(&self, ctx: &Ctx, relation: &str, op: crate::links::LinkOp) -> AppResult<()> {
        self.admit_many(ctx, relation, op).await
    }

    async fn link_targets(
        &self,
        ctx: &Ctx,
        relation: &str,
        key: &str,
    ) -> AppResult<(String, crate::resource::Through)> {
        self.many_through(ctx, relation, key).await
    }

    async fn link_values(
        &self,
        ctx: &Ctx,
        relation: &str,
        key: &str,
        targets: &[String],
    ) -> AppResult<BTreeMap<String, Data>> {
        self.many_values(ctx, relation, key, targets).await
    }

    async fn link(
        &self,
        ctx: &Ctx,
        relation: &str,
        key: &str,
        request: crate::links::LinkRequest,
    ) -> AppResult<crate::links::LinkOutcome> {
        ResourceAdapter::link(self, ctx, relation, key, request).await
    }

    async fn link_options(
        &self,
        ctx: &Ctx,
        relation: &str,
        key: &str,
        search: Option<&str>,
        keys: &[String],
    ) -> AppResult<Vec<crate::links::LinkOption>> {
        ResourceAdapter::link_options(self, ctx, relation, key, search, keys).await
    }

    async fn keys_in_scope(&self, ctx: &Ctx, txn: &DatabaseTransaction, keys: &[String]) -> AppResult<HashSet<String>> {
        if !self.policy.allows(ctx, ability::VIEW_ANY, None).await {
            return Ok(HashSet::new());
        }
        let mut out = HashSet::new();
        for (model, record) in self.models_in_scope(ctx, txn, keys).await? {
            if self.policy.allows(ctx, ability::VIEW, Some(&record)).await {
                out.insert(self.key_of(&model));
            }
        }
        Ok(out)
    }
}
