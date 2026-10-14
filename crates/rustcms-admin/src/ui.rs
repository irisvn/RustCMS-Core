//! Presentation builders, in the spirit of Filament: forms and tables.
//!
//! They only describe the UI. Validation and authorization come from the
//! resource's [`Field`](crate::Field)s and policy, never from these builders:
//! an input for a field the user may not write is dropped or disabled by the
//! schema endpoint, and the write is refused by the engine regardless.
//!
//! ```rust,ignore
//! Form::new().columns(2).schema(components![
//!     Section::make("Content").schema(components![
//!         TextInput::make("title"),
//!         Textarea::make("excerpt").column_span(2),
//!     ]),
//!     SelectInput::make("status").options([("draft", "Draft"), ("published", "Published")]),
//! ])
//! ```

use serde_json::Value as Json;

/// Input types of the form vocabulary (`vocabularyVersion` 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum InputType {
    /// Single-line text.
    TextInput,
    /// Multi-line text.
    Textarea,
    /// Number (`I64` / `Decimal` values stay strings on the wire).
    NumberInput,
    /// Choice among static options, or related records for `belongs_to` fields.
    Select,
    /// On/off switch.
    Toggle,
    /// Checkbox.
    Checkbox,
    /// Several checkboxes (multiple values).
    CheckboxList,
    /// Calendar date.
    DatePicker,
    /// Date and time.
    DateTimePicker,
    /// Not displayed; sent back unchanged.
    Hidden,
    /// Pairs of text keys and values, stored as a JSON object of strings (vocabulary 2).
    KeyValue,
    /// Uploads to the media library; the value is a media id or a list of ids (vocabulary 3).
    FileUpload,
    /// Rich text document editor (vocabulary 3).
    RichEditor,
    /// A widget from an application extension script (vocabulary 4).
    Custom,
}

/// An option of a select, checkbox list or select filter.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SelectOption {
    /// Value in wire format.
    pub value: Json,
    /// Displayed label.
    pub label: String,
}

/// A form input bound to a field of the resource.
#[derive(Debug, Clone)]
pub struct Input {
    pub(crate) name: String,
    pub(crate) kind: InputType,
    pub(crate) label: Option<String>,
    pub(crate) placeholder: Option<String>,
    pub(crate) help: Option<String>,
    pub(crate) options: Vec<SelectOption>,
    pub(crate) column_span: u8,
    pub(crate) mode: Option<&'static str>,
    pub(crate) widget: Option<String>,
    pub(crate) props: Option<serde_json::Value>,
}

impl Input {
    pub(crate) fn with_kind(name: impl Into<String>, kind: InputType) -> Self {
        Self::new(name, kind)
    }

    fn new(name: impl Into<String>, kind: InputType) -> Self {
        Self {
            name: name.into(),
            kind,
            label: None,
            placeholder: None,
            help: None,
            options: Vec::new(),
            column_span: 1,
            mode: None,
            widget: None,
            props: None,
        }
    }

    /// Displayed label (default: the field name, humanized).
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Placeholder text.
    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = Some(text.into());
        self
    }

    /// Help text below the input.
    pub fn help(mut self, text: impl Into<String>) -> Self {
        self.help = Some(text.into());
        self
    }

    /// Static options (selects, checkbox lists).
    pub fn options<V: Into<Json>, L: Into<String>>(mut self, options: impl IntoIterator<Item = (V, L)>) -> Self {
        self.options = options.into_iter().map(|(v, l)| SelectOption { value: v.into(), label: l.into() }).collect();
        self
    }

    /// Columns of the surrounding grid this input spans.
    pub fn column_span(mut self, span: u8) -> Self {
        self.column_span = span.max(1);
        self
    }

    /// Email keyboard and format hint.
    pub fn email(mut self) -> Self {
        self.mode = Some("email");
        self
    }

    /// Masked input.
    pub fn password(mut self) -> Self {
        self.mode = Some("password");
        self
    }

    /// URL keyboard and format hint.
    pub fn url(mut self) -> Self {
        self.mode = Some("url");
        self
    }
}

/// An input rendered by a widget that an application extension registers
/// (`window.rustcms.registerWidget("acme/color", …)`, see `AdminProvider::extension_script`).
pub struct CustomInput;

impl CustomInput {
    /// Field `name` edited by widget `widget` (`vendor/name`, lowercase letters, digits and `-`).
    #[allow(clippy::new_ret_no_self)]
    pub fn make(name: impl Into<String>, widget: &str) -> Input {
        let mut input = Input::new(name, InputType::Custom);
        input.widget = Some(widget.to_owned());
        input
    }
}

impl Input {
    /// Settings passed to a custom widget (JSON, at most 4 KiB).
    pub fn props(mut self, props: serde_json::Value) -> Self {
        self.props = Some(props);
        self
    }
}

/// Whether `name` is a valid `vendor/name` widget name.
pub(crate) fn valid_widget(name: &str) -> bool {
    let part = |p: &str| {
        (1..=32).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    };
    matches!(name.split_once('/'), Some((vendor, widget)) if part(vendor) && part(widget))
}

macro_rules! input_types {
    ($($(#[$doc:meta])* $name:ident => $kind:ident),* $(,)?) => {$(
        $(#[$doc])*
        pub struct $name;

        impl $name {
            /// Input for the field `name`.
            #[allow(clippy::new_ret_no_self)]
            pub fn make(name: impl Into<String>) -> Input {
                Input::new(name, InputType::$kind)
            }
        }
    )*};
}

input_types! {
    /// Single-line text input.
    TextInput => TextInput,
    /// Multi-line text input.
    Textarea => Textarea,
    /// Number input.
    NumberInput => NumberInput,
    /// Select (static options, or related records for `belongs_to` fields).
    /// Named `SelectInput` so it does not clash with `sea_orm::Select` in `scope()`.
    SelectInput => Select,
    /// Toggle switch.
    Toggle => Toggle,
    /// Checkbox.
    Checkbox => Checkbox,
    /// Checkbox list.
    CheckboxList => CheckboxList,
    /// Date picker.
    DatePicker => DatePicker,
    /// Date-time picker.
    DateTimePicker => DateTimePicker,
    /// Hidden input.
    Hidden => Hidden,
    /// Key/value pairs on a JSON column (an object of strings).
    KeyValue => KeyValue,
    /// File upload for a `Field::media()` / `Field::media_list()` field.
    FileUpload => FileUpload,
    /// Rich text editor for a `Field::rich_text()` field.
    RichEditor => RichEditor,
}

/// Layout kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum LayoutType {
    /// Titled group.
    Section,
    /// Columns without a title.
    Grid,
    /// Tabs: every child is a [`LayoutType::Tab`] (vocabulary 2).
    Tabs,
    /// One tab of [`LayoutType::Tabs`], with a label and a stable `id`.
    Tab,
}

/// A layout grouping components.
#[derive(Debug, Clone)]
pub struct Layout {
    pub(crate) kind: LayoutType,
    pub(crate) id: Option<String>,
    pub(crate) label: Option<String>,
    pub(crate) columns: u8,
    pub(crate) children: Vec<Component>,
}

impl Layout {
    /// Number of columns.
    pub fn columns(mut self, columns: u8) -> Self {
        self.columns = columns.max(1);
        self
    }

    /// Child components.
    pub fn schema(mut self, children: Vec<Component>) -> Self {
        self.children = children;
        self
    }
}

/// Titled section.
pub struct Section;

impl Section {
    /// Section titled `label`.
    #[allow(clippy::new_ret_no_self)]
    pub fn make(label: impl Into<String>) -> Layout {
        Layout { kind: LayoutType::Section, id: None, label: Some(label.into()), columns: 1, children: Vec::new() }
    }
}

/// Untitled grid.
pub struct Grid;

impl Grid {
    /// Grid with `columns` columns.
    #[allow(clippy::new_ret_no_self)]
    pub fn make(columns: u8) -> Layout {
        Layout { kind: LayoutType::Grid, id: None, label: None, columns: columns.max(1), children: Vec::new() }
    }
}

/// Tabs; children are [`Tab`]s.
pub struct Tabs;

impl Tabs {
    /// Empty tabs: add [`Tab`]s with `.schema(components![…])`.
    #[allow(clippy::new_ret_no_self)]
    pub fn make() -> Layout {
        Layout { kind: LayoutType::Tabs, id: None, label: None, columns: 1, children: Vec::new() }
    }
}

/// One tab, identified by its label (lowercased, non-alphanumerics as `-`).
pub struct Tab;

impl Tab {
    /// Tab titled `label`.
    #[allow(clippy::new_ret_no_self)]
    pub fn make(label: impl Into<String>) -> Layout {
        let label = label.into();
        let id = label
            .to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("-");
        Layout { kind: LayoutType::Tab, id: Some(id), label: Some(label), columns: 1, children: Vec::new() }
    }
}

/// An element of a form.
#[derive(Debug, Clone)]
pub enum Component {
    /// Field input.
    Input(Input),
    /// Layout.
    Layout(Layout),
}

impl From<Input> for Component {
    fn from(input: Input) -> Self {
        Self::Input(input)
    }
}

impl From<Layout> for Component {
    fn from(layout: Layout) -> Self {
        Self::Layout(layout)
    }
}

/// Builds a `Vec<Component>` from inputs and layouts.
#[macro_export]
macro_rules! components {
    ($($component:expr),* $(,)?) => { vec![$($crate::ui::Component::from($component)),*] };
}

/// A form. An empty form is generated from the resource's fields.
#[derive(Debug, Clone, Default)]
pub struct Form {
    pub(crate) columns: u8,
    pub(crate) components: Vec<Component>,
}

impl Form {
    /// Empty one-column form.
    pub fn new() -> Self {
        Self { columns: 1, components: Vec::new() }
    }

    /// Number of columns.
    pub fn columns(mut self, columns: u8) -> Self {
        self.columns = columns.max(1);
        self
    }

    /// Components.
    pub fn schema(mut self, components: Vec<Component>) -> Self {
        self.components = components;
        self
    }

    /// Every input, depth first.
    pub(crate) fn inputs(&self) -> Vec<&Input> {
        fn walk<'a>(components: &'a [Component], out: &mut Vec<&'a Input>) {
            for component in components {
                match component {
                    Component::Input(input) => out.push(input),
                    Component::Layout(layout) => walk(&layout.children, out),
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.components, &mut out);
        out
    }
}

/// Column types of the table vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ColumnType {
    /// Plain text (also numbers, related titles).
    Text,
    /// Colored badge.
    Badge,
    /// Check or cross.
    Boolean,
    /// Date or date-time.
    Date,
}

/// A table column bound to a field.
#[derive(Debug, Clone)]
pub struct TableColumn {
    pub(crate) name: String,
    pub(crate) kind: ColumnType,
    pub(crate) label: Option<String>,
    pub(crate) colors: Vec<(Json, String)>,
}

impl TableColumn {
    fn new(name: impl Into<String>, kind: ColumnType) -> Self {
        Self { name: name.into(), kind, label: None, colors: Vec::new() }
    }

    /// Header label (default: the field name, humanized).
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Badge color for `value` (`primary`, `success`, `warning`, `danger`, `gray`).
    pub fn color(mut self, value: impl Into<Json>, color: impl Into<String>) -> Self {
        self.colors.push((value.into(), color.into()));
        self
    }
}

macro_rules! column_types {
    ($($(#[$doc:meta])* $name:ident => $kind:ident),* $(,)?) => {$(
        $(#[$doc])*
        pub struct $name;

        impl $name {
            /// Column for the field `name`.
            #[allow(clippy::new_ret_no_self)]
            pub fn make(name: impl Into<String>) -> TableColumn {
                TableColumn::new(name, ColumnType::$kind)
            }
        }
    )*};
}

column_types! {
    /// Text column.
    TextColumn => Text,
    /// Badge column.
    BadgeColumn => Badge,
    /// Boolean column.
    BooleanColumn => Boolean,
    /// Date column.
    DateColumn => Date,
}

/// Filter types of the table vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum FilterType {
    /// One or more of the options (`in`).
    Select,
    /// Yes / no / any (`eq`).
    Ternary,
    /// From / until (`gte`, `lte`).
    DateRange,
}

/// A table filter bound to a filterable field.
#[derive(Debug, Clone)]
pub struct TableFilter {
    pub(crate) name: String,
    pub(crate) kind: FilterType,
    pub(crate) label: Option<String>,
    pub(crate) options: Vec<SelectOption>,
}

impl TableFilter {
    fn new(name: impl Into<String>, kind: FilterType) -> Self {
        Self { name: name.into(), kind, label: None, options: Vec::new() }
    }

    /// Label (default: the field name, humanized).
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Options of a select filter.
    pub fn options<V: Into<Json>, L: Into<String>>(mut self, options: impl IntoIterator<Item = (V, L)>) -> Self {
        self.options = options.into_iter().map(|(v, l)| SelectOption { value: v.into(), label: l.into() }).collect();
        self
    }
}

macro_rules! filter_types {
    ($($(#[$doc:meta])* $name:ident => $kind:ident),* $(,)?) => {$(
        $(#[$doc])*
        pub struct $name;

        impl $name {
            /// Filter on the field `name` (which must be `.filterable()`).
            #[allow(clippy::new_ret_no_self)]
            pub fn make(name: impl Into<String>) -> TableFilter {
                TableFilter::new(name, FilterType::$kind)
            }
        }
    )*};
}

filter_types! {
    /// Select filter.
    SelectFilter => Select,
    /// Ternary (yes / no / any) filter.
    TernaryFilter => Ternary,
    /// Date range filter.
    DateRangeFilter => DateRange,
}

/// A table. An empty table is generated from the resource's fields.
#[derive(Debug, Clone, Default)]
pub struct Table {
    pub(crate) columns: Vec<TableColumn>,
    pub(crate) filters: Vec<TableFilter>,
    pub(crate) default_sort: Option<String>,
}

impl Table {
    /// Empty table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Columns.
    pub fn columns(mut self, columns: Vec<TableColumn>) -> Self {
        self.columns = columns;
        self
    }

    /// Filters.
    pub fn filters(mut self, filters: Vec<TableFilter>) -> Self {
        self.filters = filters;
        self
    }

    /// Default sort, e.g. `-created_at` (the field must be `.sortable()`).
    pub fn default_sort(mut self, sort: impl Into<String>) -> Self {
        self.default_sort = Some(sort.into());
        self
    }
}

/// `category_id` → `Category`, `published_at` → `Published at`.
pub(crate) fn humanize(name: &str) -> String {
    let name = name.strip_suffix("_id").unwrap_or(name);
    let mut out = name.replace('_', " ");
    if let Some(first) = out.get(0..1) {
        let upper = first.to_uppercase();
        out.replace_range(0..1, &upper);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn humanizes_names() {
        assert_eq!(humanize("category_id"), "Category");
        assert_eq!(humanize("published_at"), "Published at");
        assert_eq!(humanize("title"), "Title");
    }

    #[test]
    fn walks_nested_inputs() {
        let form = Form::new().schema(crate::components![
            TextInput::make("a"),
            Section::make("S").schema(crate::components![Grid::make(2).schema(crate::components![Toggle::make("b")])]),
        ]);
        let names: Vec<&str> = form.inputs().iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["a", "b"]);
    }
}
