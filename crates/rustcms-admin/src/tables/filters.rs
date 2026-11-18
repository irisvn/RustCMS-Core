//! Table filters (`SelectFilter`, `TernaryFilter`, `DateRangeFilter`).

use serde_json::Value as Json;

use crate::forms::SelectOption;

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
    /// Label of the "no value" option (select filters on a nullable column).
    pub(crate) none: Option<String>,
}

/// The value of a select filter's "no value" option in `in` lists
/// ([`TableFilter::none`]): `filter[collection][in]=~none,covers`.
pub const NONE_OPTION: &str = "~none";

impl TableFilter {
    fn new(name: impl Into<String>, kind: FilterType) -> Self {
        Self { name: name.into(), kind, label: None, options: Vec::new(), none: None }
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

    /// Adds an option, shown last with `label`, for records without a value
    /// (a select filter on a nullable column): its value is [`NONE_OPTION`]
    /// in `in` lists, matched as `IS NULL` (with the other values chosen, any
    /// of them). Without it, `~none` is compared as any other value.
    ///
    /// # Panics
    /// At boot, on a filter that is not a select filter, a column that is not
    /// nullable, or a `belongs_to` field (its filter picks records).
    pub fn none(mut self, label: impl Into<String>) -> Self {
        self.none = Some(label.into());
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
