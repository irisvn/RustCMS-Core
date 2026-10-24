//! Form layouts: `Section`, `Grid`, `Tabs`, `Tab`, `Split`.

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
    /// A main column and a narrow aside (vocabulary 11): its children are
    /// [`LayoutType::Pane`]s, `main` and `aside`; at the form's top level only.
    Split,
    /// One column of a [`LayoutType::Split`], `id` `main` or `aside`.
    Pane,
}

/// A layout grouping components.
#[derive(Debug, Clone)]
pub struct Layout {
    pub(crate) kind: LayoutType,
    pub(crate) id: Option<String>,
    pub(crate) label: Option<String>,
    pub(crate) columns: u8,
    pub(crate) children: Vec<Component>,
    /// Sections: collapsible, and whether it starts collapsed.
    pub(crate) collapsed: Option<bool>,
    /// Sections: tighter, for an aside.
    pub(crate) compact: bool,
    /// Splits: the aside first on narrow screens.
    pub(crate) aside_first: bool,
}

impl Layout {
    fn of(kind: LayoutType, id: Option<String>, label: Option<String>, columns: u8) -> Self {
        Self { kind, id, label, columns, children: Vec::new(), collapsed: None, compact: false, aside_first: false }
    }

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

    /// A section the user may fold and unfold (a button with `aria-expanded`),
    /// starting `collapsed` or open. Nothing is remembered; a folded section
    /// opens by itself when one of its inputs has an error. Sections only.
    pub fn collapsible(mut self, collapsed: bool) -> Self {
        self.collapsed = Some(collapsed);
        self
    }

    /// A tighter section (less padding and spacing), e.g. for the aside of a
    /// [`Split`]. Sections only.
    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
    }
}

/// Titled section.
pub struct Section;

impl Section {
    /// Section titled `label`.
    #[allow(clippy::new_ret_no_self)]
    pub fn make(label: impl Into<String>) -> Layout {
        Layout::of(LayoutType::Section, None, Some(label.into()), 1)
    }
}

/// Untitled grid.
pub struct Grid;

impl Grid {
    /// Grid with `columns` columns.
    #[allow(clippy::new_ret_no_self)]
    pub fn make(columns: u8) -> Layout {
        Layout::of(LayoutType::Grid, None, None, columns.max(1))
    }
}

/// Tabs; children are [`Tab`]s.
pub struct Tabs;

impl Tabs {
    /// Empty tabs: add [`Tab`]s with `.schema(components![…])`.
    #[allow(clippy::new_ret_no_self)]
    pub fn make() -> Layout {
        Layout::of(LayoutType::Tabs, None, None, 1)
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
        Layout::of(LayoutType::Tab, Some(id), Some(label), 1)
    }
}

/// A main column and a narrow aside, like an editor's sidebar: from the `lg`
/// breakpoint the aside is about 20rem wide and stays in view (sticky) while
/// the main column scrolls; narrower screens show one column, the aside after
/// the main one (or before: [`Split::aside_first_on_mobile`]). Only at the
/// form's top level; both columns hold inputs and layouts (sections, grids,
/// tabs). Create, edit and view pages lay the record out the same way.
#[derive(Debug, Clone, Default)]
pub struct Split {
    main: Vec<Component>,
    aside: Vec<Component>,
    aside_first: bool,
}

impl Split {
    /// An empty split: add [`Split::main`] and [`Split::aside`].
    pub fn make() -> Self {
        Self::default()
    }

    /// The main column.
    pub fn main(mut self, components: Vec<Component>) -> Self {
        self.main = components;
        self
    }

    /// The aside.
    pub fn aside(mut self, components: Vec<Component>) -> Self {
        self.aside = components;
        self
    }

    /// On narrow screens, the aside before the main column.
    pub fn aside_first_on_mobile(mut self) -> Self {
        self.aside_first = true;
        self
    }
}

impl From<Split> for Layout {
    fn from(split: Split) -> Self {
        let pane = |id: &str, children: Vec<Component>| {
            Component::Layout(Layout { children, ..Layout::of(LayoutType::Pane, Some(id.to_owned()), None, 1) })
        };
        Layout {
            children: vec![pane("main", split.main), pane("aside", split.aside)],
            aside_first: split.aside_first,
            ..Layout::of(LayoutType::Split, None, None, 1)
        }
    }
}

impl From<Split> for Component {
    fn from(split: Split) -> Self {
        Component::Layout(split.into())
    }
}
