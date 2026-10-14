//! Presentation builders, in the spirit of Filament: forms and tables.
//!
//! They only describe the UI. Validation and authorization come from the
//! resource's [`Field`](crate::Field)s and policy, never from these builders:
//! an input for a field the user may not write is dropped or disabled by the
//! schema endpoint, and the write is refused by the engine regardless.
//!
//! ```rust,ignore
//! Form::new().columns(2).schema(components![
//!     Section::make("Content").schema(components![
//!         TextInput::make("title"),
//!         Textarea::make("excerpt").column_span(2),
//!     ]),
//!     SelectInput::make("status").options([("draft", "Draft"), ("published", "Published")]),
//! ])
//! ```

pub use crate::forms::*;
pub use crate::tables::*;

/// `category_id` → `Category`, `published_at` → `Published at`.
pub(crate) fn humanize(name: &str) -> String {
    let name = name.strip_suffix("_id").unwrap_or(name);
    let mut out = name.replace('_', " ");
    if let Some(first) = out.get(0..1) {
        let upper = first.to_uppercase();
        out.replace_range(0..1, &upper);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn humanizes_names() {
        assert_eq!(humanize("category_id"), "Category");
        assert_eq!(humanize("published_at"), "Published at");
        assert_eq!(humanize("title"), "Title");
    }

    #[test]
    fn walks_nested_inputs() {
        let form = Form::new().schema(crate::components![
            TextInput::make("a"),
            Section::make("S").schema(crate::components![Grid::make(2).schema(crate::components![Toggle::make("b")])]),
        ]);
        let names: Vec<&str> = form.inputs().iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["a", "b"]);
    }
}
