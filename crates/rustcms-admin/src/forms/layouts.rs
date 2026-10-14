//! Form layouts: `Section`, `Grid`, `Tabs`, `Tab`.

use super::Component;

/// Layout kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum LayoutType {
    /// Titled group.
    Section,
    /// Columns without a title.
    Grid,
    /// Tabs: every child is a [`LayoutType::Tab`] (vocabulary 2).
    Tabs,
    /// One tab of [`LayoutType::Tabs`], with a label and a stable `id`.
    Tab,
}

/// A layout grouping components.
#[derive(Debug, Clone)]
pub struct Layout {
    pub(crate) kind: LayoutType,
    pub(crate) id: Option<String>,
    pub(crate) label: Option<String>,
    pub(crate) columns: u8,
    pub(crate) children: Vec<Component>,
}

impl Layout {
    /// Number of columns.
    pub fn columns(mut self, columns: u8) -> Self {
        self.columns = columns.max(1);
        self
    }

    /// Child components.
    pub fn schema(mut self, children: Vec<Component>) -> Self {
        self.children = children;
        self
    }
}

/// Titled section.
pub struct Section;

impl Section {
    /// Section titled `label`.
    #[allow(clippy::new_ret_no_self)]
    pub fn make(label: impl Into<String>) -> Layout {
        Layout { kind: LayoutType::Section, id: None, label: Some(label.into()), columns: 1, children: Vec::new() }
    }
}

/// Untitled grid.
pub struct Grid;

impl Grid {
    /// Grid with `columns` columns.
    #[allow(clippy::new_ret_no_self)]
    pub fn make(columns: u8) -> Layout {
        Layout { kind: LayoutType::Grid, id: None, label: None, columns: columns.max(1), children: Vec::new() }
    }
}

/// Tabs; children are [`Tab`]s.
pub struct Tabs;

impl Tabs {
    /// Empty tabs: add [`Tab`]s with `.schema(components![…])`.
    #[allow(clippy::new_ret_no_self)]
    pub fn make() -> Layout {
        Layout { kind: LayoutType::Tabs, id: None, label: None, columns: 1, children: Vec::new() }
    }
}

/// One tab, identified by its label (lowercased, non-alphanumerics as `-`).
pub struct Tab;

impl Tab {
    /// Tab titled `label`.
    #[allow(clippy::new_ret_no_self)]
    pub fn make(label: impl Into<String>) -> Layout {
        let label = label.into();
        let id = label
            .to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("-");
        Layout { kind: LayoutType::Tab, id: Some(id), label: Some(label), columns: 1, children: Vec::new() }
    }
}
