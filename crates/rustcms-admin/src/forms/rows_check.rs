//! `validate_rows` (ADR 0044, part D): an invariant over a repeater's final
//! rows, checked on the server whenever a save changes them (the panel, the
//! API, imports).

use std::sync::Arc;

use serde_json::Value;

use crate::context::Ctx;
use crate::{Data, Operation};

/// A repeater's rule over its rows: `Err` refuses the save (nothing is written).
pub(crate) type RowsValidator = Arc<dyn Fn(&Ctx, &RowsCheck) -> Result<(), RowsError> + Send + Sync>;

/// What a repeater's [`validate_rows`](super::Input::validate_rows) reads: the
/// rows as the save leaves them and what holds them.
///
/// For a relationship repeater the rows are **every** live row of the relation
/// (filtered, not trashed), including rows the user cannot see: do not put
/// their values in messages.
#[derive(Debug, Clone)]
pub struct RowsCheck {
    pub(crate) operation: Operation,
    pub(crate) parent: Data,
    pub(crate) rows: Vec<CheckedRow>,
}

impl RowsCheck {
    /// `Create` or `Update` of the record.
    pub fn operation(&self) -> Operation {
        self.operation
    }

    /// The values of what holds the rows, as this save leaves them: the
    /// record's (the form's repeaters), or the enclosing row's (a nested one).
    pub fn parent(&self) -> &Data {
        &self.parent
    }

    /// The final rows, in order.
    pub fn rows(&self) -> &[CheckedRow] {
        &self.rows
    }
}

/// A row of a [`RowsCheck`].
#[derive(Debug, Clone)]
pub struct CheckedRow {
    pub(crate) id: String,
    pub(crate) values: Data,
}

impl CheckedRow {
    /// How errors name the row: a relationship row's key (the ref of a row
    /// this save created), a JSON row's index.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The value of `name` (wire format), if any.
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.values.get(name)
    }

    /// Every value of the row (a relationship row: the child's fields, not
    /// encrypted ones).
    pub fn values(&self) -> &Data {
        &self.values
    }
}

/// Why a repeater's rows are refused: messages on the repeater, a row or a
/// row's input (`options`, `options.n1`, `options.n1.correct`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RowsError {
    pub(crate) messages: Vec<(Option<String>, Option<String>, String)>,
}

impl RowsError {
    /// A message on the repeater itself.
    pub fn list(message: impl Into<String>) -> Self {
        Self { messages: vec![(None, None, message.into())] }
    }

    /// A message on row `id` ([`CheckedRow::id`]).
    pub fn row(id: impl Into<String>, message: impl Into<String>) -> Self {
        Self { messages: vec![(Some(id.into()), None, message.into())] }
    }

    /// A message on input `name` of row `id`.
    pub fn input(id: impl Into<String>, name: impl Into<String>, message: impl Into<String>) -> Self {
        Self { messages: vec![(Some(id.into()), Some(name.into()), message.into())] }
    }

    /// This error and `other`'s messages.
    #[must_use]
    pub fn and(mut self, other: RowsError) -> Self {
        self.messages.extend(other.messages);
        self
    }

    /// Adds the messages to `errors`, under `path` (the repeater's).
    pub(crate) fn add_at(&self, path: &str, errors: &mut rustcms_core::ValidationErrors) {
        for (id, input, message) in &self.messages {
            let at = match (id, input) {
                (Some(id), Some(input)) => format!("{path}.{id}.{input}"),
                (Some(id), None) => format!("{path}.{id}"),
                _ => path.to_owned(),
            };
            errors.add(at, message.clone());
        }
        // An error without a message still refuses the save.
        if self.messages.is_empty() {
            errors.add(path, "is invalid");
        }
    }
}
