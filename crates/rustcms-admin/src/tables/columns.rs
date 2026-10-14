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
