//! The schema a user receives: inputs, table, relations, filtered by what
//! they may read and write.

use super::*;

impl<R: Resource> ResourceAdapter<R> {
    /// Relations the user may list, and what they may do through them on the
    /// viewed record (`record`, trusted).
    pub(super) async fn relation_schemas(&self, ctx: &Ctx, record: &Data) -> Vec<crate::schema::RelationSchema> {
        use crate::links::LinkOp;
        use crate::schema::{RelationKind, RelationSchema};
        let mut out = Vec::new();
        for relation in &self.relations {
            let Some(target) = ctx.registry.get(relation.target) else { continue };
            let label = ctx.t_in(R::SLUG, &relation.label.clone().unwrap_or_else(|| humanize(relation.name)));
            if !target.allows(ctx, ability::VIEW_ANY).await {
                continue;
            }
            let Some(pivot) = &relation.pivot else {
                let (readable, creatable) = target.field_access(ctx, relation.field);
                if !readable {
                    continue;
                }
                out.push(RelationSchema {
                    name: relation.name,
                    kind: RelationKind::HasMany,
                    label,
                    resource: relation.target,
                    field: relation.field,
                    create: creatable && target.allows(ctx, ability::CREATE).await,
                    // In canonical wire form, as the server writes them (checked at boot).
                    fixed: relation
                        .filters
                        .iter()
                        .map(|(f, v)| {
                            ((*f).to_owned(), target.relation_filter_value(f, v).unwrap_or_else(|_| v.value()))
                        })
                        .collect(),
                    attach: false,
                    detach: false,
                    pivot: Vec::new(),
                    attach_form: None,
                    pivot_form: None,
                });
                continue;
            };
            let attach = self.policy.allows(ctx, &LinkOp::Attach.ability(relation.name), Some(record)).await;
            let detach = self.policy.allows(ctx, &LinkOp::Detach.ability(relation.name), Some(record)).await;
            let update = !relation.pivot_fields.is_empty()
                && self.policy.allows(ctx, &LinkOp::UpdatePivot.ability(relation.name), Some(record)).await;
            let inputs = |op: Option<Operation>| -> Vec<ComponentSchema> {
                relation
                    .pivot_fields
                    .iter()
                    .filter(|f| match op {
                        None => f.can_read(ctx),
                        Some(op) => f.can_write(op, ctx),
                    })
                    .filter_map(|f| pivot.column(&f.name).map(|c| (f, c)))
                    .map(|(f, c)| ComponentSchema::Input(pivot_input_schema(ctx, R::SLUG, f, c, op.is_none())))
                    .collect()
            };
            let form = |components: Vec<ComponentSchema>| {
                (!components.is_empty()).then_some(FormSchema { columns: 1, components, width: Default::default() })
            };
            let readable = inputs(None)
                .into_iter()
                .filter_map(|c| match c {
                    ComponentSchema::Input(input) => Some(input),
                    ComponentSchema::Layout(_) | ComponentSchema::Entry(_) => None,
                })
                .collect();
            out.push(RelationSchema {
                name: relation.name,
                kind: RelationKind::BelongsToMany,
                label,
                resource: relation.target,
                field: relation.field,
                create: false,
                fixed: BTreeMap::new(),
                attach,
                detach,
                pivot: readable,
                attach_form: if attach { form(inputs(Some(Operation::Create))) } else { None },
                pivot_form: if update { form(inputs(Some(Operation::Update))) } else { None },
            });
        }
        out
    }

    /// The source strings [`ResourceAdapter::schema`] and the panel translate,
    /// read from the definitions (no policy, no hook, no record).
    pub(super) fn display_strings(&self) -> Vec<(bool, String)> {
        fn walk(components: &[Component], out: &mut Vec<String>, field_label: &dyn Fn(&str) -> Option<String>) {
            for component in components {
                match component {
                    Component::Input(input) => {
                        out.extend(input.label.clone().or_else(|| field_label(&input.name)));
                        out.extend(input.placeholder.clone());
                        out.extend(input.help.clone());
                        out.extend(input.options.iter().map(|o| o.label.clone()));
                        if let Some(spec) = input.repeater.as_deref() {
                            crate::engine::repeater::display_strings(spec, out);
                        }
                    }
                    Component::Layout(layout) => {
                        out.extend(layout.label.clone());
                        walk(&layout.children, out, field_label);
                    }
                    Component::Entry(entry) => {
                        // A computed value's label defaults to its name, humanized.
                        super::infolist::entry_strings(
                            entry,
                            || field_label(&entry.name).unwrap_or_else(|| humanize(&entry.name)),
                            out,
                        );
                    }
                }
            }
        }
        let info = self.info();
        let mut out = vec![info.label, info.plural_label];
        let field_label = |name: &str| {
            self.field(name).map(|f| f.display_label()).or_else(|| {
                // A `BelongsToManySelect` is labelled by its relation.
                self.relations
                    .iter()
                    .find(|r| r.name == name)
                    .map(|r| r.label.clone().unwrap_or_else(|| humanize(r.name)))
            })
        };
        walk(&self.form.components, &mut out, &field_label);
        if let Some(infolist) = &self.infolist {
            walk(&infolist.components, &mut out, &field_label);
        }
        for column in &self.table_ui.columns {
            let default = || match column.counts {
                Some(_) => Some(humanize(&column.name)),
                None => field_label(&column.name),
            };
            out.extend(column.label.clone().or_else(default));
            out.extend(column.options.iter().map(|o| o.label.clone()));
        }
        for filter in &self.table_ui.filters {
            out.extend(filter.label.clone().or_else(|| field_label(&filter.name)));
            out.extend(filter.options.iter().map(|o| o.label.clone()));
            out.extend(filter.none.clone());
        }
        // Messages of declared unique constraints (translated when they fire).
        out.extend(self.meta.constraints.iter().map(|c| c.message.clone()));
        // Labels of the rich text nodes the fields accept.
        for field in &self.fields {
            if let Some(accepts) = self.accepts(field) {
                out.extend(
                    field.rich_nodes.iter().filter_map(|n| accepts.nodes.get(n)).map(|n| n.label_text().to_owned()),
                );
            }
        }
        for action in &self.actions {
            out.push(action.label.clone().unwrap_or_else(|| humanize(action.name)));
            out.extend(action.confirm.clone());
            out.extend(action.submit_label.clone());
            for field in &action.form {
                out.push(field.display_label());
                out.extend(field.input.placeholder.clone());
                out.extend(field.input.help.clone());
                out.extend(field.input.options.iter().map(|o| o.label.clone()));
            }
        }
        for relation in self.relations.iter().filter(|r| r.is_many()) {
            out.extend(relation.pivot_fields.iter().map(|f| f.display_label()));
        }
        for relation in &self.relations {
            out.push(relation.label.clone().unwrap_or_else(|| humanize(relation.name)));
        }
        // Navigation groups are translated globally (they are shared by resources).
        let mut out: Vec<(bool, String)> = out.into_iter().map(|s| (true, s)).collect();
        out.extend(info.group.map(|g| (false, g)));
        out.sort();
        out.dedup();
        out
    }

    pub(super) fn info(&self) -> ResourceInfo {
        ResourceInfo {
            slug: R::SLUG,
            label: self.meta.label.clone(),
            plural_label: self.meta.plural_label.clone().unwrap_or_else(|| format!("{}s", self.meta.label)),
            icon: self.meta.icon.clone(),
            group: self.meta.group.clone(),
            title_field: self.title.as_ref().and_then(|t| t.single_field()).map(str::to_owned),
            // Listed only for a title of several fields (`title_field` holds a single one).
            title_fields: self
                .title
                .as_ref()
                .filter(|t| t.single_field().is_none())
                .map(|t| {
                    let mut fields: Vec<String> = Vec::new();
                    for field in t.fields() {
                        if !fields.iter().any(|f| f == field) {
                            fields.push(field.to_owned());
                        }
                    }
                    fields
                })
                .unwrap_or_default(),
            sort: (self.meta.sort != 0).then_some(self.meta.sort),
            nav: (!self.meta.navigation).then_some(false),
        }
    }

    /// The options of select `name`: declared, or the panel's media
    /// collections for the media library's `collection`.
    fn options_of(
        &self,
        ctx: &Ctx,
        name: &str,
        declared: &[crate::forms::SelectOption],
    ) -> Vec<crate::forms::SelectOption> {
        if self.is_media_collection(name) {
            return crate::media::collection_options(ctx);
        }
        translated_options(ctx, R::SLUG, declared)
    }

    /// Whether `name` is the media library's `collection`.
    fn is_media_collection(&self, name: &str) -> bool {
        std::any::TypeId::of::<R>() == std::any::TypeId::of::<crate::media::MediaResource>() && name == "collection"
    }

    /// The media library's `collection` is shown only when the panel declares collections.
    fn shown_here(&self, ctx: &Ctx, name: &str) -> bool {
        !self.is_media_collection(name) || !ctx.registry.media_collections().is_empty()
    }

    pub(super) fn input_schema(&self, ctx: &Ctx, input: &Input, op: SchemaOperation) -> Option<InputSchema> {
        let field = self.field(&input.name)?;
        if !self.shown_here(ctx, &input.name) {
            return None;
        }
        let is_key = input.name == self.pk_name;
        let (include, disabled) = match op {
            SchemaOperation::Create => {
                let natural = is_key && matches!(self.meta.key, KeyStrategy::Natural);
                (field.can_write(Operation::Create, ctx) && (!is_key || natural), false)
            }
            SchemaOperation::Edit => {
                // Write-only fields (e.g. a password) stay editable without showing a value.
                let writable = !is_key && field.can_write(Operation::Update, ctx);
                (field.can_read(ctx) || writable, !writable)
            }
            SchemaOperation::View => (field.can_read(ctx), true),
        };
        if !include {
            return None;
        }
        // Encrypted fields are always write-only: their record value is a
        // masked view, never the secret.
        let write_only = !field.can_read(ctx) || field.secret.is_some();
        let mut rules = Vec::new();
        // Leaving a write-only input empty keeps the stored value: never required on edit.
        if field.required && !disabled && !(write_only && op == SchemaOperation::Edit) {
            rules.push(Rule::Required);
        }
        if field.secret == Some(crate::field::SecretKind::Map) {
            let (items, length) = self.secret_bounds(&input.name);
            rules.push(Rule::MaxItems { value: items });
            rules.push(Rule::MaxLength { value: length });
        } else if let Some(&(items, length)) =
            self.key_values.get(&input.name).filter(|_| input.kind == InputType::KeyValue)
        {
            rules.push(Rule::MaxItems { value: items });
            rules.push(Rule::MaxLength { value: length });
        } else if let Some(max) = field.max_length {
            rules.push(Rule::MaxLength { value: max });
        }
        Some(InputSchema {
            kind: input.kind,
            name: input.name.clone(),
            label: ctx.t_in(R::SLUG, &input.label.clone().unwrap_or_else(|| field.display_label())),
            value_type: match field.secret {
                Some(crate::field::SecretKind::Text) => "secret",
                Some(crate::field::SecretKind::Map) => "secret_map",
                None => self.column(&input.name).kind.name(),
            },
            nullable: self.column(&input.name).nullable,
            // A default is data: only for fields the user may read.
            default: (op == SchemaOperation::Create && !write_only)
                .then(|| self.column(&input.name).default.clone())
                .flatten(),
            disabled,
            write_only,
            rules,
            column_span: input.column_span,
            placeholder: input.placeholder.as_deref().map(|p| ctx.t_in(R::SLUG, p)),
            help: input.help.as_deref().map(|h| ctx.t_in(R::SLUG, h)),
            mode: input.mode,
            options: self.options_of(ctx, &input.name, &input.options),
            relationship: field.belongs_to.map(|resource| Relationship { resource }),
            widget: input.widget.clone(),
            props: input.props.clone(),
            media: field.media.as_ref().map(|rule| crate::schema::MediaSchema {
                multiple: rule.multiple,
                max_items: rule.max_items,
                accept: rule.accept.clone(),
                max_size: rule.max_size,
                preview: input.preview,
            }),
            // Where its uploads go and its picker opens: for inputs the user may change.
            collection: field.collection.clone().filter(|_| !disabled),
            links: None,
            // Shown as written (`/posts/`, `.html`): not translated.
            prefix: input.prefix.clone(),
            suffix: input.suffix.clone(),
            number: input.number.as_deref().copied(),
            shown_when: input.shown_when.as_deref().cloned(),
            clear_when_hidden: input.clear_when_hidden,
            integer: false,
            repeater: self.repeater_schema(ctx, input, disabled),
            rich_nodes: self.rich_node_schemas(ctx, &input.name),
            // Never on view pages, nor for a user who cannot change the input (ADR 0043).
            reacts: !disabled && op != SchemaOperation::View && input.reaction.is_some(),
        })
    }

    /// The application nodes rich text field `name` accepts (ADR 0042).
    pub(super) fn rich_node_schemas(&self, ctx: &Ctx, name: &str) -> Vec<crate::contract::RichNodeSchema> {
        use crate::rich_text::AttrKind;
        let Some(accepts) = self.field(name).and_then(|f| self.accepts(f)) else { return Vec::new() };
        let names = accepts.only.unwrap_or_default();
        names
            .iter()
            .filter_map(|n| accepts.nodes.get(n))
            .map(|node| crate::contract::RichNodeSchema {
                name: node.name().to_owned(),
                label: ctx.t_in(R::SLUG, node.label_text()),
                inline: node.is_inline(),
                attrs: node
                    .attrs()
                    .map(|(name, attr)| {
                        let (kind, min, max, options) = match attr.kind() {
                            AttrKind::Text { min, max } => ("text", Some(*min as i64), Some(*max as i64), Vec::new()),
                            AttrKind::Integer { min, max } => ("integer", Some(*min), Some(*max), Vec::new()),
                            AttrKind::Boolean => ("boolean", None, None, Vec::new()),
                            AttrKind::OneOf(options) => ("one_of", None, None, options.clone()),
                            AttrKind::Uuid => ("uuid", None, None, Vec::new()),
                        };
                        crate::contract::RichAttrSchema {
                            name: name.to_owned(),
                            label: attr.label_text().map_or_else(|| name.to_owned(), |l| ctx.t_in(R::SLUG, l)),
                            kind,
                            required: attr.default_value().is_none(),
                            default: attr.default_value().cloned(),
                            min,
                            max,
                            options,
                        }
                    })
                    .collect(),
            })
            .collect()
    }

    /// The rows of a repeater input (JSON: as declared; relationship rows come
    /// with ADR 0040's second part).
    fn repeater_schema(&self, ctx: &Ctx, input: &Input, disabled: bool) -> Option<Box<crate::schema::RepeaterSchema>> {
        let spec = input.repeater.as_deref()?;
        let t = |text: &str| ctx.t_in(R::SLUG, text);
        Some(Box::new(crate::engine::repeater::rows_schema(spec, &t, disabled)))
    }

    /// The input of a relationship repeater, before its rows are filled in.
    fn row_repeater_input(&self, ctx: &Ctx, input: &Input) -> InputSchema {
        let relation = self.relations.iter().find(|r| r.name == input.name);
        let label = input
            .label
            .clone()
            .or_else(|| relation.and_then(|r| r.label.clone()))
            .unwrap_or_else(|| humanize(&input.name));
        InputSchema {
            kind: InputType::Repeater,
            name: input.name.clone(),
            label: ctx.t_in(R::SLUG, &label),
            value_type: "rows",
            nullable: false,
            default: None,
            disabled: false,
            write_only: false,
            rules: Vec::new(),
            column_span: input.column_span,
            placeholder: None,
            help: input.help.as_deref().map(|h| ctx.t_in(R::SLUG, h)),
            mode: None,
            options: Vec::new(),
            relationship: None,
            widget: None,
            props: None,
            media: None,
            collection: None,
            links: None,
            prefix: None,
            suffix: None,
            number: None,
            // Hidden by its condition, a write removes its rows (ADR 0044).
            shown_when: input.shown_when.as_deref().cloned(),
            clear_when_hidden: false,
            integer: false,
            repeater: None,
            rich_nodes: Vec::new(),
            reacts: false,
        }
    }

    /// Fills in the rows of the relationship repeaters in `components`: the
    /// child's inputs for the operation and what the user may do; a repeater
    /// whose child the user may not list is left out.
    pub(super) async fn fill_row_repeaters(
        &self,
        ctx: &Ctx,
        components: Vec<ComponentSchema>,
        op: SchemaOperation,
    ) -> Vec<ComponentSchema> {
        let mut out = Vec::with_capacity(components.len());
        for component in components {
            match component {
                ComponentSchema::Input(mut input) if input.value_type == "rows" => {
                    let Some(repeater) = self.row_repeaters.iter().find(|r| r.name == input.name) else { continue };
                    let Some(child) = ctx.registry.get(repeater.target) else { continue };
                    if !child.allows(ctx, ability::VIEW_ANY).await {
                        continue;
                    }
                    let create = child.admits(ctx, ability::CREATE).await && child.allows(ctx, ability::CREATE).await;
                    let update = op == SchemaOperation::Edit && child.admits(ctx, ability::UPDATE).await;
                    let delete = op == SchemaOperation::Edit && child.admits(ctx, ability::DELETE).await;
                    let rows = child.row_schema(ctx, &repeater.rules().components, op);
                    let t = |text: &str| ctx.t_in(R::SLUG, text);
                    let relationship = crate::schema::RepeaterRelationship {
                        resource: repeater.target,
                        relation: repeater.name,
                        create,
                        update,
                        delete,
                        order_column: repeater.order_column.clone(),
                    };
                    input.disabled = !(create || update || delete);
                    let mut schema =
                        crate::engine::repeater::options_schema(&repeater.spec, rows, &t, Some(relationship));
                    // Empty rows to start with are for new records only.
                    if op != SchemaOperation::Create {
                        schema.default_items = None;
                    }
                    // Without an order column an order is never stored: no reordering offered.
                    schema.reorderable = repeater.order_column.is_some();
                    // On an edit page, new rows are the child's creates: its create form's inputs.
                    if op == SchemaOperation::Edit && create {
                        schema.create_components =
                            Some(child.row_schema(ctx, &repeater.rules().components, SchemaOperation::Create));
                    }
                    input.repeater = Some(Box::new(schema));
                    out.push(ComponentSchema::Input(input));
                }
                ComponentSchema::Layout(mut layout) => {
                    layout.components = Box::pin(self.fill_row_repeaters(ctx, layout.components, op)).await;
                    if !layout.components.is_empty() {
                        out.push(ComponentSchema::Layout(layout));
                    }
                }
                other => out.push(other),
            }
        }
        out
    }

    /// What the user may change with each `BelongsToManySelect` of the form
    /// (ADR 0035): on create, attach (admitted and allowed without a record);
    /// on edit, attach or detach on `record`; never on view (the relation
    /// manager shows the links). Inputs the user can change nothing with, or
    /// whose target they may not list, are left out.
    pub(super) async fn link_inputs(
        &self,
        ctx: &Ctx,
        op: SchemaOperation,
        record: Option<&Data>,
    ) -> HashMap<String, crate::schema::LinksSchema> {
        use crate::links::LinkOp;
        let mut out = HashMap::new();
        for input in self.form.inputs().into_iter().filter(|i| i.kind == InputType::BelongsToMany) {
            let Some(relation) = self.relations.iter().find(|r| r.name == input.name) else { continue };
            let Some(target) = ctx.registry.get(relation.target) else { continue };
            if !target.allows(ctx, ability::VIEW_ANY).await {
                continue;
            }
            let (attach, detach) = (LinkOp::Attach.ability(relation.name), LinkOp::Detach.ability(relation.name));
            let links = match (op, record) {
                (SchemaOperation::Create, _) => crate::schema::LinksSchema {
                    attach: self.policy.admits(ctx, &attach).await && self.policy.allows(ctx, &attach, None).await,
                    detach: false,
                },
                (SchemaOperation::Edit, Some(record)) => crate::schema::LinksSchema {
                    attach: self.policy.allows(ctx, &attach, Some(record)).await,
                    detach: self.policy.allows(ctx, &detach, Some(record)).await,
                },
                _ => continue,
            };
            if links.attach || links.detach {
                out.insert(input.name.clone(), links);
            }
        }
        out
    }

    /// The input of a `BelongsToManySelect` (its abilities from [`Self::link_inputs`]).
    fn links_input_schema(&self, ctx: &Ctx, input: &Input, links: crate::schema::LinksSchema) -> Option<InputSchema> {
        let relation = self.relations.iter().find(|r| r.name == input.name)?;
        let label = input.label.clone().or_else(|| relation.label.clone()).unwrap_or_else(|| humanize(relation.name));
        Some(InputSchema {
            kind: InputType::BelongsToMany,
            name: input.name.clone(),
            label: ctx.t_in(R::SLUG, &label),
            value_type: "links",
            nullable: false,
            default: None,
            disabled: false,
            write_only: false,
            rules: Vec::new(),
            column_span: input.column_span,
            placeholder: input.placeholder.as_deref().map(|p| ctx.t_in(R::SLUG, p)),
            help: input.help.as_deref().map(|h| ctx.t_in(R::SLUG, h)),
            mode: None,
            options: Vec::new(),
            relationship: Some(Relationship { resource: relation.target }),
            widget: None,
            props: None,
            media: None,
            collection: None,
            links: Some(links),
            prefix: None,
            suffix: None,
            number: None,
            shown_when: None,
            clear_when_hidden: false,
            integer: false,
            repeater: None,
            rich_nodes: Vec::new(),
            reacts: false,
        })
    }

    pub(super) fn components(
        &self,
        ctx: &Ctx,
        components: &[Component],
        op: SchemaOperation,
        links: &HashMap<String, crate::schema::LinksSchema>,
    ) -> Vec<ComponentSchema> {
        components
            .iter()
            .filter_map(|component| match component {
                Component::Input(input) if input.kind == InputType::BelongsToMany => links
                    .get(&input.name)
                    .and_then(|l| self.links_input_schema(ctx, input, *l))
                    .map(ComponentSchema::Input),
                // Relationship repeaters: their rows come from the child (`fill_row_repeaters`); never on view pages.
                Component::Input(input) if input.repeater.as_deref().is_some_and(|s| s.relationship) => {
                    (op != SchemaOperation::View).then(|| ComponentSchema::Input(self.row_repeater_input(ctx, input)))
                }
                Component::Input(input) => self.input_schema(ctx, input, op).map(ComponentSchema::Input),
                // Forms hold no entries (refused at boot).
                Component::Entry(_) => None,
                Component::Layout(layout) => {
                    let children = self.components(ctx, &layout.children, op, links);
                    (!children.is_empty())
                        .then(|| ComponentSchema::Layout(layout_schema(layout, children, |l| ctx.t_in(R::SLUG, l))))
                }
            })
            .collect()
    }

    pub(super) async fn table_schema(&self, ctx: &Ctx) -> TableSchema {
        let readable = |name: &str| self.field(name).is_some_and(|f| f.can_read(ctx));
        let mut columns = Vec::new();
        for c in &self.table_ui.columns {
            let Some(name) = c.counts else {
                if readable(&c.name) && self.shown_here(ctx, &c.name) {
                    columns.push(c);
                }
                continue;
            };
            // As the relation's list: listable, and its parent field readable.
            let relation = self.relations.iter().find(|r| r.name == name).expect("validated at registration");
            if let Some(target) = ctx.registry.get(relation.target)
                && target.allows(ctx, ability::VIEW_ANY).await
                && target.field_access(ctx, relation.field).0
            {
                columns.push(c);
            }
        }
        let columns = columns
            .into_iter()
            .map(|c| {
                if c.counts.is_some() {
                    return ColumnSchema {
                        kind: c.kind,
                        name: c.name.clone(),
                        label: ctx.t_in(R::SLUG, &c.label.clone().unwrap_or_else(|| humanize(&c.name))),
                        value_type: FieldKind::I32.name(),
                        sortable: false,
                        searchable: false,
                        colors: Vec::new(),
                        options: Vec::new(),
                        format: None,
                        relationship: None,
                        cell: None,
                        props: c.props.clone(),
                        hide_below: c.hide_below,
                        count: false,
                        counts: c.counts,
                    };
                }
                let field = self.field(&c.name).expect("validated at registration");
                ColumnSchema {
                    kind: c.kind,
                    name: c.name.clone(),
                    label: ctx.t_in(R::SLUG, &c.label.clone().unwrap_or_else(|| field.display_label())),
                    value_type: self.column(&c.name).kind.name(),
                    // A count column shows a number: sorting or searching its JSON text would mislead.
                    sortable: field.sortable && !c.count,
                    searchable: field.searchable && !c.count,
                    colors: c
                        .colors
                        .iter()
                        .map(|(value, color)| BadgeColor { value: value.clone(), color: color.clone() })
                        .collect(),
                    options: self.options_of(ctx, &c.name, &c.options),
                    format: c.date_format.clone(),
                    relationship: field.belongs_to.map(|resource| Relationship { resource }),
                    cell: c.cell.clone(),
                    props: c.props.clone(),
                    hide_below: c.hide_below,
                    count: c.count,
                    counts: None,
                }
            })
            .collect();
        let filters = self
            .table_ui
            .filters
            .iter()
            .filter(|f| readable(&f.name) && self.shown_here(ctx, &f.name))
            .map(|f| {
                let field = self.field(&f.name).expect("validated at registration");
                FilterSchema {
                    kind: f.kind,
                    name: f.name.clone(),
                    label: ctx.t_in(R::SLUG, &f.label.clone().unwrap_or_else(|| field.display_label())),
                    value_type: self.column(&f.name).kind.name(),
                    options: self.options_of(ctx, &f.name, &f.options),
                    relationship: field.belongs_to.map(|resource| Relationship { resource }),
                    none: f.none.as_deref().map(|label| ctx.t_in(R::SLUG, label)),
                }
            })
            .collect();
        let mut actions = Vec::new();
        for action in &self.actions {
            // Listed unless an explicit admission permission (or the policy's
            // admission) excludes the user; availability per record is in each
            // envelope's `abilities`.
            if action.required_permission.is_none_or(|p| ctx.can(p)) && self.policy.admits(ctx, action.ability).await {
                actions.push(ActionSchema {
                    name: action.name,
                    label: ctx.t_in(R::SLUG, &action.label.clone().unwrap_or_else(|| humanize(action.name))),
                    bulk: action.bulk,
                    confirm: action.confirm.as_deref().map(|c| ctx.t_in(R::SLUG, c)),
                    color: action.color.clone(),
                    icon: action.icon.clone(),
                    form: self.action_form(ctx, action),
                    primary: action.primary,
                    submit_label: action.submit_label.as_deref().map(|l| ctx.t_in(R::SLUG, l)),
                });
            }
        }
        TableSchema {
            columns,
            filters,
            default_sort: self.table_ui.default_sort.clone().filter(|s| readable(s.trim_start_matches('-'))),
            per_page_options: PER_PAGE_OPTIONS.to_vec(),
            searchable: self.fields.iter().any(|f| f.searchable && f.can_read(ctx)),
            actions,
            soft_deletes: self.soft.is_some(),
        }
    }
}

/// The schema of `layout` holding `children`, its label translated by `t`.
pub(super) fn layout_schema(
    layout: &Layout,
    children: Vec<ComponentSchema>,
    t: impl Fn(&str) -> String,
) -> LayoutSchema {
    LayoutSchema {
        kind: layout.kind,
        id: layout.id.clone(),
        label: layout.label.as_deref().map(t),
        columns: layout.columns,
        components: children,
        collapsed: layout.collapsed,
        compact: layout.compact,
        aside_first: layout.aside_first,
        shown_when: layout.shown_when.as_deref().cloned(),
    }
}

/// Option labels in the request's language (values unchanged).
fn translated_options(
    ctx: &Ctx,
    scope: &str,
    options: &[crate::forms::SelectOption],
) -> Vec<crate::forms::SelectOption> {
    options
        .iter()
        .map(|o| crate::forms::SelectOption { value: o.value.clone(), label: ctx.t_in(scope, &o.label) })
        .collect()
}

/// The input of a pivot column (`read_only`: shown with each link).
fn pivot_input_schema(
    ctx: &Ctx,
    slug: &str,
    field: &Field,
    column: crate::adapter::pivot::PivotColumn,
    read_only: bool,
) -> InputSchema {
    let kind = match column.kind {
        FieldKind::I32 | FieldKind::I64 | FieldKind::F64 | FieldKind::Decimal => InputType::NumberInput,
        FieldKind::Bool => InputType::Toggle,
        FieldKind::Date => InputType::DatePicker,
        FieldKind::LocalDateTime | FieldKind::Instant => InputType::DateTimePicker,
        FieldKind::Json => InputType::Textarea,
        FieldKind::String | FieldKind::Uuid => InputType::TextInput,
    };
    let mut rules = Vec::new();
    if field.required && !read_only {
        rules.push(Rule::Required);
    }
    if let Some(max) = field.max_length {
        rules.push(Rule::MaxLength { value: max });
    }
    InputSchema {
        kind,
        name: field.name.clone(),
        label: ctx.t_in(slug, &field.display_label()),
        value_type: column.kind.name(),
        nullable: column.nullable,
        // Pivot defaults stay in the database.
        default: None,
        disabled: read_only,
        write_only: !read_only && !field.can_read(ctx),
        rules,
        column_span: 1,
        placeholder: None,
        help: None,
        mode: None,
        options: Vec::new(),
        relationship: None,
        widget: None,
        props: None,
        media: None,
        collection: None,
        links: None,
        prefix: None,
        suffix: None,
        number: None,
        shown_when: None,
        clear_when_hidden: false,
        integer: false,
        repeater: None,
        rich_nodes: Vec::new(),
        reacts: false,
    }
}
