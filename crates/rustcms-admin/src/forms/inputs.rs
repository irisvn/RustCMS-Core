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
    /// Rows of inputs, in a JSON column or as child records (vocabulary 15,
    /// ADR 0040).
    Repeater,
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

/// Bounds and step of a number input (`NumberInput::make(…).min(…).max(…).step(…)`).
#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct NumberBounds {
    /// Smallest value a client may write.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub min: Option<f64>,
    /// Largest value a client may write.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub max: Option<f64>,
    /// Increment of the input's arrows and keyboard (a hint, not checked).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub step: Option<f64>,
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
    pub(crate) prefix: Option<String>,
    pub(crate) suffix: Option<String>,
    /// Boxed: most inputs have none, and forms hold many inputs.
    pub(crate) number: Option<Box<NumberBounds>>,
    /// When the input is shown (ADR 0037); boxed for the same reason.
    pub(crate) shown_when: Option<Box<super::Condition>>,
    /// Hidden after a write: the field takes its cleared value.
    pub(crate) clear_when_hidden: bool,
    /// Repeaters: their rows (boxed: most inputs are not repeaters).
    pub(crate) repeater: Option<Box<super::repeater::RepeaterSpec>>,
    /// The input's reaction (ADR 0043; boxed: most inputs have none).
    pub(crate) reaction: Option<Box<super::reactions::Reaction>>,
    /// Rules computed on the server (ADR 0044, part E; boxed: most inputs have none).
    pub(crate) rules: Option<Box<super::rules::InputRules>>,
    /// JSON repeater rows: rules a field gives elsewhere.
    pub(crate) required: bool,
    pub(crate) max_length: Option<usize>,
    pub(crate) integer: bool,
    /// File uploads: images open in the panel's viewer (the default).
    pub(crate) preview: bool,
    /// File uploads and rich editors in rows (ADR 0044): rules of JSON rows
    /// and the toolbar (boxed: most inputs have none).
    pub(crate) content: Option<Box<RowContent>>,
}

/// What a file upload or a rich editor of a repeater's row declares (ADR
/// 0044): in a JSON row, the rules a field gives elsewhere; in any row, the
/// rich editor's toolbar.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct RowContent {
    /// Accepted MIME types (`image/*`), JSON rows.
    pub(crate) accept: Vec<String>,
    /// Largest file in bytes, JSON rows.
    pub(crate) max_size: Option<u64>,
    /// A list of up to this many files (else one), JSON rows.
    pub(crate) max_files: Option<usize>,
    /// The media collection of uploads, JSON rows.
    pub(crate) collection: Option<String>,
    /// Application nodes a rich editor accepts, JSON rows.
    pub(crate) nodes: Vec<String>,
    /// A rich editor in a row shows the full toolbar (else the compact one).
    pub(crate) full: bool,
}

impl RowContent {
    /// The rule a file upload of a JSON row holds its files to.
    pub(crate) fn media_rule(&self) -> crate::field::MediaRule {
        crate::field::MediaRule {
            multiple: self.max_files.is_some(),
            max_items: self.max_files.unwrap_or(1),
            accept: self.accept.clone(),
            max_size: self.max_size,
        }
    }

    /// The toolbar of a rich editor in a row: compact unless `full` (ADR 0044).
    pub(crate) fn toolbar(content: Option<&Self>) -> Option<crate::schema::RichToolbar> {
        (!content.is_some_and(|c| c.full)).then_some(crate::schema::RichToolbar::Compact)
    }

    /// Whether a rule meant for JSON rows is set (everything but `full`).
    pub(crate) fn has_rules(&self) -> bool {
        !self.accept.is_empty()
            || self.max_size.is_some()
            || self.max_files.is_some()
            || self.collection.is_some()
            || !self.nodes.is_empty()
    }
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
            prefix: None,
            suffix: None,
            number: None,
            shown_when: None,
            clear_when_hidden: false,
            repeater: None,
            reaction: None,
            rules: None,
            required: false,
            max_length: None,
            integer: false,
            preview: true,
            content: None,
        }
    }

    fn content(&mut self) -> &mut RowContent {
        self.content.get_or_insert_with(Box::default)
    }

    /// File uploads in JSON repeater rows: accepted MIME types (`image/*`,
    /// `application/pdf`). Elsewhere the field decides (`Field::accept`).
    pub fn accept(mut self, types: &[&str]) -> Self {
        self.content().accept = types.iter().map(|t| (*t).to_owned()).collect();
        self
    }

    /// File uploads in JSON repeater rows: the largest file, in bytes.
    pub fn max_size(mut self, bytes: u64) -> Self {
        self.content().max_size = Some(bytes);
        self
    }

    /// File uploads in JSON repeater rows: a list of up to `max` files, in
    /// order (else one file).
    pub fn multiple(mut self, max: usize) -> Self {
        self.content().max_files = Some(max.max(1));
        self
    }

    /// File uploads and rich editors in JSON repeater rows: uploads go into
    /// the media collection `name` (declared with
    /// [`crate::AdminProvider::media_collection`]), and the picker opens on it.
    pub fn collection(mut self, name: impl Into<String>) -> Self {
        self.content().collection = Some(name.into());
        self
    }

    /// Rich editors in JSON repeater rows: the application nodes the document
    /// accepts (declared with [`crate::AdminProvider::rich_text_node`]).
    pub fn nodes<S: Into<String>>(mut self, names: impl IntoIterator<Item = S>) -> Self {
        self.content().nodes.extend(names.into_iter().map(Into::into));
        self
    }

    /// Rich editors in repeater rows: the full toolbar (rows show a compact
    /// one: bold, italic, underline, superscript, subscript, images and
    /// application elements). The document's rules are the same either way.
    pub fn full(mut self) -> Self {
        self.content().full = true;
        self
    }

    /// Displayed label (default: the field name, humanized).
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Fixed text shown inside the input before the value (`/posts/` before a
    /// slug): never part of the value, shown as written (not translated),
    /// read by screen readers with the input. Text inputs only.
    pub fn prefix(mut self, text: impl Into<String>) -> Self {
        self.prefix = Some(text.into());
        self
    }

    /// Fixed text shown inside the input after the value (`.html`, `km`).
    /// Text inputs only.
    pub fn suffix(mut self, text: impl Into<String>) -> Self {
        self.suffix = Some(text.into());
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

    /// Static options (selects, checkbox lists). On a resource they bound what
    /// clients write: one of them for a select, a list of them (each once) for
    /// a checkbox list, which needs a JSON column. Select values must be
    /// values of the field's column; they are compared, and shown, in
    /// canonical wire form. Not used for `belongs_to` fields.
    pub fn options<V: Into<Json>, L: Into<String>>(mut self, options: impl IntoIterator<Item = (V, L)>) -> Self {
        self.options = options.into_iter().map(|(v, l)| SelectOption { value: v.into(), label: l.into() }).collect();
        self
    }

    /// Whether the images of a file upload open in the panel's viewer when
    /// clicked (default: `true`); also the entry its default view page shows.
    /// Other inputs refuse it at boot.
    pub fn preview(mut self, preview: bool) -> Self {
        self.preview = preview;
        self
    }

    /// Columns of the surrounding grid this input spans.
    pub fn column_span(mut self, span: u8) -> Self {
        self.column_span = span.max(1);
        self
    }

    /// Smallest value of a number input. Like a select's options, it bounds
    /// what clients write to the field (`must be at least {min}`); hooks may
    /// write any value.
    pub fn min(mut self, value: impl Into<f64>) -> Self {
        self.number.get_or_insert_with(Box::default).min = Some(value.into());
        self
    }

    /// Largest value of a number input; bounds client writes like
    /// [`Input::min`] (`may not be greater than {max}`).
    pub fn max(mut self, value: impl Into<f64>) -> Self {
        self.number.get_or_insert_with(Box::default).max = Some(value.into());
        self
    }

    /// Step of a number input's arrows and keyboard (`1` by default for
    /// integers, `any` otherwise). A presentation hint: not checked.
    pub fn step(mut self, value: impl Into<f64>) -> Self {
        self.number.get_or_insert_with(Box::default).step = Some(value.into());
        self
    }

    /// Shows the input only when `condition` holds (ADR 0037): the panel
    /// hides it as the user types, and the server ignores a value sent for it
    /// while hidden. `required` then means "required when shown".
    ///
    /// ```rust,ignore
    /// TextInput::make("url").shown_when(when("link_type").eq("external"))
    /// ```
    pub fn shown_when(mut self, condition: super::Condition) -> Self {
        self.shown_when = Some(Box::new(condition));
        self
    }

    /// When the input is hidden after a write, sets its field to the
    /// column's declared default, else `null`, else an empty text or list
    /// (instead of keeping the stored value). For inputs with a condition.
    pub fn clear_when_hidden(mut self) -> Self {
        self.clear_when_hidden = true;
        self
    }

    /// Runs `react` on the server when the user changes this input (after a
    /// short pause in typing), and applies the changes it answers with (ADR
    /// 0041). It sees the fields the user may read; the changes are checked
    /// as the user's own input would be, and the save is checked anyway.
    ///
    /// ```rust,ignore
    /// TextInput::make("name").reacts(|_ctx, state| {
    ///     let slug = slugify(state.text("name"));
    ///     Box::pin(async move { Ok(vec![FormPatch::set_if_empty("slug", slug)]) })
    /// })
    /// ```
    pub fn reacts<F>(mut self, react: F) -> Self
    where
        F: for<'a> Fn(
                &'a crate::Ctx,
                &'a super::FormState,
            ) -> crate::BoxFuture<'a, rustcms_core::AppResult<Vec<super::FormPatch>>>
            + Send
            + Sync
            + 'static,
    {
        self.reaction.get_or_insert_default().run = Some(std::sync::Arc::new(react));
        self
    }

    /// How long the reaction may take (default 5 seconds, at most 30).
    pub fn reaction_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.reaction.get_or_insert_default().timeout = timeout;
        self
    }

    /// Whether the input is shown, computed on the server from the form's
    /// state (ADR 0044, part E): for rules `when(..)` cannot state (they need
    /// code or data). Run when the form is read, when an input of
    /// [`depends_on`](Self::depends_on) changes, and on every save (a hidden
    /// input's value is dropped, as a condition's). With `shown_when`, both must
    /// say shown. Needs `depends_on`.
    ///
    /// ```rust,ignore
    /// TextInput::make("tax_id")
    ///     .depends_on(["country"])
    ///     .shown_when_fn(|_ctx, state| Box::pin(async move { Ok(state.text("country") == "VN") }))
    /// ```
    pub fn shown_when_fn<F>(mut self, rule: F) -> Self
    where
        F: for<'a> Fn(&'a crate::Ctx, &'a super::FormState) -> crate::BoxFuture<'a, rustcms_core::AppResult<bool>>
            + Send
            + Sync
            + 'static,
    {
        self.rules.get_or_insert_default().shown = Some(std::sync::Arc::new(rule));
        self
    }

    /// Whether the input is required (when shown), computed on the server (see
    /// [`shown_when_fn`](Self::shown_when_fn)). Needs `depends_on`.
    pub fn required_fn<F>(mut self, rule: F) -> Self
    where
        F: for<'a> Fn(&'a crate::Ctx, &'a super::FormState) -> crate::BoxFuture<'a, rustcms_core::AppResult<bool>>
            + Send
            + Sync
            + 'static,
    {
        self.rules.get_or_insert_default().required = Some(std::sync::Arc::new(rule));
        self
    }

    /// The options of a select or checkbox list (not `belongs_to`), computed on
    /// the server (see [`shown_when_fn`](Self::shown_when_fn)): a value outside
    /// them is refused on save. Needs `depends_on`.
    pub fn options_fn<F>(mut self, rule: F) -> Self
    where
        F: for<'a> Fn(
                &'a crate::Ctx,
                &'a super::FormState,
            ) -> crate::BoxFuture<'a, rustcms_core::AppResult<Vec<SelectOption>>>
            + Send
            + Sync
            + 'static,
    {
        self.rules.get_or_insert_default().options = Some(std::sync::Arc::new(rule));
        self
    }

    /// The inputs whose change asks the server for this input's rules again
    /// (`shown_when_fn`, `required_fn`, `options_fn`; required with them). An
    /// empty list: computed when the form is read and on save only.
    pub fn depends_on<I, S>(mut self, inputs: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.rules.get_or_insert_default().depends_on = Some(inputs.into_iter().map(Into::into).collect());
        self
    }

    /// Time the input's rules may take together (default 5 seconds, at most 30).
    pub fn rules_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.rules.get_or_insert_default().timeout = timeout;
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
