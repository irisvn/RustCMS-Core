//! A table: columns, filters, default sort.

use super::{TableColumn, TableFilter};

/// A table. An empty table is generated from the resource's fields.
#[derive(Debug, Clone, Default)]
pub struct Table {
    pub(crate) columns: Vec<TableColumn>,
    pub(crate) filters: Vec<TableFilter>,
    pub(crate) default_sort: Option<String>,
}

impl Table {
    /// Empty table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Columns.
    pub fn columns(mut self, columns: Vec<TableColumn>) -> Self {
        self.columns = columns;
        self
    }

    /// Filters.
    pub fn filters(mut self, filters: Vec<TableFilter>) -> Self {
        self.filters = filters;
        self
    }

    /// Default sort, e.g. `-created_at` (the field must be `.sortable()`).
    pub fn default_sort(mut self, sort: impl Into<String>) -> Self {
        self.default_sort = Some(sort.into());
        self
    }
}
