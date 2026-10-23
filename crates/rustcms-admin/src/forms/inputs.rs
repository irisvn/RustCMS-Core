//! Form inputs (`TextInput`, `SelectInput`, `FileUpload`, `CustomInput`, …).

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
    /// A secret setting: shows whether it is set, never its value; replaced
    /// or cleared explicitly (vocabulary 5). Also an encrypted field
    /// (`Field::encrypted`, vocabulary 8).
    Secret,
    /// Named secrets of a `Field::encrypted_map` field: the names, never the
    /// values; each kept, replaced or removed (vocabulary 8).
    SecretMap,
    /// The links of a belongs-to-many relation, chosen in the record's own
    /// form; the value is a change, `{attach?: [keys], detach?: [keys]}`
    /// (vocabulary 9, ADR 0035).
    BelongsToMany,
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
    /// Secret setting (`SettingField::secret`) or encrypted field
    /// (`Field::encrypted`): set, replace or clear, never shown.
    SecretInput => Secret,
    /// Named secrets of a `Field::encrypted_map` field.
    SecretMapInput => SecretMap,
    /// Several records of a belongs-to-many relation, linked from the
    /// record's own form: `BelongsToManySelect::make("tags")` names a
    /// `Relation::belongs_to_many` of the resource (ADR 0035).
    BelongsToManySelect => BelongsToMany,
}
