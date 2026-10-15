//! The schema a user receives: inputs, table, relations, filtered by what
//! they may read and write.

use super::*;

impl<R: Resource> ResourceAdapter<R> {
    /// Relations the user may list, and whether they may create through them.
    pub(super) async fn relation_schemas(&self, ctx: &Ctx) -> Vec<crate::schema::RelationSchema> {
        let mut out = Vec::new();
        for relation in &self.relations {
            let Some(target) = ctx.registry.get(relation.target) else { continue };
            let (readable, creatable) = target.field_access(ctx, relation.field);
            if !readable || !target.allows(ctx, ability::VIEW_ANY).await {
                continue;
            }
            out.push(crate::schema::RelationSchema {
                name: relation.name,
                label: ctx.t_in(R::SLUG, &relation.label.clone().unwrap_or_else(|| humanize(relation.name))),
                resource: relation.target,
                field: relation.field,
                create: creatable && target.allows(ctx, ability::CREATE).await,
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
                    }
                    Component::Layout(layout) => {
                        out.extend(layout.label.clone());
                        walk(&layout.children, out, field_label);
                    }
                }
            }
        }
        let info = self.info();
        let mut out = vec![info.label, info.plural_label];
        let field_label = |name: &str| self.field(name).map(|f| f.display_label());
        walk(&self.form.components, &mut out, &field_label);
        for column in &self.table_ui.columns {
            out.extend(column.label.clone().or_else(|| field_label(&column.name)));
        }
        for filter in &self.table_ui.filters {
            out.extend(filter.label.clone().or_else(|| field_label(&filter.name)));
            out.extend(filter.options.iter().map(|o| o.label.clone()));
        }
        // Messages of declared unique constraints (translated when they fire).
        out.extend(self.meta.constraints.iter().map(|c| c.message.clone()));
        for action in &self.actions {
            out.push(action.label.clone().unwrap_or_else(|| humanize(action.name)));
            out.extend(action.confirm.clone());
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
            title_field: self.meta.title_field.clone(),
        }
    }

    pub(super) fn input_schema(&self, ctx: &Ctx, input: &Input, op: SchemaOperation) -> Option<InputSchema> {
        let field = self.field(&input.name)?;
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
        let write_only = !field.can_read(ctx);
        let mut rules = Vec::new();
        // Leaving a write-only input empty keeps the stored value: never required on edit.
        if field.required && !disabled && !(write_only && op == SchemaOperation::Edit) {
            rules.push(Rule::Required);
        }
        if let Some(&(items, length)) = self.key_values.get(&input.name).filter(|_| input.kind == InputType::KeyValue) {
            rules.push(Rule::MaxItems { value: items });
            rules.push(Rule::MaxLength { value: length });
        } else if let Some(max) = field.max_length {
            rules.push(Rule::MaxLength { value: max });
        }
        Some(InputSchema {
            kind: input.kind,
            name: input.name.clone(),
            label: ctx.t_in(R::SLUG, &input.label.clone().unwrap_or_else(|| field.display_label())),
            value_type: self.column(&input.name).kind.name(),
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
            options: translated_options(ctx, R::SLUG, &input.options),
            relationship: field.belongs_to.map(|resource| Relationship { resource }),
            widget: input.widget.clone(),
            props: input.props.clone(),
            media: field.media.as_ref().map(|rule| crate::schema::MediaSchema {
                multiple: rule.multiple,
                max_items: rule.max_items,
                accept: rule.accept.clone(),
                max_size: rule.max_size,
            }),
        })
    }

    pub(super) fn components(&self, ctx: &Ctx, components: &[Component], op: SchemaOperation) -> Vec<ComponentSchema> {
        components
            .iter()
            .filter_map(|component| match component {
                Component::Input(input) => self.input_schema(ctx, input, op).map(ComponentSchema::Input),
                Component::Layout(layout) => {
                    let children = self.components(ctx, &layout.children, op);
                    (!children.is_empty()).then(|| {
                        ComponentSchema::Layout(LayoutSchema {
                            kind: layout.kind,
                            id: layout.id.clone(),
                            label: layout.label.as_deref().map(|l| ctx.t_in(R::SLUG, l)),
                            columns: layout.columns,
                            components: children,
                        })
                    })
                }
            })
            .collect()
    }

    pub(super) async fn table_schema(&self, ctx: &Ctx) -> TableSchema {
        let readable = |name: &str| self.field(name).is_some_and(|f| f.can_read(ctx));
        let columns = self
            .table_ui
            .columns
            .iter()
            .filter(|c| readable(&c.name))
            .map(|c| {
                let field = self.field(&c.name).expect("validated at registration");
                ColumnSchema {
                    kind: c.kind,
                    name: c.name.clone(),
                    label: ctx.t_in(R::SLUG, &c.label.clone().unwrap_or_else(|| field.display_label())),
                    value_type: self.column(&c.name).kind.name(),
                    sortable: field.sortable,
                    searchable: field.searchable,
                    colors: c
                        .colors
                        .iter()
                        .map(|(value, color)| BadgeColor { value: value.clone(), color: color.clone() })
                        .collect(),
                    relationship: field.belongs_to.map(|resource| Relationship { resource }),
                    cell: c.cell.clone(),
                    props: c.props.clone(),
                }
            })
            .collect();
        let filters = self
            .table_ui
            .filters
            .iter()
            .filter(|f| readable(&f.name))
            .map(|f| {
                let field = self.field(&f.name).expect("validated at registration");
                FilterSchema {
                    kind: f.kind,
                    name: f.name.clone(),
                    label: ctx.t_in(R::SLUG, &f.label.clone().unwrap_or_else(|| field.display_label())),
                    value_type: self.column(&f.name).kind.name(),
                    options: translated_options(ctx, R::SLUG, &f.options),
                    relationship: field.belongs_to.map(|resource| Relationship { resource }),
                }
            })
            .collect();
        let mut actions = Vec::new();
        for action in &self.actions {
            // Listed unless an explicit admission permission excludes the user;
            // availability per record is in each envelope's `abilities`.
            if action.required_permission.is_none_or(|p| ctx.can(p)) {
                actions.push(ActionSchema {
                    name: action.name,
                    label: ctx.t_in(R::SLUG, &action.label.clone().unwrap_or_else(|| humanize(action.name))),
                    bulk: action.bulk,
                    confirm: action.confirm.as_deref().map(|c| ctx.t_in(R::SLUG, c)),
                    color: action.color.clone(),
                    icon: action.icon.clone(),
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
