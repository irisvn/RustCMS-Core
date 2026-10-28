//! A form and its components.

use super::{Input, Layout};

/// An element of a form.
#[derive(Debug, Clone)]
pub enum Component {
    /// Field input.
    Input(Input),
    /// Layout.
    Layout(Layout),
}

impl From<Input> for Component {
    fn from(input: Input) -> Self {
        Self::Input(input)
    }
}

impl From<Layout> for Component {
    fn from(layout: Layout) -> Self {
        Self::Layout(layout)
    }
}

/// Builds a `Vec<Component>` from inputs and layouts.
#[macro_export]
macro_rules! components {
    ($($component:expr),* $(,)?) => { vec![$($crate::ui::Component::from($component)),*] };
}

/// How wide a record's page lays its form out (create, edit, view).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum FormWidth {
    /// About 56rem (the panel's default).
    #[default]
    Default,
    /// About 72rem.
    Wide,
    /// The whole width of the page (its padding kept).
    Full,
}

impl FormWidth {
    pub(crate) fn is_default(&self) -> bool {
        *self == Self::Default
    }
}

/// A form. An empty form is generated from the resource's fields.
#[derive(Debug, Clone, Default)]
pub struct Form {
    pub(crate) columns: u8,
    pub(crate) components: Vec<Component>,
    pub(crate) width: FormWidth,
}

impl Form {
    /// Empty one-column form.
    pub fn new() -> Self {
        Self { columns: 1, components: Vec::new(), width: FormWidth::Default }
    }

    /// How wide the record's page is (default: [`FormWidth::Default`]).
    pub fn width(mut self, width: FormWidth) -> Self {
        self.width = width;
        self
    }

    /// Number of columns.
    pub fn columns(mut self, columns: u8) -> Self {
        self.columns = columns.max(1);
        self
    }

    /// Components.
    pub fn schema(mut self, components: Vec<Component>) -> Self {
        self.components = components;
        self
    }

    /// Every input, depth first.
    pub(crate) fn inputs(&self) -> Vec<&Input> {
        fn walk<'a>(components: &'a [Component], out: &mut Vec<&'a Input>) {
            for component in components {
                match component {
                    Component::Input(input) => out.push(input),
                    Component::Layout(layout) => walk(&layout.children, out),
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.components, &mut out);
        out
    }

    /// Every input, depth first, to change.
    pub(crate) fn inputs_mut(&mut self) -> Vec<&mut Input> {
        fn walk<'a>(components: &'a mut [Component], out: &mut Vec<&'a mut Input>) {
            for component in components {
                match component {
                    Component::Input(input) => out.push(input),
                    Component::Layout(layout) => walk(&mut layout.children, out),
                }
            }
        }
        let mut out = Vec::new();
        walk(&mut self.components, &mut out);
        out
    }
}
