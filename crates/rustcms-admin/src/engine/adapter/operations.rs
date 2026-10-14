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
        abilities.extend(self.actions.iter().map(|a| a.ability));
        abilities.sort_unstable();
        abilities.dedup();
        let mut permissions: Vec<String> = abilities.into_iter().map(|a| format!("{}.{a}", R::SLUG)).collect();
        // Explicit admission permissions are checked verbatim: declare them too.
        permissions.extend(self.actions.iter().filter_map(|a| a.required_permission).map(str::to_owned));
        permissions.sort_unstable();
        permissions.dedup();
        permissions
    }

    async fn schema(&self, ctx: &Ctx, op: SchemaOperation, key: Option<&str>) -> AppResult<ResourceSchema> {
        let mut abilities = BTreeMap::new();
        abilities.insert("viewAny", self.policy.allows(ctx, ability::VIEW_ANY, None).await);
        abilities.insert("create", self.policy.allows(ctx, ability::CREATE, None).await);
        if self.soft.is_some() {
            // Whether the trash can be listed; each record still decides.
            abilities.insert("restore", self.policy.admits(ctx, ability::RESTORE).await);
            abilities.insert("forceDelete", self.policy.admits(ctx, ability::FORCE_DELETE).await);
        }
        if let Some(key) = key {
            let admission = if op == SchemaOperation::Edit { ability::UPDATE } else { ability::VIEW };
            self.admit(ctx, admission).await?;
            let record = Self::record(&self.find_model(ctx, &ctx.db, key).await?);
            for name in [ability::VIEW, ability::UPDATE, ability::DELETE] {
                abilities.insert(name, self.policy.allows(ctx, name, Some(&record)).await);
            }
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
            relations: if op == SchemaOperation::View && key.is_some() {
                self.relation_schemas(ctx).await
            } else {
                Vec::new()
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
        self.fields.iter().filter_map(|f| f.belongs_to).collect()
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
        let mut errors = ValidationErrors::new();
        let mut select = self.base(ctx, query.trashed).filter(self.filter_condition(ctx, query, &mut errors));
        // Without `sort`, the table's default sort applies (when this user may use it),
        // so the order matches what the schema announces.
        let explicit = query.sort.is_some();
        let sort = query.sort.clone().or_else(|| self.table_ui.default_sort.clone());
        if let Some(sort) = sort.as_deref() {
            let (order, name) = match sort.strip_prefix('-') {
                Some(name) => (Order::Desc, name),
                None => (Order::Asc, sort),
            };
            if self.field(name).is_some_and(|f| f.sortable && f.can_read(ctx)) {
                select = select.order_by(self.column(name).column, order);
            } else if explicit {
                errors.add("sort", format!("cannot sort by `{name}`"));
            }
        }
        errors.into_result()?;
        let select = select.order_by(self.pk, Order::Asc);
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
        let input = self.accept_input(ctx, Operation::Create, input)?;
        let data = R::mutate_before_create(&wctx, &txn, input).await?;
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
        let model = active.insert(txn.as_ref()).await.map_err(|e| self.map_db_err(e))?;
        R::after_save(&wctx, &txn, &model).await?;
        if let Some(claim) = &claim {
            crate::idempotency::complete(txn.as_ref(), claim, &serde_json::json!({ "key": self.key_of(&model) }))
                .await?;
        }
        Self::commit(txn, wctx).await?;
        Ok(self.committed_envelope(ctx, model).await)
    }

    async fn update(&self, ctx: &Ctx, key: &str, input: Data) -> AppResult<Envelope> {
        self.admit(ctx, ability::UPDATE).await?;
        let input = self.accept_input(ctx, Operation::Update, input)?;
        let (txn, wctx) = self.begin(ctx).await?;
        let model = self.find_authorized_locked(&wctx, &txn, key, ability::UPDATE, Trashed::Without).await?;
        let data = R::mutate_before_update(&wctx, &txn, &model, input).await?;
        self.check_written(&wctx, &txn, &data).await?;
        let model = if data.is_empty() {
            model
        } else {
            let mut active = model.into_active_model();
            self.apply(&mut active, &data, Operation::Update)?;
            active.update(txn.as_ref()).await.map_err(|e| self.map_db_err(e))?
        };
        R::after_save(&wctx, &txn, &model).await?;
        Self::commit(txn, wctx).await?;
        Ok(self.committed_envelope(ctx, model).await)
    }

    async fn delete(&self, ctx: &Ctx, key: &str) -> AppResult<()> {
        self.admit(ctx, ability::DELETE).await?;
        let (txn, wctx) = self.begin(ctx).await?;
        let model = self.find_authorized_locked(&wctx, &txn, key, ability::DELETE, Trashed::Without).await?;
        if self.soft.is_some() {
            // To the trash: related records are kept (no database cascade runs).
            let mut active = model.into_active_model();
            self.set_marker(&mut active, true)?;
            active.update(txn.as_ref()).await.map_err(|e| self.map_db_err(e))?;
        } else {
            Entity::<R>::delete_many()
                .filter(self.pk.eq(model.get(self.pk)))
                .exec(txn.as_ref())
                .await
                .map_err(|e| self.map_db_err(e))?;
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
        let mut active = model.into_active_model();
        self.set_marker(&mut active, false)?;
        let model = active.update(txn.as_ref()).await.map_err(|e| self.map_db_err(e))?;
        R::after_save(&wctx, &txn, &model).await?;
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
            .map_err(|e| self.map_db_err(e))?;
        R::after_force_delete(&wctx, &txn, &model).await?;
        Self::commit(txn, wctx).await?;
        Ok(())
    }

    async fn run_action(&self, ctx: &Ctx, name: &str, keys: &[String], data: Data) -> AppResult<ActionOutcome> {
        let action = self.actions.iter().find(|a| a.name == name).ok_or(AppError::NotFound)?;
        // Per-record policy checks are authoritative; an explicit admission
        // permission, when declared, refuses outright before them.
        if action.required_permission.is_some_and(|p| !ctx.can(p)) {
            return Err(AppError::Forbidden);
        }
        self.admit(ctx, action.ability).await?;
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
        // Lock only records the user may act on, then re-read just those under
        // the lock: a record that became visible meanwhile is never processed
        // unlocked.
        let mut outcome = ActionOutcome::default();
        let mut candidates = Vec::new();
        let mut seen = HashSet::new();
        for (model, record) in self.models_in_scope(&wctx, txn.as_ref(), keys).await? {
            let key = self.key_of(&model);
            seen.insert(key.clone());
            if self.policy.allows(&wctx, action.ability, Some(&record)).await {
                candidates.push(key);
            } else {
                outcome.denied.push(key);
            }
        }
        // Candidates are re-counted below; any not locked (deleted meanwhile)
        // or gone on reload end up in `missing`.
        seen.retain(|k| !candidates.contains(k));
        let locked = self.lock_rows(&txn, &candidates, Trashed::Without).await?;
        candidates.retain(|k| locked.contains(k));
        let mut allowed = Vec::new();
        for (model, record) in self.models_in_scope(&wctx, txn.as_ref(), &candidates).await? {
            let key = self.key_of(&model);
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
            (action.handler)(&wctx, txn.as_ref(), allowed, data).await?;
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
