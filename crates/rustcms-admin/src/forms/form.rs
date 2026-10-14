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

/// A form. An empty form is generated from the resource's fields.
#[derive(Debug, Clone, Default)]
pub struct Form {
    pub(crate) columns: u8,
    pub(crate) components: Vec<Component>,
}

impl Form {
    /// Empty one-column form.
    pub fn new() -> Self {
        Self { columns: 1, components: Vec::new() }
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
}
