//! Schema DTOs: the contract between the server and admin clients
//! (`docs/core/admin-protocol.md`). Bump [`SCHEMA_VERSION`] on breaking changes
//! of the structure and [`VOCABULARY_VERSION`] when component types change.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::ui::{
    ColumnType, Condition, EntryType, FilterType, FormWidth, InputType, LayoutType, NumberBounds, SelectOption,
};

/// Version of the schema structure.
pub const SCHEMA_VERSION: u32 = 2;
/// Version of the component, column and filter vocabulary.
pub const VOCABULARY_VERSION: u32 = 20;

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
    /// Field used as record title (a title of one field; see `title_fields`).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_field: Option<String>,
    /// Fields of a record title of several fields, in template order (ADR 0038).
    #[cfg_attr(feature = "ts", ts(as = "Option<Vec<String>>", optional))]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub title_fields: Vec<String>,
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
    /// Fixed text shown inside the input before the value (text inputs, vocabulary 11).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    /// Fixed text shown inside the input after the value (text inputs, vocabulary 11).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suffix: Option<String>,
    /// Bounds and step of a number input (vocabulary 12); `min` and `max`
    /// are checked again by the server on every client write.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<NumberBounds>,
    /// Shown only when this holds (vocabulary 13, ADR 0037): evaluated on
    /// every change; the server drops values sent for hidden inputs.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shown_when: Option<Condition>,
    /// Hidden after a write, the field is cleared by the server (vocabulary 13).
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub clear_when_hidden: bool,
    /// JSON repeater rows: a whole number (vocabulary 15).
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub integer: bool,
    /// Repeaters: their rows (vocabulary 15, ADR 0040).
    #[cfg_attr(feature = "ts", ts(as = "Option<RepeaterSchema>", optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repeater: Option<Box<RepeaterSchema>>,
    /// A change asks the server's reaction (`POST form/react`) for changes to
    /// the form (vocabulary 19, ADR 0043). Create and edit forms only.
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub reacts: bool,
    /// Rich text: the application nodes the document may hold (vocabulary
    /// 18, ADR 0042); absent when none.
    #[cfg_attr(feature = "ts", ts(as = "Option<Vec<RichNodeSchema>>", optional))]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rich_nodes: Vec<RichNodeSchema>,
}

/// An application node a rich text document may hold (vocabulary 18, ADR
/// 0042): an atom with typed attributes, checked again by the server.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct RichNodeSchema {
    /// Its name (`app.…`).
    pub name: String,
    /// Label (translated).
    pub label: String,
    /// Inside text, or a block.
    pub inline: bool,
    /// Its attributes, in declaration order.
    pub attrs: Vec<RichAttrSchema>,
}

/// An attribute of an application node. Options absent when unset.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct RichAttrSchema {
    /// Its name.
    pub name: String,
    /// The label of its input (translated; its name when none is declared).
    pub label: String,
    /// `text`, `integer`, `boolean`, `one_of` or `uuid`.
    pub kind: &'static str,
    /// Without a default: the document must give it.
    pub required: bool,
    /// The value when left out.
    #[cfg_attr(feature = "ts", ts(optional, type = "JsonValue"))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
    /// Text: fewest characters; integer: smallest value.
    #[cfg_attr(feature = "ts", ts(optional, type = "number"))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<i64>,
    /// Text: most characters; integer: largest value.
    #[cfg_attr(feature = "ts", ts(optional, type = "number"))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<i64>,
    /// `one_of`: the values allowed.
    #[cfg_attr(feature = "ts", ts(as = "Option<Vec<String>>", optional))]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
}

/// The rows of a repeater (vocabulary 15, ADR 0040). Options absent when unset.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct RepeaterSchema {
    /// The row's inputs and layouts.
    pub components: Vec<ComponentSchema>,
    /// Relationship repeaters on an edit page: the inputs of a new row (the
    /// child's create form); `components` are those of a stored row
    /// (vocabulary 16).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_components: Option<Vec<ComponentSchema>>,
    /// Fewest rows.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_items: Option<u32>,
    /// Most rows.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_items: Option<u32>,
    /// The user may reorder the rows.
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub reorderable: bool,
    /// Rows fold.
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub collapsible: bool,
    /// Rows start folded.
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub collapsed: bool,
    /// Each row's heading: `{input}` placeholders.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_label: Option<String>,
    /// The label of the button adding a row (translated).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub add_action_label: Option<String>,
    /// Rows can be added.
    pub addable: bool,
    /// Rows can be removed.
    pub deletable: bool,
    /// Rows a create form starts with.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_items: Option<u32>,
    /// A row can be copied.
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub cloneable: bool,
    /// Relationship repeaters: the child records and what the user may do.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relationship: Option<RepeaterRelationship>,
}

/// A relationship repeater's child records (ADR 0040).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct RepeaterRelationship {
    /// The child resource.
    pub resource: &'static str,
    /// The has-many relation (its list reads the rows).
    pub relation: &'static str,
    /// The user may add rows.
    pub create: bool,
    /// The user may change rows (each row's envelope decides too).
    pub update: bool,
    /// The user may remove rows (each row's envelope decides too).
    pub delete: bool,
    /// The child field holding each row's position (rows are shown in its order).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_column: Option<String>,
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
    /// Sections the user may fold: whether it starts folded (vocabulary 11).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collapsed: Option<bool>,
    /// A tighter section (vocabulary 11).
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub compact: bool,
    /// Splits: the aside first on narrow screens (vocabulary 11).
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub aside_first: bool,
    /// Shown, with everything in it, only when this holds (vocabulary 13).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shown_when: Option<Condition>,
}

/// An element of a form or infolist: input, layout or entry, told apart by `type`.
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
    /// Infolist entry (vocabulary 14).
    Entry(EntrySchema),
}

/// An entry of an infolist (vocabulary 14, ADR 0039): how a value is shown
/// on a view page. Options absent when unset.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct EntrySchema {
    /// Entry type.
    #[serde(rename = "type")]
    pub kind: EntryType,
    /// Field (or computed value, or key of a panel page's data).
    pub name: String,
    /// Label (translated).
    pub label: String,
    /// Wire kind of the value (`json` for computed values and panel pages).
    pub value_type: &'static str,
    /// Columns spanned.
    pub column_span: u8,
    /// Help text (translated).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
    /// Shown only when this holds for the stored values.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shown_when: Option<Condition>,
    /// Shown as a badge.
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub badge: bool,
    /// Colors by value (badges, booleans).
    #[cfg_attr(feature = "ts", ts(as = "Option<Vec<BadgeColor>>", optional))]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub colors: Vec<BadgeColor>,
    /// A button copies the value.
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub copyable: bool,
    /// Most characters shown.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// The value is a link (`http`, `https` only).
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub url: bool,
    /// A number with this many decimals.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decimals: Option<u8>,
    /// One item per line.
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub list: bool,
    /// Line breaks kept.
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub multiline: bool,
    /// Masked.
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub password: bool,
    /// Fixed text before the value.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    /// Fixed text after the value.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suffix: Option<String>,
    /// Labels by value (translated).
    #[cfg_attr(feature = "ts", ts(as = "Option<Vec<SelectOption>>", optional))]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<SelectOption>,
    /// Labels of a boolean entry, `[yes, no]` (translated).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<[String; 2]>,
    /// Thumbnail size in pixels.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u16>,
    /// Images open in a viewer.
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub gallery: bool,
    /// The document's headings as an outline.
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub outline: bool,
    /// Rich text entries: the application nodes the document may hold
    /// (vocabulary 18).
    #[cfg_attr(feature = "ts", ts(as = "Option<Vec<RichNodeSchema>>", optional))]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rich_nodes: Vec<RichNodeSchema>,
    /// Related resource (relation entries; the title is the record's `labels[name]`).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relationship: Option<Relationship>,
    /// The value is computed on the server (`records/{key}/computed`), not in the record's data.
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub computed: bool,
    /// Relation entries: the title links to the related record (the user may list its resource).
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub link: bool,
    /// Custom entries: the script drawing the value (`vendor/name`).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub widget: Option<String>,
    /// Settings of the custom entry.
    #[cfg_attr(feature = "ts", ts(optional, type = "JsonValue"))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub props: Option<serde_json::Value>,
    /// Repeatable entries: the entries of each row (vocabulary 15).
    #[cfg_attr(feature = "ts", ts(as = "Option<Vec<ComponentSchema>>", optional))]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub components: Vec<ComponentSchema>,
}

/// A record's view page (vocabulary 14, ADR 0039).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct InfolistSchema {
    /// Columns.
    pub columns: u8,
    /// Entries and layouts the user may see.
    pub components: Vec<ComponentSchema>,
    /// How wide the view page is (absent: the default).
    #[cfg_attr(feature = "ts", ts(as = "Option<FormWidth>", optional))]
    #[serde(skip_serializing_if = "FormWidth::is_default")]
    pub width: FormWidth,
    /// Made from the record's form (the resource declares no `infolist()`): its
    /// conditions read fields as the edit form does, a declared infolist's the
    /// stored values (vocabulary 17).
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    #[serde(rename = "fromForm", skip_serializing_if = "std::ops::Not::not")]
    pub from_form: bool,
}

/// `GET records/{key}/computed`: the computed values of a view page.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ComputedValues {
    /// Values by name.
    #[cfg_attr(feature = "ts", ts(type = "Record<string, JsonValue>"))]
    pub data: crate::Data,
    /// Names that failed (error, timeout, too large): shown as not available.
    pub failed: Vec<String>,
}

/// `POST form/react`: the form's state when an input with a reaction changed
/// (vocabulary 19, ADR 0043).
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct FormReactRequest {
    /// `create` or `edit` (`view` is refused).
    pub operation: SchemaOperation,
    /// The record's key (`edit` only).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(default)]
    pub key: Option<String>,
    /// The input that changed (one with `reacts`).
    pub changed: String,
    /// The form's values, in wire format (as a save sends them).
    #[cfg_attr(feature = "ts", ts(type = "Record<string, JsonValue>"))]
    pub state: crate::Data,
    /// The changed input's value when the panel last applied an answer (or
    /// opened the form).
    #[cfg_attr(feature = "ts", ts(optional, type = "JsonValue"))]
    #[serde(default)]
    pub previous: Option<serde_json::Value>,
    /// A create form opened through a relation of another record: the
    /// reaction sees the key that relation binds, as the save does.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(default)]
    pub via: Option<FormReactVia>,
    /// Sent back unchanged: the panel applies only its latest request's answer.
    pub seq: u32,
}

/// The relation a create form was opened through (`relations/{relation}` of
/// the record `record` of `parent`).
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct FormReactVia {
    /// The parent resource.
    pub parent: String,
    /// The parent record's key.
    pub record: String,
    /// The relation's name.
    pub relation: String,
}

/// The answer to a [`FormReactRequest`].
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FormReactResponse {
    /// The request's `seq`.
    pub seq: u32,
    /// The changes to apply, in order (messages translated).
    pub patches: Vec<crate::ui::FormPatch>,
    /// The reaction failed (error, timeout): nothing to apply.
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub failed: bool,
}

/// A form.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FormSchema {
    /// Columns.
    pub columns: u8,
    /// Components the user may see for the operation.
    pub components: Vec<ComponentSchema>,
    /// How wide the record's page is (absent: the default; vocabulary 11).
    #[cfg_attr(feature = "ts", ts(as = "Option<FormWidth>", optional))]
    #[serde(skip_serializing_if = "FormWidth::is_default")]
    pub width: FormWidth,
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
    /// Labels shown for values of a text or badge column, in the user's
    /// language (the column's own, or its form select's; vocabulary 20).
    #[cfg_attr(feature = "ts", ts(as = "Option<Vec<SelectOption>>", optional))]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<SelectOption>,
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
    /// Shows the number of items of the field's JSON list (vocabulary 15).
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub count: bool,
    /// Shows `counts[relation]` of each record: how many records of this
    /// has-many relation it has that the user may list (vocabulary 15). The
    /// column's `name` is not a field.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counts: Option<&'static str>,
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
    /// Label of the button that runs it from its form dialog (default: the
    /// action's label without a trailing ellipsis).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub submit_label: Option<String>,
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
    /// The view page's entries, for `operation=view` of a resource that
    /// declares them (vocabulary 14); absent: the form is shown read-only.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub infolist: Option<InfolistSchema>,
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
