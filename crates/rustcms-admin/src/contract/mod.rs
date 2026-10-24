//! Schema DTOs: the contract between the server and admin clients
//! (`docs/core/admin-protocol.md`). Bump [`SCHEMA_VERSION`] on breaking changes
//! of the structure and [`VOCABULARY_VERSION`] when component types change.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::ui::{ColumnType, FilterType, InputType, LayoutType, SelectOption};

/// Version of the schema structure.
pub const SCHEMA_VERSION: u32 = 2;
/// Version of the component, column and filter vocabulary.
pub const VOCABULARY_VERSION: u32 = 10;

/// Navigation entry and identity of a resource.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ResourceInfo {
    /// URL slug.
    pub slug: &'static str,
    /// Singular label.
    pub label: String,
    /// Plural label.
    pub plural_label: String,
    /// Icon name (client icon set).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Navigation group.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// Field used as record title.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_field: Option<String>,
    /// Navigation position (`ResourceMeta::sort`), absent when 0.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort: Option<i32>,
    /// `false` for a resource with no navigation entry
    /// (`ResourceMeta::navigation(false)`), absent otherwise.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nav: Option<bool>,
}

impl ResourceInfo {
    /// The labels in the request's language (the English plural is formed first).
    pub fn translated(mut self, ctx: &crate::Ctx) -> Self {
        self.label = ctx.t_in(self.slug, &self.label);
        self.plural_label = ctx.t_in(self.slug, &self.plural_label);
        self.group = self.group.map(|g| ctx.t(&g));
        self
    }
}

/// Which form a schema describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum SchemaOperation {
    /// New record.
    Create,
    /// Existing record, editable fields enabled.
    Edit,
    /// Existing record, read-only.
    View,
}

/// A validation rule mirrored for the client. The server always re-checks.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "rule", rename_all = "snake_case")]
pub enum Rule {
    /// Must be present and non-null.
    Required,
    /// Maximum length in characters (for key/value inputs: of each value; keys
    /// are limited to 255 characters).
    MaxLength {
        /// Limit.
        value: usize,
    },
    /// Maximum number of items (key/value pairs).
    MaxItems {
        /// Limit.
        value: usize,
    },
}

/// Related resource of a `belongs_to` input or column.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Relationship {
    /// Slug of the related resource (options come from the field's options endpoint).
    pub resource: &'static str,
}

/// A form input.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct InputSchema {
    /// Input type.
    #[serde(rename = "type")]
    pub kind: InputType,
    /// Field name.
    pub name: String,
    /// Label.
    pub label: String,
    /// Wire kind of the value (`string`, `i64`, `decimal`, `instant`, …).
    pub value_type: &'static str,
    /// The column accepts `null` (an emptied input sends `null`; otherwise an
    /// empty string for text kinds).
    pub nullable: bool,
    /// Create forms: the column's declared default (entity `default_value`), in
    /// wire format. Inputs without one start empty, and untouched ones are left
    /// out of the payload so the database default applies.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub default: Option<serde_json::Value>,
    /// Shown but not editable.
    pub disabled: bool,
    /// Writable but not readable (e.g. a password): no current value is sent;
    /// leave it out of the payload to keep the stored value.
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub write_only: bool,
    /// Client-side rules.
    pub rules: Vec<Rule>,
    /// Grid span.
    pub column_span: u8,
    /// Placeholder.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    /// Help text.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
    /// Input mode (`email`, `password`, `url`).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<&'static str>,
    /// Static options.
    #[cfg_attr(feature = "ts", ts(as = "Option<Vec<SelectOption>>", optional))]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<SelectOption>,
    /// Related resource for `belongs_to` fields.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relationship: Option<Relationship>,
    /// Widget of a `custom` input (`vendor/name`).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub widget: Option<String>,
    /// Settings of a `custom` input's widget.
    #[cfg_attr(feature = "ts", ts(optional, type = "JsonValue"))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub props: Option<serde_json::Value>,
    /// Media references (`file_upload` inputs).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media: Option<MediaSchema>,
    /// What the user may change (`belongs_to_many` inputs, vocabulary 9).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub links: Option<LinksSchema>,
}

/// The changes a `belongs_to_many` input may send (checked again by the
/// server on every write, on the record as stored then).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct LinksSchema {
    /// Link more records (`attach_{relation}`).
    pub attach: bool,
    /// Unlink linked records (`detach_{relation}`; edit forms only).
    pub detach: bool,
}

/// What a media input accepts (checked again by the server on every write).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct MediaSchema {
    /// Several media, in order (a list of ids).
    pub multiple: bool,
    /// Most items.
    pub max_items: usize,
    /// Accepted MIME types (`image/*`); empty accepts every uploadable type.
    pub accept: Vec<String>,
    /// Largest accepted file, in bytes.
    #[cfg_attr(feature = "ts", ts(optional, type = "number"))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_size: Option<u64>,
}

/// A layout.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct LayoutSchema {
    /// Layout type.
    #[serde(rename = "type")]
    pub kind: LayoutType,
    /// Stable identity (tabs).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Title (sections, tabs).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Columns.
    pub columns: u8,
    /// Children.
    pub components: Vec<ComponentSchema>,
}

/// An element of a form: input or layout, told apart by `type`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(untagged)]
// A serialized DTO, built once per schema request: the size difference does not matter.
#[allow(clippy::large_enum_variant)]
pub enum ComponentSchema {
    /// Input.
    Input(InputSchema),
    /// Layout.
    Layout(LayoutSchema),
}

/// A form.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FormSchema {
    /// Columns.
    pub columns: u8,
    /// Components the user may see for the operation.
    pub components: Vec<ComponentSchema>,
}

/// A table column.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ColumnSchema {
    /// Column type.
    #[serde(rename = "type")]
    pub kind: ColumnType,
    /// Field name.
    pub name: String,
    /// Header.
    pub label: String,
    /// Wire kind of the value.
    pub value_type: &'static str,
    /// May be used in `sort`.
    pub sortable: bool,
    /// Included in `search`.
    pub searchable: bool,
    /// Badge colors.
    #[cfg_attr(feature = "ts", ts(as = "Option<Vec<BadgeColor>>", optional))]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub colors: Vec<BadgeColor>,
    /// Related resource (display `labels[name]`).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relationship: Option<Relationship>,
    /// Custom cell (`vendor/name`) drawing the value, if registered.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell: Option<String>,
    /// Settings of the custom cell.
    #[cfg_attr(feature = "ts", ts(optional, type = "JsonValue"))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub props: Option<serde_json::Value>,
    /// Hidden on screens narrower than this breakpoint (`sm`: 640 px, `md`:
    /// 768 px, `lg`: 1024 px); presentation only (vocabulary 10).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hide_below: Option<crate::ui::Breakpoint>,
}

/// Badge color for one value.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct BadgeColor {
    /// Value.
    pub value: serde_json::Value,
    /// Color name.
    pub color: String,
}

/// A table filter.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct FilterSchema {
    /// Filter type.
    #[serde(rename = "type")]
    pub kind: FilterType,
    /// Field name.
    pub name: String,
    /// Label.
    pub label: String,
    /// Wire kind of the value.
    pub value_type: &'static str,
    /// Options (select filters).
    #[cfg_attr(feature = "ts", ts(as = "Option<Vec<SelectOption>>", optional))]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<SelectOption>,
    /// Related resource (options endpoint).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relationship: Option<Relationship>,
}

/// An action the user may run (per-record permission is in each envelope's `abilities`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ActionSchema {
    /// Name (URL segment).
    pub name: &'static str,
    /// Label.
    pub label: String,
    /// Offered for selections of records.
    pub bulk: bool,
    /// Confirmation question, if any.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirm: Option<String>,
    /// Color (`primary`, `danger`, …).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Icon name.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Values asked for before running (in a dialog).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub form: Option<FormSchema>,
    /// Shown with its label in list rows; other row actions go in the row's
    /// menu, those with color `danger` last (vocabulary 10).
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub primary: bool,
}

/// A table.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct TableSchema {
    /// Columns.
    pub columns: Vec<ColumnSchema>,
    /// Filters.
    pub filters: Vec<FilterSchema>,
    /// Default sort (`-field` for descending).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_sort: Option<String>,
    /// Allowed page sizes.
    pub per_page_options: Vec<u64>,
    /// Whether `search` matches anything.
    pub searchable: bool,
    /// Actions.
    pub actions: Vec<ActionSchema>,
    /// Deleting moves records to the trash (`trashed` list parameter, restore, force delete).
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub soft_deletes: bool,
}

/// Everything a client needs to render a resource for one user and operation.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ResourceSchema {
    /// [`SCHEMA_VERSION`].
    pub schema_version: u32,
    /// [`VOCABULARY_VERSION`].
    pub vocabulary_version: u32,
    /// Identity.
    pub resource: ResourceInfo,
    /// Operation the form describes.
    pub operation: SchemaOperation,
    /// `viewAny`, `create`, and for a given record `view`, `update`, `delete`.
    pub abilities: BTreeMap<&'static str, bool>,
    /// Form.
    pub form: FormSchema,
    /// Table.
    pub table: TableSchema,
    /// Has-many relations of the viewed record the user may list.
    #[cfg_attr(feature = "ts", ts(as = "Option<Vec<RelationSchema>>", optional))]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<RelationSchema>,
}

/// A relation shown on a record's page.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct RelationSchema {
    /// URL segment.
    pub name: &'static str,
    /// Has-many or belongs-to-many.
    pub kind: RelationKind,
    /// Section title.
    pub label: String,
    /// Slug of the related resource.
    pub resource: &'static str,
    /// Has-many: field of the related resource holding this record's key (set
    /// by the server). Belongs-to-many: the pivot column (informational).
    pub field: &'static str,
    /// Whether the user may create related records from here (has-many).
    pub create: bool,
    /// Whether the user may attach targets to this record (belongs-to-many).
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub attach: bool,
    /// Whether the user may detach targets from this record.
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub detach: bool,
    /// Pivot columns the user may read, shown with each link.
    #[cfg_attr(feature = "ts", ts(as = "Option<Vec<InputSchema>>", optional))]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub pivot: Vec<InputSchema>,
    /// Pivot columns set when attaching (with `attach`).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attach_form: Option<FormSchema>,
    /// Pivot columns the user may change on a link.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pivot_form: Option<FormSchema>,
}

/// Kind of a [`RelationSchema`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum RelationKind {
    /// Records of the related resource point to this one.
    HasMany,
    /// Linked through a pivot table.
    BelongsToMany,
}
