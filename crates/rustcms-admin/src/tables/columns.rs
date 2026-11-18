//! Table columns (`TextColumn`, `BadgeColumn`, …).

use serde_json::Value as Json;

use crate::forms::SelectOption;

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

/// A screen width under which a column is hidden ([`TableColumn::hide_below`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Breakpoint {
    /// 640 px.
    Sm,
    /// 768 px.
    Md,
    /// 1024 px.
    Lg,
}

/// A table column bound to a field.
#[derive(Debug, Clone)]
pub struct TableColumn {
    pub(crate) name: String,
    pub(crate) kind: ColumnType,
    pub(crate) label: Option<String>,
    pub(crate) colors: Vec<(Json, String)>,
    pub(crate) options: Vec<SelectOption>,
    pub(crate) raw: bool,
    pub(crate) date_format: Option<String>,
    pub(crate) cell: Option<String>,
    pub(crate) props: Option<Json>,
    pub(crate) hide_below: Option<Breakpoint>,
    pub(crate) count: bool,
    pub(crate) counts: Option<&'static str>,
}

impl TableColumn {
    fn new(name: impl Into<String>, kind: ColumnType) -> Self {
        Self {
            name: name.into(),
            kind,
            label: None,
            colors: Vec::new(),
            options: Vec::new(),
            raw: false,
            date_format: None,
            cell: None,
            props: None,
            hide_below: None,
            count: false,
            counts: None,
        }
    }

    /// Drawn by the cell an extension script registered as `vendor/name`
    /// (`window.rustcms.registerCell`); as its column type otherwise.
    pub fn cell(mut self, name: &str) -> Self {
        self.cell = Some(name.to_owned());
        self
    }

    /// Settings passed to the custom cell (JSON, at most 4 KiB).
    pub fn props(mut self, props: Json) -> Self {
        self.props = Some(props);
        self
    }

    /// Hidden on screens narrower than `breakpoint`, by static CSS classes
    /// (no inline style). Presentation only: the value is still in each
    /// record and on its page.
    pub fn hide_below(mut self, breakpoint: Breakpoint) -> Self {
        self.hide_below = Some(breakpoint);
        self
    }

    /// Header label (default: the field name, humanized).
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Shows how many items the field's JSON list holds (a repeater, a tags
    /// list) instead of the items. The field needs a JSON column.
    pub fn count(mut self) -> Self {
        self.count = true;
        self
    }

    /// Shows how many records of the has-many `relation` each record has,
    /// among those the user may list (its scope and policy), as Filament's
    /// `counts()`. The column's name is its own (`lines_count`), not a field;
    /// not sortable. Shown only to users who may list the relation.
    pub fn counts(mut self, relation: &'static str) -> Self {
        self.counts = Some(relation);
        self
    }

    /// Badge color for `value` (`primary`, `success`, `warning`, `danger`, `gray`).
    /// The value is compared in canonical wire form, as a select's options
    /// (an `i64` or a decimal as a string); a value the column cannot hold
    /// refuses to boot.
    pub fn color(mut self, value: impl Into<Json>, color: impl Into<String>) -> Self {
        self.colors.push((value.into(), color.into()));
        self
    }

    /// Labels shown for values of a text or badge column (translated as other
    /// labels). Without them, the column takes the options of the form's
    /// select or checkbox list of the same field, if any ([`Self::raw`] keeps
    /// the values). Colors, sorting, search and exports still use the values;
    /// a value without a label is shown as it is.
    pub fn options<V: Into<Json>, L: Into<String>>(mut self, options: impl IntoIterator<Item = (V, L)>) -> Self {
        self.options = options.into_iter().map(|(v, l)| SelectOption { value: v.into(), label: l.into() }).collect();
        self
    }

    /// Shows the dates or times of the field (a date, local date-time or
    /// instant) with `pattern`, e.g. `"HH:mm dd/MM/yyyy"`: `yyyy` `yy`, `MMMM`
    /// `MMM` `MM` `M` (month names in the user's language), `dd` `d`, `EEEE`
    /// `EEE` (weekday), `HH` `H` (0–23), `hh` `h` (1–12), `mm`, `ss`, `a`
    /// (AM/PM), `z` (time zone); text between single quotes. Instants are
    /// shown in the user's time zone, dates and local date-times as they are.
    /// A pattern that does not fit the field (a time on a date, `z` on a local
    /// date-time) refuses to boot. Default: the panel's
    /// ([`crate::AdminProvider::date_format`]).
    pub fn date_format(mut self, pattern: impl Into<String>) -> Self {
        self.date_format = Some(pattern.into());
        self
    }

    /// Shows the values themselves, never the labels of the form's options.
    pub fn raw(mut self) -> Self {
        self.raw = true;
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
    /// Text column (option labels: [`TableColumn::options`]).
    TextColumn => Text,
    /// Badge column ([`TableColumn::color`] per value; option labels:
    /// [`TableColumn::options`]).
    BadgeColumn => Badge,
    /// Boolean column.
    BooleanColumn => Boolean,
    /// Date column: instants in the user's time zone, dates and local
    /// date-times as they are ([`TableColumn::date_format`]).
    DateColumn => Date,
}
