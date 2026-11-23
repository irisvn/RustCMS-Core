//! The view page's infolist (ADR 0039): boot checks, the schema a user
//! receives, and computed values.

use super::schema::layout_schema;
use super::*;
use crate::schema::{ComputedValues, EntryRows, EntrySchema, InfolistSchema};
use crate::ui::{Computed, Entry, EntryType, Infolist};

/// View pages computing their values at once, across users.
static COMPUTING: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(8);
/// Longest wait for a turn before answering 429.
const COMPUTING_WAIT: std::time::Duration = std::time::Duration::from_secs(10);
/// View pages one user may compute at once (across resources): one user
/// cannot hold every turn of [`COMPUTING`].
const COMPUTING_PER_USER: usize = 2;

/// Pages computing per user; an entry lives while one of its turns is held.
static COMPUTING_USERS: std::sync::Mutex<Option<HashMap<String, usize>>> = std::sync::Mutex::new(None);

/// One of a user's [`COMPUTING_PER_USER`] turns, given back when dropped
/// (also when the request is cancelled).
struct UserTurn(String);

impl UserTurn {
    /// A turn for `user`, or `None` when the user already computes as many
    /// pages as allowed.
    fn take(user: &str) -> Option<Self> {
        let mut users = COMPUTING_USERS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let count = users.get_or_insert_with(HashMap::new).entry(user.to_owned()).or_default();
        if *count >= COMPUTING_PER_USER {
            return None;
        }
        *count += 1;
        Some(Self(user.to_owned()))
    }
}

impl Drop for UserTurn {
    fn drop(&mut self) {
        let mut users = COMPUTING_USERS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(users) = users.as_mut()
            && let Some(count) = users.get_mut(&self.0)
        {
            *count -= 1;
            if *count == 0 {
                users.remove(&self.0);
            }
        }
    }
}

/// Largest computed value sent, serialized.
const COMPUTED_MAX_BYTES: usize = 64 * 1024;

/// Checks the infolist of resource `slug` and its computed values; panics on
/// what cannot hold (ADR 0039, boot checks).
pub(super) fn check_infolist<C: ColumnTrait, E: EntityTrait>(
    slug: &str,
    infolist: &Infolist,
    computed: &[Computed<E>],
    fields: &[Field],
    columns: &HashMap<String, ColumnInfo<C>>,
    relations: &[Relation],
) {
    let owner = format!("resource `{slug}`");
    let mut names = HashSet::new();
    for value in computed {
        let name = value.name.as_str();
        assert!(names.insert(name), "{owner}: computed value `{name}` declared twice");
        assert!(!fields.iter().any(|f| f.name == name), "{owner}: computed value `{name}` has the name of a field");
        assert!(
            !value.timeout.is_zero() && value.timeout <= crate::ui::COMPUTED_MAX_TIMEOUT,
            "{owner}: computed value `{name}`: the timeout must be above 0 and at most 30 seconds"
        );
    }
    if infolist.is_empty() {
        return;
    }
    check_layouts(&owner, &infolist.components, None);
    assert!(
        !holds_inputs(&infolist.components),
        "{owner}: an infolist holds entries, not inputs (inputs go in `form()`)"
    );
    let mut seen = HashSet::new();
    for entry in infolist.entries() {
        let name = entry.name.as_str();
        assert!(seen.insert(name), "{owner}: entry `{name}` appears twice in the infolist");
        assert!(
            !entry.relationship || entry.kind == EntryType::RepeatableEntry,
            "{owner}: entry `{name}`: `relationship()` is for repeatable entries"
        );
        if entry.relationship {
            // The rows are a has-many relation's records; their entries are
            // checked with the child's fields (`check_relation_rows`).
            assert!(
                relations.iter().any(|r| r.name == name && !r.is_many()),
                "{owner}: repeatable entry `{name}`: `relationship()` needs a has-many relation named `{name}`"
            );
            check_relationship_options(&owner, entry);
            continue;
        }
        let field = fields.iter().find(|f| f.name == name);
        let is_computed = names.contains(name);
        match entry.kind {
            EntryType::ComputedEntry => {
                assert!(is_computed, "{owner}: computed entry `{name}` names no value of `computed()`")
            }
            EntryType::CustomEntry => assert!(
                field.is_some() || is_computed,
                "{owner}: custom entry `{name}` names no field and no computed value"
            ),
            _ => assert!(field.is_some(), "{owner}: entry `{name}` is not a field"),
        }
        if let Some(field) = field {
            assert!(field.secret.is_none(), "{owner}: encrypted field `{name}` cannot have an entry");
        }
        let kind = field.map(|f| columns[&f.name].kind);
        let fits = entry_fits(entry.kind, field, kind);
        assert!(fits, "{owner}: entry `{name}`: a {} does not fit this field", entry_label(entry.kind));
        check_options(&owner, entry, kind);
        if entry.kind == EntryType::RepeatableEntry {
            check_row_entries(&owner, entry, 1);
        }
    }
    // Conditions name entries of the infolist, with the rules of forms (ADR 0037).
    let kind_of = |name: &str| {
        let field = fields.iter().find(|f| f.name == name && f.secret.is_none())?;
        let column = columns.get(&field.name)?;
        Some((column.kind, crate::wire::decimal_scale(column.column.def().get_column_type())))
    };
    crate::engine::conditions::FormConditions::new(&format!("{owner} (infolist)"), &infolist.components, &kind_of);
}

/// Whether an entry of kind `kind` shows `field` (`kind`: its column's).
fn entry_fits(entry: EntryType, field: Option<&Field>, kind: Option<FieldKind>) -> bool {
    match entry {
        EntryType::RelationEntry => field.is_some_and(|f| f.belongs_to.is_some()),
        EntryType::MediaEntry => field.is_some_and(|f| f.media.is_some()),
        EntryType::RichTextEntry => field.is_some_and(|f| f.rich_text),
        EntryType::KeyValueEntry | EntryType::RepeatableEntry => {
            kind == Some(FieldKind::Json) && field.is_some_and(|f| !f.rich_text && f.media.is_none())
        }
        EntryType::BooleanEntry => kind == Some(FieldKind::Bool),
        EntryType::TextEntry | EntryType::ComputedEntry | EntryType::CustomEntry => true,
    }
}

/// Most rows a relationship repeatable entry shows (ADR 0044).
pub(crate) const MAX_ENTRY_ROWS: u32 = 100;

/// The options of a relationship repeatable entry: rows (`limit`, at most
/// 100) and the entries of each row; the entries themselves are checked
/// with the child's fields once every resource is registered.
fn check_relationship_options(owner: &str, entry: &Entry) {
    let here = format!("{owner}: repeatable entry `{}`", entry.name);
    assert!(!entry.components.is_empty(), "{here}: the row has no entry (`.schema(components![…])`)");
    check_layouts(&here, &entry.components, None);
    assert!(!holds_inputs(&entry.components), "{here}: a row holds entries, not inputs");
    assert!(
        entry.display.limit.is_none_or(|n| n <= MAX_ENTRY_ROWS),
        "{here}: `limit` shows at most {MAX_ENTRY_ROWS} rows (the others are in the relation's section)"
    );
}

/// Checks the entries of the rows of a relationship repeatable entry with
/// `fields` and `columns`, the child's (ADR 0044): each a readable field of
/// the child that its entry fits, once; no nested relationship rows.
pub(super) fn check_relation_rows<C: ColumnTrait>(
    here: &str,
    components: &[Component],
    fields: &[Field],
    columns: &HashMap<String, ColumnInfo<C>>,
) {
    let rows = Infolist::new().schema(components.to_vec());
    let mut seen = HashSet::new();
    for entry in rows.entries() {
        let name = entry.name.as_str();
        assert!(seen.insert(name), "{here}: entry `{name}` appears twice in a row");
        assert!(!entry.relationship, "{here}: entry `{name}`: rows of rows come later (ADR 0044, part C)");
        let field = fields.iter().find(|f| f.name == name && f.secret.is_none());
        assert!(
            field.is_some() && !matches!(entry.kind, EntryType::ComputedEntry),
            "{here}: entry `{name}` is not a field of the related resource (encrypted fields have no entry)"
        );
        let kind = field.map(|f| columns[&f.name].kind);
        assert!(
            entry_fits(entry.kind, field, kind),
            "{here}: entry `{name}`: a {} does not fit this field",
            entry_label(entry.kind)
        );
        check_options(here, entry, kind);
        if entry.kind == EntryType::RepeatableEntry {
            check_row_entries(here, entry, 2);
        }
    }
    let kind_of = |name: &str| {
        let field = fields.iter().find(|f| f.name == name && f.secret.is_none())?;
        let column = columns.get(&field.name)?;
        Some((column.kind, crate::wire::decimal_scale(column.column.def().get_column_type())))
    };
    crate::engine::conditions::FormConditions::new(here, components, &kind_of);
}

/// The view page of a resource without `infolist()`: its form shown read-only,
/// each input as the entry that draws its value as the form's read-only view
/// always has. Inputs that show nothing there (hidden,
/// belongs-to-many, secrets, relationship repeaters) are left out; an entry
/// that would not fit its field falls back to text, so no form fails at boot.
/// Not checked as a declared infolist: the form's own checks hold (a field
/// may appear twice, a condition may name a hidden input).
pub(super) fn from_form<C: ColumnTrait>(
    form: &Form,
    fields: &[Field],
    columns: &HashMap<String, ColumnInfo<C>>,
) -> Infolist {
    let components = entries_of(&form.components, &|input| {
        // A relationship repeater names a relation: its rows' entries name the child's fields.
        if input.repeater.as_deref().is_some_and(|spec| spec.relationship) {
            return form_entry(input, None);
        }
        let field = fields.iter().find(|f| f.name == input.name && f.secret.is_none())?;
        form_entry(input, Some((field, columns.get(&field.name)?.kind)))
    });
    Infolist { columns: form.columns, components, width: None, from_form: true }
}

/// The entry showing `input` read-only: of a field (`field`, with its column
/// kind), or of a repeater row (`None`: the row's value, of the input's kind).
fn form_entry(input: &Input, field: Option<(&Field, FieldKind)>) -> Option<Entry> {
    use crate::engine::repeater::row_kind;
    let json = |f: &Field, kind: FieldKind| kind == FieldKind::Json && !f.rich_text && f.media.is_none();
    let kind = match (input.kind, field) {
        (InputType::Hidden | InputType::BelongsToMany | InputType::Secret | InputType::SecretMap, _) => return None,
        // A relationship repeater: its rows, as the relation's section reads them (ADR 0044).
        (InputType::Repeater, _) if input.repeater.as_deref().is_some_and(|spec| spec.relationship) => {
            let spec = input.repeater.as_deref()?;
            let mut entry = Entry::new(input.name.clone(), EntryType::RepeatableEntry);
            entry.label = input.label.clone();
            entry.column_span = input.column_span;
            entry.shown_when = input.shown_when.clone();
            entry.relationship = true;
            entry.components = entries_of(&spec.components, &|input| form_entry(input, None));
            return Some(entry);
        }
        (InputType::Repeater, _) if input.repeater.is_none() => return None,
        (InputType::Repeater, Some((f, kind))) if !json(f, kind) => EntryType::TextEntry,
        (InputType::Repeater, _) => EntryType::RepeatableEntry,
        (InputType::Select, Some((f, _))) if f.belongs_to.is_some() => EntryType::RelationEntry,
        (InputType::Toggle | InputType::Checkbox, Some((_, FieldKind::Bool)) | None) => EntryType::BooleanEntry,
        (InputType::KeyValue, Some((f, kind))) if json(f, kind) => EntryType::KeyValueEntry,
        (InputType::KeyValue, None) => EntryType::KeyValueEntry,
        (InputType::FileUpload, Some((f, _))) if f.media.is_some() => EntryType::MediaEntry,
        (InputType::RichEditor, Some((f, _))) if f.rich_text => EntryType::RichTextEntry,
        // A JSON row's files and documents (ADR 0044).
        (InputType::FileUpload, None) => EntryType::MediaEntry,
        (InputType::RichEditor, None) => EntryType::RichTextEntry,
        _ => EntryType::TextEntry,
    };
    let mut entry = Entry::new(input.name.clone(), kind);
    entry.label = input.label.clone();
    entry.column_span = input.column_span;
    entry.shown_when = input.shown_when.clone();
    if field.is_none() {
        entry.row_kind = Some(row_kind(input));
        entry.row_nodes = input.content.as_deref().map(|c| c.nodes.clone()).unwrap_or_default();
    }
    if kind == EntryType::TextEntry {
        let d = &mut entry.display;
        // The read-only view keeps line breaks, and JSON indented, everywhere.
        d.multiline = true;
        d.options = input.options.clone();
        d.list = input.kind == InputType::CheckboxList;
        if input.kind == InputType::TextInput {
            d.password = input.mode == Some("password");
            d.prefix = input.prefix.clone();
            d.suffix = input.suffix.clone();
        }
    }
    if kind == EntryType::MediaEntry && !input.preview {
        entry.display.preview = Some(false);
    }
    if kind == EntryType::RepeatableEntry {
        let spec = input.repeater.as_deref()?;
        entry.components = entries_of(&spec.components, &|input| form_entry(input, None));
    }
    Some(entry)
}

/// `components` with each input as the entry `entry` makes of it (none:
/// left out), in their layouts (layouts left empty dropped).
fn entries_of(components: &[Component], entry: &dyn Fn(&Input) -> Option<Entry>) -> Vec<Component> {
    components
        .iter()
        .filter_map(|component| match component {
            Component::Input(input) => entry(input).map(Component::Entry),
            Component::Layout(layout) => {
                let children = entries_of(&layout.children, entry);
                (!children.is_empty()).then(|| Component::Layout(Layout { children, ..layout.clone() }))
            }
            Component::Entry(_) => None,
        })
        .collect()
}

fn entry_label(kind: EntryType) -> &'static str {
    match kind {
        EntryType::TextEntry => "TextEntry",
        EntryType::RelationEntry => "RelationEntry (belongs_to fields)",
        EntryType::BooleanEntry => "BooleanEntry (boolean columns)",
        EntryType::KeyValueEntry => "KeyValueEntry (key/value JSON columns)",
        EntryType::MediaEntry => "MediaEntry (media fields)",
        EntryType::RichTextEntry => "RichTextEntry (rich text fields)",
        EntryType::ComputedEntry => "ComputedEntry",
        EntryType::CustomEntry => "CustomEntry",
        EntryType::RepeatableEntry => "RepeatableEntry (JSON columns)",
    }
}

/// The entries of each row of a repeatable entry: keys of the row, so text,
/// boolean, key/value, custom and (nested, three deep at most) repeatable
/// entries, each once.
fn check_row_entries(owner: &str, entry: &Entry, depth: u8) {
    let here = format!("{owner}: repeatable entry `{}`", entry.name);
    assert!(depth <= crate::engine::repeater::MAX_DEPTH, "{here}: repeatable entries nest 3 deep at most");
    assert!(!entry.components.is_empty(), "{here}: the row has no entry (`.schema(components![…])`)");
    check_layouts(&here, &entry.components, None);
    assert!(!holds_inputs(&entry.components), "{here}: a row holds entries, not inputs");
    let rows = Infolist::new().schema(entry.components.clone());
    let mut seen = HashSet::new();
    for row_entry in rows.entries() {
        let name = row_entry.name.as_str();
        assert!(seen.insert(name), "{here}: entry `{name}` appears twice in a row");
        assert!(
            matches!(
                row_entry.kind,
                EntryType::TextEntry
                    | EntryType::BooleanEntry
                    | EntryType::KeyValueEntry
                    | EntryType::CustomEntry
                    | EntryType::RepeatableEntry
                    | EntryType::MediaEntry
                    | EntryType::RichTextEntry
            ),
            "{here}: entry `{name}`: a row shows its keys with text, boolean, key/value, media, rich text, custom and \
             repeatable entries"
        );
        check_options(&here, row_entry, None);
        if row_entry.kind == EntryType::RepeatableEntry {
            check_row_entries(&here, row_entry, depth + 1);
        }
    }
    // Conditions in a row name entries of the same row (values read from the row).
    let kind_of = |name: &str| seen.contains(name).then_some((FieldKind::Json, None));
    crate::engine::conditions::FormConditions::new(&here, &entry.components, &kind_of);
}

/// Gives the rich text entries of a row's `schemas` the application nodes
/// their `entries` (made from a JSON row's rich editors) accept, at any depth.
fn fill_row_nodes(
    schemas: &mut [ComponentSchema],
    entries: &[Component],
    table: &crate::rich_text::AppNodes,
    t: &dyn Fn(&str) -> String,
) {
    let declared = Infolist::new().schema(entries.to_vec());
    let declared = declared.entries();
    for schema in schemas {
        match schema {
            ComponentSchema::Entry(entry) => {
                let Some(source) = declared.iter().find(|e| e.name == entry.name) else { continue };
                if entry.kind == EntryType::RichTextEntry && !source.row_nodes.is_empty() {
                    let accepts = crate::rich_text::Accepts { nodes: table, only: Some(&source.row_nodes) };
                    entry.rich_nodes = super::schema::rich_node_schemas(accepts, t);
                }
                if entry.kind == EntryType::RepeatableEntry {
                    fill_row_nodes(&mut entry.components, &source.components, table, t);
                }
            }
            ComponentSchema::Layout(layout) => fill_row_nodes(&mut layout.components, entries, table, t),
            _ => {}
        }
    }
}

/// The schemas of a row's entries (values read from the row: kind `json`).
fn row_components(components: &[Component], t: &dyn Fn(&str) -> String) -> Vec<ComponentSchema> {
    components
        .iter()
        .filter_map(|component| match component {
            Component::Entry(entry) => {
                let (label, value_type) = (humanize(&entry.name), entry.row_kind.map_or("json", FieldKind::name));
                Some(ComponentSchema::Entry(entry_schema(entry, &label, value_type, None, false, false, t)))
            }
            Component::Layout(layout) => {
                let children = row_components(&layout.children, t);
                (!children.is_empty()).then(|| ComponentSchema::Layout(layout_schema(layout, children, t)))
            }
            Component::Input(_) => None,
        })
        .collect()
}

/// Options that do not fit the entry, or its field's kind, are refused.
fn check_options(owner: &str, entry: &Entry, kind: Option<FieldKind>) {
    let name = &entry.name;
    let d = &entry.display;
    let text = matches!(entry.kind, EntryType::TextEntry | EntryType::ComputedEntry);
    let refuse = |option: &str, allowed: bool| {
        assert!(allowed, "{owner}: entry `{name}`: `{option}` does not fit a {}", entry_label(entry.kind));
    };
    refuse("badge", !d.badge || text);
    refuse("color", d.colors.is_empty() || text || entry.kind == EntryType::BooleanEntry);
    // Text entries color only their badges (the panel ignores colors otherwise).
    assert!(d.colors.is_empty() || !text || d.badge, "{owner}: entry `{name}`: `color` needs `badge`");
    refuse("copyable", !d.copyable || text);
    refuse("limit", d.limit.is_none() || text);
    refuse("url", !d.url || text);
    refuse("list", !d.list || text);
    refuse("multiline", !d.multiline || text);
    refuse("password", !d.password || text);
    refuse("prefix", d.prefix.is_none() || text);
    refuse("suffix", d.suffix.is_none() || text);
    refuse("options", d.options.is_empty() || text);
    refuse("labels", d.labels.is_none() || entry.kind == EntryType::BooleanEntry);
    refuse("size", d.size.is_none() || entry.kind == EntryType::MediaEntry);
    refuse("preview", d.preview.is_none() || entry.kind == EntryType::MediaEntry);
    refuse("date_format", d.date_format.is_none() || entry.kind == EntryType::TextEntry);
    if let Some(pattern) = &d.date_format {
        // Computed values and row keys have no declared kind: a date format needs a field's.
        let kind = kind.unwrap_or_else(|| {
            panic!("{owner}: entry `{name}`: a date format needs a date field (computed values, rows and page data have no kind)")
        });
        crate::engine::date_format::check(&format!("{owner}: entry `{name}`"), pattern, Some(kind));
    }
    refuse("outline", !d.outline || entry.kind == EntryType::RichTextEntry);
    refuse("props", d.props.is_none() || entry.kind == EntryType::CustomEntry);
    refuse("schema", entry.components.is_empty() || entry.kind == EntryType::RepeatableEntry);
    if d.decimals.is_some() {
        let numeric =
            matches!(kind, None | Some(FieldKind::I32 | FieldKind::I64 | FieldKind::F64 | FieldKind::Decimal));
        assert!(text && numeric, "{owner}: entry `{name}`: `number` needs a number field or a computed value");
        // Browsers format at most 100 decimals; a value has far fewer worth showing.
        assert!(d.decimals.is_some_and(|n| n <= 20), "{owner}: entry `{name}`: `number` shows at most 20 decimals");
    }
    if let Some(widget) = &d.widget {
        assert!(crate::ui::valid_widget(widget), "{owner}: invalid entry name `{widget}`");
        let size = d.props.as_ref().map_or(0, |p| p.to_string().len());
        assert!(size <= 4096, "{owner}: props of entry `{widget}` exceed 4 KiB");
    }
}

impl<R: Resource> ResourceAdapter<R> {
    /// The infolist the user of `ctx` sees (entries of fields they may not
    /// read, and computed values they may not see, left out).
    pub(super) async fn infolist_schema(&self, ctx: &Ctx) -> Option<InfolistSchema> {
        let infolist = self.infolist.as_ref()?;
        // Relation entries link when the user may list the related resource.
        let mut links = HashMap::new();
        for entry in infolist.entries().into_iter().filter(|e| e.kind == EntryType::RelationEntry) {
            let Some(target) = self.field(&entry.name).and_then(|f| f.belongs_to) else { continue };
            if let std::collections::hash_map::Entry::Vacant(slot) = links.entry(target) {
                let listable = match ctx.registry.get(target) {
                    Some(resource) => resource.allows(ctx, ability::VIEW_ANY).await,
                    None => false,
                };
                slot.insert(listable);
            }
        }
        // Rows of has-many relations, for a user who may list them (ADR 0044).
        let mut rows = HashMap::new();
        for entry in infolist.entries().into_iter().filter(|e| e.relationship) {
            let Some(relation) = self.relations.iter().find(|r| r.name == entry.name) else { continue };
            let Some(child) = ctx.registry.get(relation.target) else { continue };
            if !child.allows(ctx, ability::VIEW_ANY).await
                || !crate::registry::relation_access(ctx, child.as_ref(), relation).readable
            {
                continue;
            }
            let order_by = self
                .row_repeaters
                .iter()
                .find(|r| r.name == relation.name)
                .and_then(|r| r.order_column.clone())
                // The list sorts by it: the first rows are the first by position.
                .filter(|column| child.sorts_by(ctx, column));
            let schema = EntryRows {
                resource: relation.target,
                relation: relation.name,
                order_by,
                limit: entry.display.limit.unwrap_or(MAX_ENTRY_ROWS).min(MAX_ENTRY_ROWS),
            };
            let label =
                entry.label.clone().or_else(|| relation.label.clone()).unwrap_or_else(|| humanize(relation.name));
            rows.insert(entry.name.clone(), (schema, child.entry_row_schemas(ctx, &entry.components), label));
        }
        Some(InfolistSchema {
            columns: infolist.columns,
            components: self.entry_components(ctx, &infolist.components, &EntryLinks { links, rows }),
            width: infolist.width.unwrap_or(self.form.width),
            from_form: infolist.from_form,
        })
    }

    pub(super) fn entry_components(
        &self,
        ctx: &Ctx,
        components: &[Component],
        links: &EntryLinks,
    ) -> Vec<ComponentSchema> {
        components
            .iter()
            .filter_map(|component| match component {
                Component::Entry(entry) => self.entry_schema(ctx, entry, links).map(ComponentSchema::Entry),
                Component::Layout(layout) => {
                    let children = self.entry_components(ctx, &layout.children, links);
                    (!children.is_empty())
                        .then(|| ComponentSchema::Layout(layout_schema(layout, children, |l| ctx.t_in(R::SLUG, l))))
                }
                // Infolists hold no inputs (refused at boot).
                Component::Input(_) => None,
            })
            .collect()
    }

    fn entry_schema(&self, ctx: &Ctx, entry: &Entry, links: &EntryLinks) -> Option<EntrySchema> {
        let t = |text: &str| ctx.t_in(R::SLUG, text);
        if entry.relationship {
            // Left out for a user who may not list the related resource.
            let (rows, components, label) = links.rows.get(&entry.name)?;
            let mut schema = entry_schema(entry, label, "rows", None, false, false, &t);
            schema.components = components.clone();
            schema.limit = None;
            schema.rows = Some(rows.clone());
            return Some(schema);
        }
        let (label, value_type, relationship) = match self.field(&entry.name) {
            Some(field) => {
                if !field.can_read(ctx) {
                    return None;
                }
                (field.display_label(), self.column(&entry.name).kind.name(), field.belongs_to)
            }
            None => {
                let computed = self.computed.iter().find(|c| c.name == entry.name)?;
                if !computed.can_read(ctx) {
                    return None;
                }
                (humanize(&entry.name), "json", None)
            }
        };
        let relation = entry.kind == EntryType::RelationEntry;
        let relationship = relationship.filter(|_| relation);
        let link = relationship.is_some_and(|r| links.links.get(r).copied().unwrap_or(false));
        let computed = self.field(&entry.name).is_none();
        let mut schema = entry_schema(entry, &label, value_type, relationship, link, computed, &t);
        if entry.kind == EntryType::RichTextEntry {
            schema.rich_nodes = self.rich_node_schemas(ctx, &entry.name);
        }
        // Rich text in JSON rows: the nodes of the rows' editors (ADR 0044).
        if entry.kind == EntryType::RepeatableEntry {
            fill_row_nodes(&mut schema.components, &entry.components, ctx.registry.rich_nodes(), &t);
        }
        Some(schema)
    }

    /// The computed values of record `key` its view page shows, for a user who
    /// may view it. Each runs within its time limit; one that fails (error,
    /// timeout, too large) is listed in `failed`, logged, and the others are
    /// returned.
    pub(super) async fn computed_values(&self, ctx: &Ctx, key: &str) -> AppResult<ComputedValues> {
        self.admit(ctx, ability::VIEW).await?;
        let model = self.find_model(ctx, &ctx.db, key).await?;
        if !self.policy.allows(ctx, ability::VIEW, Some(&self.record(&model))).await {
            return Err(AppError::Forbidden);
        }
        // A turn of the user's first (never waiting: a user beyond theirs gets
        // 429 at once, holding nothing), then a bounded wait for a shared
        // turn, after authorization and before running anything (the route
        // is also limited per user and minute, see `http::computed`).
        let _user = UserTurn::take(&ctx.principal.id).ok_or(AppError::TooManyRequests { retry_after: 5 })?;
        let _turn = tokio::time::timeout(COMPUTING_WAIT, COMPUTING.acquire())
            .await
            .map_err(|_| AppError::TooManyRequests { retry_after: 5 })?
            .map_err(AppError::internal)?;
        let shown: HashSet<&str> = self
            .infolist
            .as_ref()
            .map(|i| i.entries().into_iter().map(|e| e.name.as_str()).collect())
            .unwrap_or_default();
        let wanted: Vec<&Computed<Entity<R>>> =
            self.computed.iter().filter(|c| shown.contains(c.name.as_str()) && c.can_read(ctx)).collect();
        let model = &model;
        let results = futures_util::future::join_all(
            wanted.iter().map(|c| async move { tokio::time::timeout(c.timeout, (c.compute)(ctx, model)).await }),
        )
        .await;
        let mut data = Data::new();
        let mut failed = Vec::new();
        for (computed, result) in wanted.into_iter().zip(results) {
            let name = computed.name.as_str();
            let failure = match result {
                Ok(Ok(value)) if fits(&value) => {
                    data.insert(name.to_owned(), value);
                    continue;
                }
                Ok(Ok(_)) => "larger than 64 KiB".to_owned(),
                Ok(Err(err)) => err.to_string(),
                Err(_) => format!("took longer than {:?}", computed.timeout),
            };
            tracing::warn!(resource = R::SLUG, computed = name, failure, "computed value not available");
            failed.push(name.to_owned());
        }
        Ok(ComputedValues { data, failed })
    }
}

/// The texts of `entry` to translate: its label (`default_label` when it
/// declares none), help text and option labels.
pub(crate) fn entry_strings(entry: &Entry, default_label: impl FnOnce() -> String, out: &mut Vec<String>) {
    out.push(entry.label.clone().unwrap_or_else(default_label));
    other_strings(entry, out);
}

/// [`entry_strings`] but the label, and the entries of a repeatable entry's rows.
fn other_strings(entry: &Entry, out: &mut Vec<String>) {
    out.extend(entry.help.clone());
    out.extend(entry.display.options.iter().map(|o| o.label.clone()));
    if let Some((yes, no)) = &entry.display.labels {
        out.extend([yes.clone(), no.clone()]);
    }
    for row_entry in Infolist::new().schema(entry.components.clone()).entries() {
        out.push(row_entry.label.clone().unwrap_or_else(|| humanize(&row_entry.name)));
        other_strings(row_entry, out);
    }
}

/// Whether `value`, serialized, takes at most [`COMPUTED_MAX_BYTES`]: counted
/// as it is written, stopping past the limit (nothing is buffered).
fn fits(value: &serde_json::Value) -> bool {
    struct Budget(usize);
    impl std::io::Write for Budget {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self.0.checked_sub(bytes.len()).ok_or(std::io::ErrorKind::WriteZero)?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(Budget(COMPUTED_MAX_BYTES), value).is_ok()
}

/// The schema of `entry` (`label`: its default label; texts translated by `t`).
fn entry_schema(
    entry: &Entry,
    label: &str,
    value_type: &'static str,
    relationship: Option<&'static str>,
    link: bool,
    computed: bool,
    t: &dyn Fn(&str) -> String,
) -> EntrySchema {
    let d = &entry.display;
    EntrySchema {
        kind: entry.kind,
        name: entry.name.clone(),
        label: t(entry.label.as_deref().unwrap_or(label)),
        value_type,
        column_span: entry.column_span,
        help: entry.help.as_deref().map(t),
        shown_when: entry.shown_when.as_deref().cloned(),
        badge: d.badge,
        colors: d
            .colors
            .iter()
            .map(|(value, color)| BadgeColor { value: value.clone(), color: color.clone() })
            .collect(),
        copyable: d.copyable,
        limit: d.limit,
        url: d.url,
        decimals: d.decimals,
        list: d.list,
        multiline: d.multiline,
        password: d.password,
        prefix: d.prefix.clone(),
        suffix: d.suffix.clone(),
        options: d.options.iter().map(|o| SelectOption { value: o.value.clone(), label: t(&o.label) }).collect(),
        labels: d.labels.as_ref().map(|(yes, no)| [t(yes), t(no)]),
        size: d.size,
        preview: d.preview.unwrap_or(true),
        format: d.date_format.clone(),
        outline: d.outline,
        rich_nodes: Vec::new(),
        relationship: relationship.map(|resource| Relationship { resource }),
        computed,
        link,
        widget: d.widget.clone(),
        props: d.props.clone(),
        components: row_components(&entry.components, t),
        rows: None,
    }
}

/// What an infolist's entries link to: related resources the user may list
/// (relation entries), and the rows of relationship repeatable entries with
/// their entries' schemas and default label (ADR 0044).
#[derive(Default)]
pub(super) struct EntryLinks {
    pub(super) links: HashMap<&'static str, bool>,
    pub(super) rows: HashMap<String, (EntryRows, Vec<ComponentSchema>, String)>,
}

/// Checks the infolist of panel page (or slot) `owner`: entries read keys of
/// the page's data, so only text, boolean, key/value and custom entries, and
/// conditions with `filled` only (the values have no declared kind).
pub(crate) fn check_page_infolist(owner: &str, infolist: &Infolist) {
    assert!(!infolist.is_empty(), "{owner}: the infolist is empty");
    check_layouts(owner, &infolist.components, None);
    assert!(!holds_inputs(&infolist.components), "{owner}: an infolist holds entries, not inputs");
    let mut seen = HashSet::new();
    for entry in infolist.entries() {
        let name = entry.name.as_str();
        assert!(seen.insert(name), "{owner}: entry `{name}` appears twice in the infolist");
        assert!(
            matches!(
                entry.kind,
                EntryType::TextEntry
                    | EntryType::BooleanEntry
                    | EntryType::KeyValueEntry
                    | EntryType::CustomEntry
                    | EntryType::RepeatableEntry
            ),
            "{owner}: entry `{name}`: a page's infolist shows its data with text, boolean, key/value and custom \
             entries only"
        );
        check_options(owner, entry, None);
        if entry.kind == EntryType::RepeatableEntry {
            check_row_entries(owner, entry, 1);
        }
    }
    let kind_of = |_: &str| Some((FieldKind::Json, None));
    crate::engine::conditions::FormConditions::new(owner, &infolist.components, &kind_of);
}

/// The schema of a panel page's (or slot's) infolist; its texts translated globally.
pub(crate) fn page_infolist_schema(ctx: &Ctx, infolist: &Infolist) -> InfolistSchema {
    fn walk(ctx: &Ctx, components: &[Component]) -> Vec<ComponentSchema> {
        let t = |text: &str| ctx.t(text);
        components
            .iter()
            .filter_map(|component| match component {
                Component::Entry(entry) => Some(ComponentSchema::Entry(entry_schema(
                    entry,
                    &humanize(&entry.name),
                    "json",
                    None,
                    false,
                    false,
                    &t,
                ))),
                Component::Layout(layout) => {
                    let children = walk(ctx, &layout.children);
                    (!children.is_empty()).then(|| ComponentSchema::Layout(layout_schema(layout, children, t)))
                }
                Component::Input(_) => None,
            })
            .collect()
    }
    InfolistSchema {
        columns: infolist.columns,
        components: walk(ctx, &infolist.components),
        width: infolist.width.unwrap_or_default(),
        from_form: false,
    }
}
