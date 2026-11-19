//! `Repeater`: a list of rows, each row several inputs (ADR 0040), in one JSON
//! column or as the child records of a has-many relation.

use super::{Component, Input, InputType, RowsValidator};

/// A repeater's `validate_rows` (ADR 0044, part D).
#[derive(Clone)]
pub(crate) struct RowsRule(pub(crate) RowsValidator);

impl std::fmt::Debug for RowsRule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RowsRule")
    }
}

/// What a repeater holds and how it is edited. Options that do not fit (a
/// JSON-only rule on a relationship row) are refused at boot.
#[derive(Debug, Clone)]
pub(crate) struct RepeaterSpec {
    /// The row's inputs and layouts (`Section`, `Grid`).
    pub(crate) components: Vec<Component>,
    pub(crate) min_items: Option<u32>,
    pub(crate) max_items: Option<u32>,
    pub(crate) reorderable: bool,
    /// Relationship repeaters: the child field holding the row's position.
    pub(crate) order_column: Option<String>,
    /// Rows are the child records of the has-many relation of the input's name.
    pub(crate) relationship: bool,
    pub(crate) collapsible: bool,
    pub(crate) collapsed: bool,
    /// A template over the row's inputs (`{question}`).
    pub(crate) item_label: Option<String>,
    pub(crate) add_action_label: Option<String>,
    pub(crate) addable: bool,
    pub(crate) deletable: bool,
    pub(crate) default_items: u32,
    pub(crate) cloneable: bool,
    /// Rows as table rows (ADR 0044).
    pub(crate) table: bool,
    /// Nested relationship repeaters: child fields the server fills with an
    /// ancestor's key (ADR 0044, part C).
    pub(crate) inherit: Vec<String>,
    /// A rule over the final rows (ADR 0044, part D).
    pub(crate) validate: Option<RowsRule>,
}

impl Default for RepeaterSpec {
    fn default() -> Self {
        Self {
            components: Vec::new(),
            min_items: None,
            max_items: None,
            reorderable: false,
            order_column: None,
            relationship: false,
            collapsible: false,
            collapsed: false,
            item_label: None,
            add_action_label: None,
            addable: true,
            deletable: true,
            default_items: 0,
            cloneable: false,
            table: false,
            inherit: Vec::new(),
            validate: None,
        }
    }
}

/// A list of rows (ADR 0040). By default its value is a JSON array of objects
/// in the column of its name; with [`Input::relationship`], its rows are the
/// child records of the has-many relation of its name.
///
/// ```rust,ignore
/// Repeater::make("faq")
///     .schema(components![
///         TextInput::make("question").required().max_length(200),
///         Textarea::make("answer").required(),
///     ])
///     .reorderable()
///     .item_label("{question}")
/// ```
pub struct Repeater;

impl Repeater {
    /// A repeater for the JSON field (or, with `relationship()`, the
    /// has-many relation) `name`.
    #[allow(clippy::new_ret_no_self)]
    pub fn make(name: impl Into<String>) -> Input {
        let mut input = Input::with_kind(name, InputType::Repeater);
        input.repeater = Some(Box::default());
        input
    }
}

impl Input {
    fn spec(&mut self) -> &mut RepeaterSpec {
        self.repeater.get_or_insert_with(Box::default)
    }

    /// The inputs (and `Section`s, `Grid`s) of each row of a repeater.
    pub fn schema(mut self, components: Vec<Component>) -> Self {
        self.spec().components = components;
        self
    }

    /// Fewest rows (repeaters).
    pub fn min_items(mut self, n: u32) -> Self {
        self.spec().min_items = Some(n);
        self
    }

    /// Most rows (repeaters).
    pub fn max_items(mut self, n: u32) -> Self {
        self.spec().max_items = Some(n);
        self
    }

    /// The user may reorder the rows of a JSON repeater (the order is kept).
    pub fn reorderable(mut self) -> Self {
        self.spec().reorderable = true;
        self
    }

    /// Relationship repeaters: the child's integer field holding each row's
    /// position, written by the server (1, 2, 3…); the user may reorder.
    pub fn order_column(mut self, field: impl Into<String>) -> Self {
        let spec = self.spec();
        spec.order_column = Some(field.into());
        spec.reorderable = true;
        self
    }

    /// The rows are the child records of the has-many relation named like the
    /// repeater; its inputs are the child's fields.
    pub fn relationship(mut self) -> Self {
        self.spec().relationship = true;
        self
    }

    /// A relationship repeater nested in another's rows: the child's field
    /// `field`, a `belongs_to` of an enclosing level (the record's resource, or
    /// an enclosing row's), is filled by the server with that level's key on
    /// create (the user never sends it). Checked at boot.
    ///
    /// ```rust,ignore
    /// // A quiz question's blanks, each with its choices: options that also
    /// // name their question (a double foreign key).
    /// Repeater::make("blanks").relationship().schema(components![
    ///     TextInput::make("label"),
    ///     Repeater::make("choices")
    ///         .relationship()
    ///         .inherit("quiz_question_id")
    ///         .schema(components![TextInput::make("content")]),
    /// ])
    /// ```
    pub fn inherit(mut self, field: impl Into<String>) -> Self {
        self.spec().inherit.push(field.into());
        self
    }

    /// A rule over the repeater's final rows, checked by the server whenever a
    /// save changes them (on create when the repeater is shown, rows sent or
    /// not): the panel, the API and imports. Relationship repeaters (any
    /// level; every live row of the relation, also those the user cannot see:
    /// keep their values out of messages) and JSON repeaters of the form.
    ///
    /// ```rust,ignore
    /// Repeater::make("options").relationship().validate_rows(|_ctx, check| {
    ///     let correct = check.rows().iter().filter(|r| r.get("correct") == Some(&json!(true))).count();
    ///     if correct == 1 { Ok(()) } else { Err(RowsError::list("must have exactly one correct option")) }
    /// })
    /// ```
    pub fn validate_rows(
        mut self,
        rule: impl Fn(&crate::Ctx, &super::RowsCheck) -> Result<(), super::RowsError> + Send + Sync + 'static,
    ) -> Self {
        self.spec().validate = Some(RowsRule(std::sync::Arc::new(rule)));
        self
    }

    /// Rows can be folded.
    pub fn collapsible(mut self) -> Self {
        self.spec().collapsible = true;
        self
    }

    /// Rows start folded (implies `collapsible`).
    pub fn collapsed(mut self) -> Self {
        let spec = self.spec();
        spec.collapsible = true;
        spec.collapsed = true;
        self
    }

    /// Each row's heading, from its inputs: `"{question}"`.
    pub fn item_label(mut self, template: impl Into<String>) -> Self {
        self.spec().item_label = Some(template.into());
        self
    }

    /// The label of the button adding a row (default: "Add").
    pub fn add_action_label(mut self, label: impl Into<String>) -> Self {
        self.spec().add_action_label = Some(label.into());
        self
    }

    /// Whether rows can be added (default: yes).
    pub fn addable(mut self, addable: bool) -> Self {
        self.spec().addable = addable;
        self
    }

    /// Whether rows can be removed (default: yes).
    pub fn deletable(mut self, deletable: bool) -> Self {
        self.spec().deletable = deletable;
        self
    }

    /// Rows a create form starts with (empty).
    pub fn default_items(mut self, n: u32) -> Self {
        self.spec().default_items = n;
        self
    }

    /// Rows laid out as a table: one column per input (labels in the header),
    /// the row's buttons last; stacked on narrow screens. Rows hold inputs
    /// only (no sections, grids or nested repeaters) and do not fold.
    pub fn table(mut self) -> Self {
        self.spec().table = true;
        self
    }

    /// A button copies a row below itself.
    pub fn cloneable(mut self) -> Self {
        self.spec().cloneable = true;
        self
    }

    /// JSON repeater rows: the input needs a value (in a JSON row there is no
    /// field to say so).
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// JSON repeater rows: the longest text, in characters.
    pub fn max_length(mut self, chars: usize) -> Self {
        self.max_length = Some(chars);
        self
    }

    /// JSON repeater rows: a whole number (within ±2^53).
    pub fn integer(mut self) -> Self {
        self.integer = true;
        self
    }
}
