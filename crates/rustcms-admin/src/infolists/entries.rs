//! Entries: how a value is shown on a record's view page (`TextEntry`,
//! `RelationEntry`, `ComputedEntry`, `CustomEntry`, …).

use serde_json::Value as Json;

use crate::ui::SelectOption;

/// Entry kinds of the infolist vocabulary (vocabulary 14).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum EntryType {
    /// Text, numbers, dates, option labels, lists.
    TextEntry,
    /// The title of the related record of a `belongs_to` field.
    RelationEntry,
    /// Yes or no.
    BooleanEntry,
    /// The pairs of a key/value field.
    KeyValueEntry,
    /// Thumbnails of a media field.
    MediaEntry,
    /// A rich text document.
    RichTextEntry,
    /// A value computed on the server (`Resource::computed`).
    ComputedEntry,
    /// Drawn by an extension script (`registerEntry`).
    CustomEntry,
    /// The rows of a JSON repeater, each shown by entries (vocabulary 15).
    RepeatableEntry,
}

/// How an entry shows its value. Options that do not fit the entry or its
/// field are refused at boot.
#[derive(Debug, Clone)]
pub struct Entry {
    pub(crate) name: String,
    pub(crate) kind: EntryType,
    pub(crate) label: Option<String>,
    pub(crate) help: Option<String>,
    pub(crate) column_span: u8,
    /// When the entry is shown, on the stored values (ADR 0037).
    pub(crate) shown_when: Option<Box<crate::ui::Condition>>,
    pub(crate) display: Box<Display>,
    /// Repeatable entries: the entries of each row (keys of the row).
    pub(crate) components: Vec<crate::ui::Component>,
    /// Entries of a row generated from a repeater's inputs: the kind of the
    /// row value (dates shown as dates); declared row entries read JSON.
    pub(crate) row_kind: Option<crate::wire::FieldKind>,
    /// Repeatable entries: the rows are the records of the has-many relation
    /// of the entry's name (ADR 0044).
    pub(crate) relationship: bool,
    /// Rich text entries of a JSON row made from a rich editor: the
    /// application nodes the input accepts (ADR 0044).
    pub(crate) row_nodes: Vec<String>,
    /// Relationship entries of the default view page: the order column of
    /// the form's repeater, wherever it is declared (rows of rows, ADR 0044).
    pub(crate) order_by: Option<String>,
}

/// Display options (boxed: most entries have few).
#[derive(Debug, Clone, Default)]
pub(crate) struct Display {
    pub(crate) badge: bool,
    pub(crate) colors: Vec<(Json, String)>,
    pub(crate) copyable: bool,
    pub(crate) limit: Option<u32>,
    pub(crate) url: bool,
    pub(crate) decimals: Option<u8>,
    pub(crate) list: bool,
    pub(crate) multiline: bool,
    pub(crate) password: bool,
    pub(crate) prefix: Option<String>,
    pub(crate) suffix: Option<String>,
    pub(crate) options: Vec<SelectOption>,
    pub(crate) labels: Option<(String, String)>,
    pub(crate) size: Option<u16>,
    /// `Some(false)`: images do not open in the viewer (media entries).
    pub(crate) preview: Option<bool>,
    pub(crate) date_format: Option<String>,
    pub(crate) outline: bool,
    pub(crate) widget: Option<String>,
    pub(crate) props: Option<Json>,
}

impl Entry {
    pub(crate) fn new(name: impl Into<String>, kind: EntryType) -> Self {
        Self {
            name: name.into(),
            kind,
            label: None,
            help: None,
            column_span: 1,
            shown_when: None,
            display: Box::default(),
            components: Vec::new(),
            row_kind: None,
            relationship: false,
            row_nodes: Vec::new(),
            order_by: None,
        }
    }

    /// Repeatable entries: the rows are the records of the has-many relation
    /// of the entry's name, read as its section reads them (the child's
    /// scope, view policy and read rules, the relation's filters); each row's
    /// entries name fields of the child. At most [`Entry::limit`] rows (100
    /// by default and at most), then a link to the relation's section.
    pub fn relationship(mut self) -> Self {
        self.relationship = true;
        self
    }

    /// Displayed label (default: the field's label).
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Help text under the value.
    pub fn help(mut self, text: impl Into<String>) -> Self {
        self.help = Some(text.into());
        self
    }

    /// Columns the entry spans in its grid.
    pub fn column_span(mut self, span: u8) -> Self {
        self.column_span = span.max(1);
        self
    }

    /// Shows the entry only when `condition` holds for the stored values
    /// (ADR 0037). It names other entries of the infolist; presentation only:
    /// every readable value is sent with the record anyway.
    pub fn shown_when(mut self, condition: crate::ui::Condition) -> Self {
        self.shown_when = Some(Box::new(condition));
        self
    }

    /// Shows the value as a colored badge (colors from [`Entry::color`]).
    pub fn badge(mut self) -> Self {
        self.display.badge = true;
        self
    }

    /// The color of the badge (text entries) or of the value (boolean
    /// entries) for `value`: `primary`, `info`, `success`, `warning`, `danger` or
    /// `gray` (the default).
    pub fn color(mut self, value: impl Into<Json>, color: impl Into<String>) -> Self {
        self.display.colors.push((value.into(), color.into()));
        self
    }

    /// A button copies the value.
    pub fn copyable(mut self) -> Self {
        self.display.copyable = true;
        self
    }

    /// Shows at most `chars` characters, then `…` (the whole text in a
    /// tooltip). Relationship repeatable entries: at most this many rows.
    pub fn limit(mut self, chars: u32) -> Self {
        self.display.limit = Some(chars.max(1));
        self
    }

    /// The value is a link (`http` and `https` only; anything else is text).
    pub fn url(mut self) -> Self {
        self.display.url = true;
        self
    }

    /// A number with `decimals` decimals, grouped in the user's language.
    pub fn number(mut self, decimals: u8) -> Self {
        self.display.decimals = Some(decimals);
        self
    }

    /// A list, one item per line.
    pub fn list(mut self) -> Self {
        self.display.list = true;
        self
    }

    /// Keeps the line breaks of the text.
    pub fn multiline(mut self) -> Self {
        self.display.multiline = true;
        self
    }

    /// Masks the value (`••••••••`).
    pub fn password(mut self) -> Self {
        self.display.password = true;
        self
    }

    /// Fixed text before the value, shown as written.
    pub fn prefix(mut self, text: impl Into<String>) -> Self {
        self.display.prefix = Some(text.into());
        self
    }

    /// Fixed text after the value (a unit), shown as written.
    pub fn suffix(mut self, text: impl Into<String>) -> Self {
        self.display.suffix = Some(text.into());
        self
    }

    /// Labels shown for values (translated); other values as written.
    pub fn options<V: Into<Json>, L: Into<String>>(mut self, options: impl IntoIterator<Item = (V, L)>) -> Self {
        self.display.options = options
            .into_iter()
            .map(|(value, label)| SelectOption { value: value.into(), label: label.into() })
            .collect();
        self
    }

    /// Labels of a boolean entry (default: Yes and No, translated).
    pub fn labels(mut self, yes: impl Into<String>, no: impl Into<String>) -> Self {
        self.display.labels = Some((yes.into(), no.into()));
        self
    }

    /// Thumbnail size in pixels (media entries; default 80).
    pub fn size(mut self, pixels: u16) -> Self {
        self.display.size = Some(pixels.clamp(16, 512));
        self
    }

    /// Images of a media entry open in the panel's viewer when clicked
    /// (default: `true`; zoom, the entry's other images, download).
    pub fn preview(mut self, preview: bool) -> Self {
        self.display.preview = Some(preview);
        self
    }

    /// Images open when clicked (media entries): now the default, in the
    /// panel's viewer; kept for applications written before it.
    #[deprecated(note = "images open in the panel's viewer by default; `.preview(false)` turns it off")]
    pub fn gallery(self) -> Self {
        self
    }

    /// Shows the dates or times of the field with `pattern` (text entries of a
    /// date, local date-time or instant field), as
    /// [`crate::ui::TableColumn::date_format`].
    pub fn date_format(mut self, pattern: impl Into<String>) -> Self {
        self.display.date_format = Some(pattern.into());
        self
    }

    /// Shows the document's headings as an outline (rich text entries).
    pub fn outline(mut self) -> Self {
        self.display.outline = true;
        self
    }

    /// Repeatable entries: how each row shows (`TextEntry`, `BooleanEntry`,
    /// `KeyValueEntry`, `CustomEntry`, `RepeatableEntry`, naming keys of the row).
    pub fn schema(mut self, components: Vec<crate::ui::Component>) -> Self {
        self.components = components;
        self
    }

    /// Settings passed to a custom entry (JSON, at most 4 KiB).
    pub fn props(mut self, props: Json) -> Self {
        self.display.props = Some(props);
        self
    }
}

/// An entry drawn by an extension script that registered `vendor/name` with
/// `registerEntry`; the value as text when no script draws it.
pub struct CustomEntry;

impl CustomEntry {
    /// Field (or computed value) `name` drawn by `widget` (`vendor/name`).
    #[allow(clippy::new_ret_no_self)]
    pub fn make(name: impl Into<String>, widget: &str) -> Entry {
        let mut entry = Entry::new(name, EntryType::CustomEntry);
        entry.display.widget = Some(widget.to_owned());
        entry
    }
}

macro_rules! entry_types {
    ($($(#[$doc:meta])* $name:ident => $kind:ident),* $(,)?) => {$(
        $(#[$doc])*
        pub struct $name;

        impl $name {
            /// Entry for `name`.
            #[allow(clippy::new_ret_no_self)]
            pub fn make(name: impl Into<String>) -> Entry {
                Entry::new(name, EntryType::$kind)
            }
        }
    )*};
}

entry_types! {
    /// Text: numbers, dates (in the user's language and time zone, or with
    /// [`Entry::date_format`]), option labels, lists.
    TextEntry => TextEntry,
    /// The title of the related record of a `belongs_to` field, linked to
    /// its page when the user may list the related resource.
    RelationEntry => RelationEntry,
    /// Yes or no.
    BooleanEntry => BooleanEntry,
    /// The pairs of a key/value field.
    KeyValueEntry => KeyValueEntry,
    /// Thumbnails of a `Field::media` / `Field::media_list` field.
    MediaEntry => MediaEntry,
    /// The document of a `Field::rich_text` field.
    RichTextEntry => RichTextEntry,
    /// A value of `Resource::computed`, computed when the view page opens.
    ComputedEntry => ComputedEntry,
    /// The rows of a JSON repeater field, each shown by the entries of its
    /// `schema` (ADR 0040).
    RepeatableEntry => RepeatableEntry,
}
