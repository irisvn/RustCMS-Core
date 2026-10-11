//! Schema DTOs: the contract between the server and admin clients
//! (`docs/src/admin-protocol.md`). Bump [`SCHEMA_VERSION`] on breaking changes
//! of the structure and [`VOCABULARY_VERSION`] when component types change.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::ui::{ColumnType, FilterType, InputType, LayoutType, SelectOption};

/// Version of the schema structure.
pub const SCHEMA_VERSION: u32 = 1;
/// Version of the component, column and filter vocabulary.
pub const VOCABULARY_VERSION: u32 = 1;

/// Navigation entry and identity of a resource.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceInfo {
    /// URL slug.
    pub slug: &'static str,
    /// Singular label.
    pub label: String,
    /// Plural label.
    pub plural_label: String,
    /// Icon name (client icon set).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Navigation group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// Field used as record title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_field: Option<String>,
}

/// Which form a schema describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
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
#[serde(tag = "rule", rename_all = "snake_case")]
pub enum Rule {
    /// Must be present and non-null.
    Required,
    /// Maximum length in characters.
    MaxLength {
        /// Limit.
        value: usize,
    },
}

/// Related resource of a `belongs_to` input or column.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Relationship {
    /// Slug of the related resource (options come from the field's options endpoint).
    pub resource: &'static str,
}

/// A form input.
#[derive(Debug, Clone, PartialEq, Serialize)]
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
    /// Shown but not editable.
    pub disabled: bool,
    /// Writable but not readable (e.g. a password): no current value is sent;
    /// leave it out of the payload to keep the stored value.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub write_only: bool,
    /// Client-side rules.
    pub rules: Vec<Rule>,
    /// Grid span.
    pub column_span: u8,
    /// Placeholder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    /// Help text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
    /// Input mode (`email`, `password`, `url`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<&'static str>,
    /// Static options.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<SelectOption>,
    /// Related resource for `belongs_to` fields.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relationship: Option<Relationship>,
}

/// A layout.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutSchema {
    /// Layout type.
    #[serde(rename = "type")]
    pub kind: LayoutType,
    /// Title (sections).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Columns.
    pub columns: u8,
    /// Children.
    pub components: Vec<ComponentSchema>,
}

/// An element of a form: input or layout, told apart by `type`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ComponentSchema {
    /// Input.
    Input(InputSchema),
    /// Layout.
    Layout(LayoutSchema),
}

/// A form.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FormSchema {
    /// Columns.
    pub columns: u8,
    /// Components the user may see for the operation.
    pub components: Vec<ComponentSchema>,
}

/// A table column.
#[derive(Debug, Clone, PartialEq, Serialize)]
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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub colors: Vec<BadgeColor>,
    /// Related resource (display `labels[name]`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relationship: Option<Relationship>,
}

/// Badge color for one value.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BadgeColor {
    /// Value.
    pub value: serde_json::Value,
    /// Color name.
    pub color: String,
}

/// A table filter.
#[derive(Debug, Clone, PartialEq, Serialize)]
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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<SelectOption>,
    /// Related resource (options endpoint).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relationship: Option<Relationship>,
}

/// An action the user may run (per-record permission is in each envelope's `abilities`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionSchema {
    /// Name (URL segment).
    pub name: &'static str,
    /// Label.
    pub label: String,
    /// Offered for selections of records.
    pub bulk: bool,
    /// Confirmation question, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirm: Option<String>,
    /// Color (`primary`, `danger`, …).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Icon name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

/// A table.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableSchema {
    /// Columns.
    pub columns: Vec<ColumnSchema>,
    /// Filters.
    pub filters: Vec<FilterSchema>,
    /// Default sort (`-field` for descending).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_sort: Option<String>,
    /// Allowed page sizes.
    pub per_page_options: Vec<u64>,
    /// Whether `search` matches anything.
    pub searchable: bool,
    /// Actions.
    pub actions: Vec<ActionSchema>,
}

/// Everything a client needs to render a resource for one user and operation.
#[derive(Debug, Clone, PartialEq, Serialize)]
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
}
