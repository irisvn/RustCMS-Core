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

    fn attach_rich_nodes(&self, nodes: &Arc<crate::rich_text::AppNodes>) {
        for field in &self.fields {
            for name in &field.rich_nodes {
                assert!(
                    nodes.get(name).is_some(),
                    "resource `{}`: field `{}` accepts the rich text node `{name}`, which is not declared \
                     (`AdminProvider::rich_text_node`)",
                    R::SLUG,
                    field.name
                );
            }
        }
        // The rich editors of JSON rows (ADR 0044).
        for rows in self.repeaters.values() {
            rows.attach_nodes(&format!("resource `{}`", R::SLUG), nodes);
        }
        let _ = self.rich_nodes.set(nodes.clone());
    }

    fn media_collections(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> =
            self.fields.iter().filter_map(|f| Some((f.name.clone(), f.collection.clone()?))).collect();
        // Inputs of JSON rows name theirs (ADR 0044).
        for input in self.form.inputs() {
            if let Some(spec) = input.repeater.as_deref() {
                crate::engine::repeater::row_collections(spec, &input.name, &mut out);
            }
        }
        out
    }

    fn field_link(&self, name: &str) -> Option<(Option<&'static str>, &'static str)> {
        self.field(name).map(|f| (f.belongs_to, self.column(name).kind.name()))
    }

    fn condition_kind(&self, name: &str) -> Option<(FieldKind, Option<u32>)> {
        super::input::condition_kind(&self.columns, &self.secrets, name)
    }

    fn sorts_by(&self, ctx: &Ctx, name: &str) -> bool {
        self.field(name).is_some_and(|f| f.sortable && f.can_read(ctx))
    }

    fn check_relation_entries(&self, registry: &crate::Registry) {
        let Some(infolist) = self.infolist.as_ref().filter(|i| !i.from_form) else { return };
        for entry in infolist.entries().into_iter().filter(|e| e.relationship) {
            let relation = self.relations.iter().find(|r| r.name == entry.name).expect("checked with the infolist");
            let Some(child) = registry.get(relation.target) else { continue };
            let here = format!("resource `{}`: repeatable entry `{}`", R::SLUG, entry.name);
            child.check_entry_rows(&here, &entry.components);
        }
    }

    fn check_entry_rows(&self, here: &str, components: &[Component]) {
        super::infolist::check_relation_rows(here, components, &self.fields, &self.columns);
    }

    fn entry_row_schemas(&self, ctx: &Ctx, components: &[Component]) -> Vec<ComponentSchema> {
        self.entry_components(ctx, components, &super::infolist::EntryLinks::default())
    }

    fn relation_filter_value(&self, name: &str, filter: &RelationFilter) -> Result<serde_json::Value, String> {
        if self.field(name).is_none() || self.secrets.contains_key(name) {
            return Err("is not a field (encrypted fields cannot be filtered)".to_owned());
        }
        let column = self.column(name);
        match filter {
            RelationFilter::Null if !column.nullable => Err("`where_null` needs a nullable column".to_owned()),
            RelationFilter::Null => Ok(serde_json::Value::Null),
            RelationFilter::Eq(value) if value.is_null() => {
                Err("`where_eq` takes a value (`where_null` for none)".to_owned())
            }
            RelationFilter::Eq(value) => self
                .canonical_filter(name, filter)
                .map_err(|_| format!("`where_eq` value {value} is not a {}", column.kind.name())),
        }
    }

    fn stores_hidden(&self, name: &str) -> bool {
        self.columns.get(name).is_some_and(|c| c.nullable || c.has_default)
    }

    fn compile_row_rules(&self, registry: &crate::Registry) {
        // The record's form, as the rows' `when_root` reads it.
        let names: HashSet<String> = self.form.inputs().into_iter().map(|i| i.name.clone()).collect();
        let root = |name: &str| names.contains(name).then(|| self.condition_kind(name)).flatten();
        for repeater in &self.row_repeaters {
            let Some(child) = registry.get(repeater.target) else { continue };
            let here = format!("resource `{}`: repeater `{}`", R::SLUG, repeater.name);
            let row =
                |name: &str| repeater.inputs.iter().any(|n| n == name).then(|| child.condition_kind(name)).flatten();
            let outer = crate::engine::conditions::Outer { parent: None, root: Some(&root) };
            let conditions =
                crate::engine::conditions::FormConditions::new_in(&here, &repeater.spec.components, &row, outer);
            for (name, _) in conditions.conditional() {
                assert!(
                    child.stores_hidden(name),
                    "{here}: row input `{name}` has a condition but `{}.{name}` is not nullable and has no default (a \
                     row created while it is hidden could not be stored): declare a default or make it nullable",
                    repeater.target
                );
            }
            let mut components = repeater.spec.components.clone();
            let kind_of = |scope: crate::ui::Scope, name: &str| match scope {
                crate::ui::Scope::Row => row(name),
                crate::ui::Scope::Root => root(name),
                crate::ui::Scope::Parent => None,
            };
            crate::engine::conditions::canonicalize_with(&mut components, &kind_of, &root);
            let _ = repeater.rules.set(crate::adapter::rows::RowRules { conditions, components });
        }
    }

    fn field_is_encrypted(&self, name: &str) -> bool {
        self.field(name).is_some_and(|f| f.secret.is_some())
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
        // The media library is filled by uploads (`POST media`), never by the generic create.
        if std::any::TypeId::of::<R>() == std::any::TypeId::of::<crate::media::MediaResource>() {
            abilities.insert("upload", ctx.can(crate::media::UPLOAD));
        }
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
            let record = self.record(&self.find_model(ctx, &ctx.db, key).await?);
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
        let links = self.link_inputs(ctx, op, viewed.as_ref()).await;
        Ok(ResourceSchema {
            schema_version: SCHEMA_VERSION,
            vocabulary_version: VOCABULARY_VERSION,
            resource: ResourceAdapter::info(self).translated(ctx),
            operation: op,
            abilities,
            form: FormSchema {
                columns: self.form.columns,
                components: self
                    .fill_row_repeaters(ctx, self.components(ctx, &self.form.components, op, &links), op)
                    .await,
                width: self.form.width,
            },
            table: self.table_schema(ctx).await,
            relations: match (&viewed, op) {
                (Some(record), SchemaOperation::View) => self.relation_schemas(ctx, record).await,
                _ => Vec::new(),
            },
            // Only a record's view page draws it (lists ask for `view` without a key).
            infolist: match (&viewed, op) {
                (Some(_), SchemaOperation::View) => self.infolist_schema(ctx).await,
                _ => None,
            },
        })
    }

    fn row_schema(&self, ctx: &Ctx, components: &[Component], op: SchemaOperation) -> Vec<ComponentSchema> {
        // Rows are checked with this resource's rules: its form's options and
        // number bounds show in the rows (ADR 0041).
        fn inherit(components: &[Component], form: &[&crate::ui::Input]) -> Vec<Component> {
            components
                .iter()
                .map(|c| match c {
                    Component::Input(input) => {
                        let mut input = input.clone();
                        // Rows never run reactions (ADR 0043): no `reacts` in their schema.
                        input.reaction = None;
                        if let Some(own) = form.iter().find(|i| i.name == input.name) {
                            if input.options.is_empty() {
                                input.options = own.options.clone();
                            }
                            if input.number.is_none() {
                                input.number = own.number.clone();
                            }
                        }
                        Component::Input(input)
                    }
                    Component::Layout(layout) => {
                        let mut layout = layout.clone();
                        layout.children = inherit(&layout.children, form);
                        Component::Layout(layout)
                    }
                    other => other.clone(),
                })
                .collect()
        }
        let rows = inherit(components, &self.form.inputs());
        let mut schemas = self.components(ctx, &rows, op, &HashMap::new());
        // Rows show a compact rich editor unless the input asks for the full one (ADR 0044).
        fn compact(list: &mut [ComponentSchema], inputs: &[&crate::ui::Input]) {
            for component in list {
                match component {
                    ComponentSchema::Input(schema) if schema.kind == InputType::RichEditor => {
                        let content = inputs.iter().find(|i| i.name == schema.name).and_then(|i| i.content.as_deref());
                        schema.toolbar = crate::ui::RowContent::toolbar(content);
                    }
                    ComponentSchema::Layout(layout) => compact(&mut layout.components, inputs),
                    _ => {}
                }
            }
        }
        compact(&mut schemas, &crate::engine::repeater::row_inputs(&rows));
        schemas
    }

    fn row_repeater_fields(&self) -> Vec<(&'static str, &'static str, Vec<String>, Option<String>)> {
        self.row_repeaters.iter().map(|r| (r.name, r.target, r.inputs.clone(), r.order_column.clone())).collect()
    }

    async fn write_rows(
        &self,
        ctx: &Ctx,
        txn: &Arc<sea_orm::DatabaseTransaction>,
        target: crate::adapter::rows::RowTarget<'_>,
        change: crate::adapter::rows::RowChange,
    ) -> AppResult<crate::adapter::rows::RowsOutcome> {
        self.write_rows_in(ctx, txn, target, change).await
    }

    async fn computed(&self, ctx: &Ctx, key: &str) -> AppResult<crate::schema::ComputedValues> {
        self.computed_values(ctx, key).await
    }

    async fn react(
        &self,
        ctx: &Ctx,
        request: crate::schema::FormReactRequest,
    ) -> AppResult<crate::schema::FormReactResponse> {
        ResourceAdapter::react(self, ctx, request).await
    }

    async fn lookup(&self, ctx: &Ctx, search: Option<&str>, keys: &[String]) -> AppResult<Vec<(String, String)>> {
        let Some(title) = self.readable_title(ctx) else { return Ok(Vec::new()) };
        if !self.policy.allows(ctx, ability::VIEW_ANY, None).await {
            return Ok(Vec::new());
        }
        let mut select = self.base(ctx, Trashed::Without);
        if !keys.is_empty() {
            select = select.filter(self.pk.is_in(self.decode_keys(keys)?));
        } else if let Some(search) = search.filter(|s| !s.is_empty()) {
            if search.chars().count() > self.limits.search_len {
                return Err(ValidationErrors::field("search", "is too long").into());
            }
            // Each word matches a field of the title (ADR 0038): "Math 10"
            // finds "Math (grade 10)". One word: as before.
            let backend = ctx.db.get_database_backend();
            let mut matches = sea_orm::Condition::all();
            // A word repeated adds nothing ("a a a …" would only add work).
            let mut words: Vec<&str> = search.split_whitespace().collect();
            words.sort_unstable();
            words.dedup();
            for word in words {
                let pattern = format!("%{}%", escape_like(word));
                let mut any = sea_orm::Condition::any();
                for field in title.fields() {
                    let column = self.column(field);
                    any = any.add(search_match(backend, column.column, column.kind, &pattern));
                }
                matches = matches.add(any);
            }
            select = select.filter(matches);
        }
        // The policy may deny records the scope lets through: read in stable
        // batches until enough authorized options are found (bounded work).
        for field in title.fields() {
            select = select.order_by(self.column(field).column, Order::Asc);
        }
        let select = select.order_by(self.pk, Order::Asc);
        let (keys, complete) = self.read_candidate_keys(&ctx.db, &select, MAX_OPTIONS, MAX_OPTION_BATCHES).await?;
        let mut out = Vec::new();
        'chunks: for chunk in keys.chunks(LOAD_CHUNK) {
            for (model, record) in self.load_ordered(ctx, &select, chunk).await? {
                if self.policy.allows(ctx, ability::VIEW, Some(&record)).await {
                    let key = self.key_of(&model);
                    let label = title.render(&record).unwrap_or_else(|| format!("#{key}"));
                    out.push((key, label));
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
                let record = self.record(&self.find_model(ctx, &ctx.db, key).await?);
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
        if !self.policy.allows(ctx, ability::VIEW, Some(&self.record(&model))).await {
            return Err(AppError::Forbidden);
        }
        Ok(self.envelopes(ctx, vec![model]).await?.remove(0))
    }

    async fn create(&self, ctx: &Ctx, input: Data) -> AppResult<Envelope> {
        if !self.policy.allows(ctx, ability::CREATE, None).await {
            return Err(AppError::Forbidden);
        }
        // Through a relation, the foreign key is the relation's: never the client's;
        // so are the fields a filtered relation sets (ADR 0044).
        let fixed = self.filter_values(&bound_filters(ctx))?;
        if let Some((_, field, _)) = &ctx.bound
            && input.contains_key(field)
        {
            return Err(ValidationErrors::field(field.as_str(), "is set by the relation").into());
        }
        if let Some((field, _)) = fixed.iter().find(|(f, _)| input.contains_key(*f)) {
            return Err(ValidationErrors::field(*field, "is set by the relation").into());
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
        // An import row: CREATE allowed to the user as they are now (under the RBAC lock).
        if ctx.import_row.is_some() && !self.policy.allows(&wctx, ability::CREATE, None).await {
            Self::abort(txn, wctx).await?;
            return Err(AppError::Forbidden);
        }
        let mut input = input;
        // Links chosen in the form (ADR 0035): admitted and checked, written after the record.
        let links = self.take_links(&wctx, Operation::Create, &mut input).await?;
        // Relationship repeaters (ADR 0040): admitted and checked, written after the record.
        let rows = self.take_rows(&wctx, Operation::Create, &mut input).await?;
        let bound = ctx.bound.as_ref().map(|(_, field, parent)| (field.clone(), self.fk_value(field, parent)));
        if let Some((field, value)) = &bound {
            input.insert(field.clone(), value.clone());
        }
        // The usual rules apply to the bound key too (writable, typed, required).
        let mut input = self.accept_input(&wctx, Operation::Create, input)?;
        // Conditions (ADR 0037) on the input and the columns' defaults.
        let bound_field = bound.as_ref().map(|(field, _)| field.as_str());
        // A filtered relation's values are read by the conditions (as the panel's form
        // holds them), and stored whether or not an input shows them (ADR 0044).
        input.extend(fixed.iter().map(|(f, v)| ((*f).to_owned(), v.clone())));
        let (hidden_before, hidden) = self.apply_conditions(&wctx, Operation::Create, &mut input, None, bound_field)?;
        input.extend(fixed.iter().map(|(f, v)| ((*f).to_owned(), v.clone())));
        // Relationship repeaters hidden by the form's conditions (ADR 0044).
        let rows = self.settle_rows(Operation::Create, rows, &hidden_before, &hidden)?;
        let root = self.row_root(&wctx, Operation::Create, None, &input);
        let parents = self.lock_parents(&wctx, &txn, self.parent_of_data(&input).flatten()).await?;
        let data = R::mutate_before_create(&wctx, &txn, input).await?;
        self.check_parent_written(&wctx, &data, &parents)?;
        self.check_filters_kept(&data, &fixed)?;
        if let Some((field, value)) = &bound
            && data.get(field) != Some(value)
        {
            return Err(AppError::internal(format!("`{}`: a hook changed the relation key `{field}`", R::SLUG)));
        }
        // What is written, hooks included: references are read here; media
        // references (explicit shared locks) are checked last (ADR 0041).
        self.check_relations(&wctx, &txn, &data).await?;
        // Link targets, before the row's foreign keys lock what they reference.
        let links = self.lock_links(&wctx, &txn, links, &data).await?;

        let mut active = <<Entity<R> as EntityTrait>::ActiveModel as ActiveModelBehavior>::new();
        self.apply(&wctx, &mut active, &data, Operation::Create, None)?;
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
        let link_audits = self.write_links(&wctx, &txn, &model, links).await?;
        // The children, once the parent is written (locked after it); `after_save` sees them.
        let mut row_outcomes = self.write_row_changes(&wctx, &txn, &model, rows, &root).await?;
        // Media last, the rows' with the record's: the media resource ranks after every other (ADR 0041, 0044).
        let row_media = row_outcomes.iter_mut().flat_map(|(_, o)| std::mem::take(&mut o.media)).collect();
        self.check_media(&wctx, &txn, &data, row_media).await?;
        R::after_save(&wctx, &txn, &model).await?;
        // As stored once the hooks ran.
        self.audit_outcome(&wctx, &txn, "created", None, &model, false, true).await?;
        self.audit_links(&wctx, &txn, &model, link_audits).await?;
        self.audit_rows(&wctx, &txn, &model, row_outcomes).await?;
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
                &serde_json::Value::Object(self.claim_input(ctx, &input)),
            )
        });
        let mut input = input;
        // Links chosen in the form (ADR 0035): admitted and checked, written after the record.
        let links = self.take_links(ctx, Operation::Update, &mut input).await?;
        // Relationship repeaters (ADR 0040): admitted and checked, written after the record.
        let rows = self.take_rows(ctx, Operation::Update, &mut input).await?;
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
        // Conditions (ADR 0037) on the locked record merged with the input.
        let mut input = input;
        let record = self.record(&model);
        let viewable = self.policy.allows(&wctx, ability::VIEW, Some(&record)).await;
        let (hidden_before, hidden) =
            self.apply_conditions(&wctx, Operation::Update, &mut input, Some((&record, viewable)), None)?;
        // Relationship repeaters hidden by the form's conditions: a write hiding one removes its rows (ADR 0044).
        let rows = self.settle_rows(Operation::Update, rows, &hidden_before, &hidden)?;
        let root = self.row_root(&wctx, Operation::Update, Some((&record, viewable)), &input);
        let data = R::mutate_before_update(&wctx, &txn, &model, input).await?;
        self.check_parent_written(&wctx, &data, &parents)?;
        self.check_relations(&wctx, &txn, &data).await?;
        // Link targets, before the row's foreign keys lock what they reference.
        let links = self.lock_links(&wctx, &txn, links, &data).await?;
        let model = if data.is_empty() {
            model
        } else {
            let stored = model.clone();
            let mut active = model.into_active_model();
            self.apply(&wctx, &mut active, &data, Operation::Update, Some(&stored))?;
            active.update(txn.as_ref()).await.map_err(|e| self.map_db_err(&wctx, e))?
        };
        self.check_parent_stored(&wctx, &model, &parents)?;
        let link_audits = self.write_links(&wctx, &txn, &model, links).await?;
        // The children, once the parent is written (locked after it); `after_save` sees them.
        let mut row_outcomes = self.write_row_changes(&wctx, &txn, &model, rows, &root).await?;
        // Media last, the rows' with the record's: the media resource ranks after every other (ADR 0041, 0044).
        let row_media = row_outcomes.iter_mut().flat_map(|(_, o)| std::mem::take(&mut o.media)).collect();
        self.check_media(&wctx, &txn, &data, row_media).await?;
        R::after_save(&wctx, &txn, &model).await?;
        // Nothing changed: nothing to record.
        self.audit_outcome(&wctx, &txn, "updated", Some(&before), &model, false, false).await?;
        self.audit_links(&wctx, &txn, &model, link_audits).await?;
        self.audit_rows(&wctx, &txn, &model, row_outcomes).await?;
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
        self.delete_locked(&wctx, &txn, model).await?;
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
        let record = self.record(&model);
        let relations: Data = self
            .fields
            .iter()
            // JSON repeaters too: their rows' files and images (ADR 0044).
            .filter(|f| {
                f.belongs_to.is_some() || f.media.is_some() || f.rich_text || self.repeaters.contains_key(&f.name)
            })
            .filter_map(|f| record.get(&f.name).map(|v| (f.name.clone(), v.clone())))
            .collect();
        self.check_written(&wctx, &txn, &relations).await?;
        let before = self.audit_snapshot(&model);
        let mut active = model.into_active_model();
        self.set_marker(&mut active, false)?;
        let model = active.update(txn.as_ref()).await.map_err(|e| self.map_db_err(&wctx, e))?;
        // By default `after_restore` calls `after_save`, as restores always did.
        R::after_restore(&wctx, &txn, &model).await?;
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
        // A run on one record the action is hidden on (`visible_when`) is
        // refused (403); a run on several skips those records (`denied`).
        let single = keys.iter().collect::<HashSet<_>>().len() == 1;
        let first_pass = self.models_in_scope(&wctx, txn.as_ref(), keys).await?;
        for (model, record) in &first_pass {
            let key = self.key_of(model);
            seen.insert(key.clone());
            if !self.policy.allows(&wctx, action.ability, Some(record)).await {
                outcome.denied.push(key);
            } else if !action.shows_for(&wctx, model) {
                if single {
                    return Err(AppError::Forbidden);
                }
                outcome.denied.push(key);
            } else {
                candidates.push(key);
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
        // An action moving records (its form's `belongs_to` field of the locked
        // parent) locks the destination with them, in the same pass.
        let destination = self.meta.parent_lock.as_ref().and_then(|(field, target)| {
            action.form.iter().find(|f| f.name == *field && f.belongs_to == Some(*target))?;
            match data.get(field.as_str())? {
                serde_json::Value::String(key) => Some(key.clone()),
                serde_json::Value::Number(key) => Some(key.to_string()),
                _ => None,
            }
        });
        let parents =
            self.lock_parents(&wctx, &txn, read_parents.values().flatten().cloned().chain(destination)).await?;
        let locked = self.lock_rows(&txn, &candidates, Trashed::Without).await?;
        candidates.retain(|k| locked.contains(k));
        let mut allowed = Vec::new();
        for (model, record) in self.models_in_scope(&wctx, txn.as_ref(), &candidates).await? {
            let key = self.key_of(&model);
            if read_parents.get(&key).is_some_and(|parent| *parent != self.parent_of_model(&model)) {
                return Err(super::write::parent_moved());
            }
            seen.insert(key.clone());
            // Decided again on the locked row: it may have changed meanwhile.
            if !self.policy.allows(&wctx, action.ability, Some(&record)).await {
                outcome.denied.push(key);
            } else if !action.shows_for(&wctx, &model) {
                if single {
                    return Err(AppError::Forbidden);
                }
                outcome.denied.push(key);
            } else {
                outcome.processed.push(key);
                allowed.push(model);
            }
        }
        outcome.missing =
            keys.iter().filter(|k| !seen.contains(*k)).cloned().collect::<BTreeSet<_>>().into_iter().collect();
        if !allowed.is_empty() {
            let before: Vec<_> =
                allowed.iter().map(|m| (self.key_of(m), self.audit_snapshot(m), self.trashed(m))).collect();
            let processed: Vec<String> = allowed.iter().map(|m| self.key_of(m)).collect();
            let returned = (action.handler)(&wctx, txn.as_ref(), allowed, data).await?;
            // What the panel does next, checked before anything commits
            // (a page to open only for a run on one record).
            (outcome.message, outcome.open) = returned.effects(!single)?;
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
        let Some(title) = self.readable_title(ctx) else {
            return Ok(BTreeMap::new());
        };
        if !self.policy.allows(ctx, ability::VIEW_ANY, None).await {
            return Ok(BTreeMap::new());
        }
        let mut out = BTreeMap::new();
        for (model, record) in self.models_in_scope(ctx, &ctx.db, keys).await? {
            if self.policy.allows(ctx, ability::VIEW, Some(&record)).await {
                // No title (every field empty): the panel shows the key.
                if let Some(text) = title.render(&record) {
                    out.insert(self.key_of(&model), text);
                }
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

    fn encrypted_columns(&self) -> Vec<rustcms_core::crypto::EncryptedColumn> {
        ResourceAdapter::encrypted_columns(self)
    }

    fn import_key(&self) -> Option<&str> {
        self.meta.import_key.as_deref()
    }

    fn import_value(&self, name: &str, text: &str, quoted: bool) -> serde_json::Value {
        ResourceAdapter::import_value(self, name, text, quoted)
    }

    async fn add_counts(&self, ctx: &Ctx, envelopes: &mut [Envelope]) {
        ResourceAdapter::add_counts(self, ctx, envelopes).await
    }

    async fn counted_by(
        &self,
        ctx: &Ctx,
        field: &str,
        filters: &[(String, RelationFilter)],
        parents: &[String],
    ) -> AppResult<BTreeMap<String, u64>> {
        ResourceAdapter::counted_by(self, ctx, field, filters, parents).await
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
        self.locked_visible(ctx, txn, keys, &[]).await
    }

    async fn lock_visible_with(
        &self,
        ctx: &Ctx,
        txn: &DatabaseTransaction,
        keys: &[String],
        also: &[String],
    ) -> AppResult<HashSet<String>> {
        self.locked_visible(ctx, txn, keys, also).await
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

    async fn link_options_new(
        &self,
        ctx: &Ctx,
        relation: &str,
        search: Option<&str>,
        keys: &[String],
    ) -> AppResult<Vec<crate::links::LinkOption>> {
        ResourceAdapter::link_options_new(self, ctx, relation, search, keys).await
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

/// The filters of the relation a create goes through (`ctx.bound`, a
/// relation section's "New": `parent.relation`), ADR 0044. Rows of a
/// relationship repeater set theirs themselves.
fn bound_filters(ctx: &Ctx) -> Vec<(&'static str, RelationFilter)> {
    let Some((relation, _, _)) = &ctx.bound else { return Vec::new() };
    let Some((slug, name)) = relation.split_once('.') else { return Vec::new() };
    ctx.registry
        .get(slug)
        .and_then(|parent| parent.relations().iter().find(|r| r.name == name).map(|r| r.filters.clone()))
        .unwrap_or_default()
}
