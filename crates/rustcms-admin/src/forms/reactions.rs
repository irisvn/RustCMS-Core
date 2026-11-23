//! Form reactions (ADR 0043): application code run on the server when the
//! user changes an input, answering with changes to the form. They help the
//! user fill the form; they never run on save (invariants belong in
//! `mutate_before_*` or validation).

use std::sync::Arc;
use std::time::Duration;

use rustcms_core::AppResult;
use serde_json::Value;

use crate::action::BoxFuture;
use crate::context::Ctx;
use crate::{Data, Operation};

/// Time a reaction may take by default.
pub const REACTION_TIMEOUT: Duration = Duration::from_secs(5);
/// The longest [`Input::reaction_timeout`](super::Input::reaction_timeout).
pub const REACTION_MAX_TIMEOUT: Duration = Duration::from_secs(30);

/// A reaction: from the form's state, the changes to make.
pub(crate) type ReactionFn =
    Arc<dyn for<'a> Fn(&'a Ctx, &'a FormState) -> BoxFuture<'a, AppResult<Vec<FormPatch>>> + Send + Sync>;

/// An input's reaction, as declared. `run` is `None` when
/// `.reaction_timeout()` came without `.reacts()` (refused at boot).
#[derive(Clone)]
pub(crate) struct Reaction {
    pub(crate) run: Option<ReactionFn>,
    pub(crate) timeout: Duration,
}

impl Default for Reaction {
    fn default() -> Self {
        Self { run: None, timeout: REACTION_TIMEOUT }
    }
}

impl std::fmt::Debug for Reaction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reaction").field("timeout", &self.timeout).finish_non_exhaustive()
    }
}

/// What the user has in the form when a reaction runs (ADR 0043): the values
/// the form holds (a record's stored values only when the user may view it,
/// an unreadable field's never) and what the user typed in inputs they may
/// write; never encrypted fields, links or relationship repeaters. Values are
/// in wire format. **They are user input**: check keys the reaction looks up
/// (a `belongs_to` id, a media id) against what the user may see.
#[derive(Debug, Clone)]
pub struct FormState {
    pub(crate) operation: Operation,
    pub(crate) key: Option<String>,
    pub(crate) changed: String,
    pub(crate) previous: Option<Value>,
    pub(crate) values: Data,
}

impl FormState {
    /// `Create` or `Update` (the edit form).
    pub fn operation(&self) -> Operation {
        self.operation
    }

    /// The record's key, on the edit form.
    pub fn key(&self) -> Option<&str> {
        self.key.as_deref()
    }

    /// The input whose change asked for the reaction; for an `options_fn`,
    /// the input whose options are asked for (ADR 0044, part E).
    pub fn changed(&self) -> &str {
        &self.changed
    }

    /// The changed input's value when the panel last applied an answer (or
    /// opened the form), if the panel sent it: to fill a field only while it
    /// still holds what an earlier value would have given (a slug that
    /// follows the name until the user edits it).
    pub fn previous(&self) -> Option<&Value> {
        self.previous.as_ref()
    }

    /// The value of `name`, when the user may read it and the form has one.
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.values.get(name)
    }

    /// The text of `name`: `""` when absent or not a string.
    pub fn text(&self, name: &str) -> &str {
        self.get(name).and_then(Value::as_str).unwrap_or_default()
    }

    /// The rows of the JSON repeater `name`: empty when absent.
    pub fn rows(&self, name: &str) -> &[Value] {
        self.get(name).and_then(Value::as_array).map_or(&[], Vec::as_slice)
    }

    /// Every value the reaction may read.
    pub fn values(&self) -> &Data {
        &self.values
    }
}

/// A change a reaction asks for (ADR 0043). Patches apply in order; a row
/// index refers to the rows as the previous patches left them. The server
/// drops a patch the user could not make (a field not writable or hidden, a
/// value its input refuses) and one that changes nothing.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum FormPatch {
    /// Sets the field.
    Set {
        /// The field.
        field: String,
        /// Its new value, in wire format.
        value: Value,
    },
    /// Sets the field when it is `null`, `""` or `[]`.
    SetIfEmpty {
        /// The field.
        field: String,
        /// Its new value, in wire format.
        value: Value,
    },
    /// Adds a row to a JSON repeater, at `index` (default: the end).
    AddRow {
        /// The repeater.
        field: String,
        /// Where the row goes.
        #[cfg_attr(feature = "ts", ts(optional))]
        #[serde(skip_serializing_if = "Option::is_none")]
        index: Option<u32>,
        /// The row's values.
        row: Value,
    },
    /// Removes a row of a JSON repeater.
    RemoveRow {
        /// The repeater.
        field: String,
        /// The row.
        index: u32,
    },
    /// Moves a row of a JSON repeater.
    MoveRow {
        /// The repeater.
        field: String,
        /// The row.
        from: u32,
        /// Its new place.
        to: u32,
    },
    /// A warning under a row; changes nothing.
    FlagRow {
        /// The repeater.
        field: String,
        /// The row.
        index: u32,
        /// The warning (translated).
        message: String,
    },
    /// A message to the user; changes nothing.
    Notice {
        /// The message (translated).
        message: String,
    },
}

impl FormPatch {
    /// Sets `field` to `value` (wire format).
    pub fn set(field: impl Into<String>, value: impl Into<Value>) -> Self {
        Self::Set { field: field.into(), value: value.into() }
    }

    /// Sets `field` to `value` when it is `null`, `""` or `[]`.
    pub fn set_if_empty(field: impl Into<String>, value: impl Into<Value>) -> Self {
        Self::SetIfEmpty { field: field.into(), value: value.into() }
    }

    /// Adds `row` at the end of the JSON repeater `field`.
    pub fn add_row(field: impl Into<String>, row: Value) -> Self {
        Self::AddRow { field: field.into(), index: None, row }
    }

    /// Adds `row` at `index` of the JSON repeater `field`.
    pub fn add_row_at(field: impl Into<String>, index: u32, row: Value) -> Self {
        Self::AddRow { field: field.into(), index: Some(index), row }
    }

    /// Removes the row `index` of the JSON repeater `field`.
    pub fn remove_row(field: impl Into<String>, index: u32) -> Self {
        Self::RemoveRow { field: field.into(), index }
    }

    /// Moves the row `from` of the JSON repeater `field` to `to`.
    pub fn move_row(field: impl Into<String>, from: u32, to: u32) -> Self {
        Self::MoveRow { field: field.into(), from, to }
    }

    /// A warning under the row `index` of the JSON repeater `field`.
    pub fn flag_row(field: impl Into<String>, index: u32, message: impl Into<String>) -> Self {
        Self::FlagRow { field: field.into(), index, message: message.into() }
    }

    /// A message to the user.
    pub fn notice(message: impl Into<String>) -> Self {
        Self::Notice { message: message.into() }
    }

    /// The field it changes or flags (`None` for a notice).
    pub(crate) fn field(&self) -> Option<&str> {
        match self {
            Self::Set { field, .. }
            | Self::SetIfEmpty { field, .. }
            | Self::AddRow { field, .. }
            | Self::RemoveRow { field, .. }
            | Self::MoveRow { field, .. }
            | Self::FlagRow { field, .. } => Some(field),
            Self::Notice { .. } => None,
        }
    }
}

/// `null`, `""` or `[]` (what `set_if_empty` fills).
pub(crate) fn is_empty(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => true,
        Some(Value::String(text)) => text.is_empty(),
        Some(Value::Array(items)) => items.is_empty(),
        Some(_) => false,
    }
}
