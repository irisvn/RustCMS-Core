//! Table columns (`TextColumn`, `BadgeColumn`, …).

use serde_json::Value as Json;

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
    pub(crate) cell: Option<String>,
    pub(crate) props: Option<Json>,
    pub(crate) hide_below: Option<Breakpoint>,
}

impl TableColumn {
    fn new(name: impl Into<String>, kind: ColumnType) -> Self {
        Self { name: name.into(), kind, label: None, colors: Vec::new(), cell: None, props: None, hide_below: None }
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
