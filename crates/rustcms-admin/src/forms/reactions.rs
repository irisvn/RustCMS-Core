//! Form reactions (ADR 0043): application code run on the server when the
//! user changes an input, answering with changes to the form. They help the
//! user fill the form; they never run on save (invariants belong in
//! `mutate_before_*` or validation).

use std::collections::HashMap;
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
/// write; never encrypted fields or links. The rows of the form's
/// relationship repeaters are apart ([`FormState::relation_rows`], ADR 0044
/// part D). Values are in wire format. **They are user input**: check keys
/// the reaction looks up (a `belongs_to` id, a media id) against what the
/// user may see.
#[derive(Debug, Clone)]
pub struct FormState {
    pub(crate) operation: Operation,
    pub(crate) key: Option<String>,
    pub(crate) changed: String,
    pub(crate) previous: Option<Value>,
    pub(crate) values: Data,
    /// The rows of the relationship repeaters the user's form holds.
    pub(crate) relation_rows: HashMap<String, Vec<StateRow>>,
}

/// A row of a relationship repeater (ADR 0044, part D): a stored child record
/// by its key, or a row the form added (not saved yet) by its reference
/// (`n1`). A key and a reference never name the same row, whatever their
/// text. On the wire: `{"key": "8"}`, `{"ref": "n1"}`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum RowId {
    /// A stored row, by its key.
    Key(String),
    /// A new row, by its reference (`n` and 1 to 9 digits).
    Ref(String),
}

impl RowId {
    /// A stored row's key.
    pub fn key(&self) -> Option<&str> {
        match self {
            Self::Key(key) => Some(key),
            Self::Ref(_) => None,
        }
    }

    /// A new row's reference.
    pub fn reference(&self) -> Option<&str> {
        match self {
            Self::Ref(reference) => Some(reference),
            Self::Key(_) => None,
        }
    }
}

/// Whether `reference` is a new row's reference: `n` and 1 to 9 digits.
pub(crate) fn valid_row_ref(reference: &str) -> bool {
    reference
        .strip_prefix('n')
        .is_some_and(|digits| (1..=9).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_digit()))
}

/// A row of a relationship repeater in a [`FormState`] (ADR 0044, part D):
/// its identity, and the values of the repeater's inputs the user may read,
/// as the form holds them (wire format, canonical). **User input**, as every
/// value of the state.
#[derive(Debug, Clone, PartialEq)]
pub struct StateRow {
    pub(crate) id: RowId,
    pub(crate) data: Data,
}

impl StateRow {
    /// The row's identity: what row patches name.
    pub fn id(&self) -> &RowId {
        &self.id
    }

    /// A stored row's key.
    pub fn key(&self) -> Option<&str> {
        self.id.key()
    }

    /// A new row's reference.
    pub fn reference(&self) -> Option<&str> {
        self.id.reference()
    }

    /// The value of `name`, if the row holds one.
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.data.get(name)
    }

    /// The text of `name`: `""` when absent or not a string.
    pub fn text(&self, name: &str) -> &str {
        self.get(name).and_then(Value::as_str).unwrap_or_default()
    }

    /// Every value of the row.
    pub fn values(&self) -> &Data {
        &self.data
    }
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

    /// The rows of the relationship repeater `name` as the form holds them
    /// (stored rows with the user's unsaved changes, and new rows, in the
    /// form's order): empty when the user's form does not hold the repeater
    /// (its relation unreadable, hidden by its conditions, more rows than the
    /// form edits).
    pub fn relation_rows(&self, name: &str) -> &[StateRow] {
        self.relation_rows.get(name).map_or(&[], Vec::as_slice)
    }
}

/// A change a reaction asks for (ADR 0043). Patches apply in order; a row
/// index refers to the rows as the previous patches left them, a relationship
/// row is named by its [`RowId`] (ADR 0044, part D). The server
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
    /// Adds a row to a relationship repeater (ADR 0044, part D), at the end.
    AddRelationRow {
        /// The repeater.
        field: String,
        /// The new row's reference (`n` and 1 to 9 digits), when later
        /// patches of the answer name it.
        #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(rename = "ref", optional))]
        reference: Option<String>,
        /// The row's values, in wire format.
        #[cfg_attr(feature = "ts", ts(type = "Record<string, JsonValue>"))]
        data: Data,
    },
    /// Sets values of a row of a relationship repeater.
    UpdateRelationRow {
        /// The repeater.
        field: String,
        /// The row.
        id: RowId,
        /// The new values, in wire format.
        #[cfg_attr(feature = "ts", ts(type = "Record<string, JsonValue>"))]
        data: Data,
    },
    /// Removes a row of a relationship repeater (a stored row is deleted on save).
    RemoveRelationRow {
        /// The repeater.
        field: String,
        /// The row.
        id: RowId,
    },
    /// A warning under a row of a relationship repeater; changes nothing.
    FlagRelationRow {
        /// The repeater.
        field: String,
        /// The row.
        id: RowId,
        /// The warning (translated).
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

    /// Adds a row holding `data` (an object of wire values) at the end of the
    /// relationship repeater `field` (ADR 0044, part D).
    pub fn add_relation_row(field: impl Into<String>, data: Value) -> Self {
        Self::AddRelationRow { field: field.into(), reference: None, data: object(data) }
    }

    /// As [`Self::add_relation_row`], the new row named `reference` (`n` and
    /// 1 to 9 digits, not a row of the state) so later patches can name it.
    pub fn add_relation_row_as(field: impl Into<String>, reference: impl Into<String>, data: Value) -> Self {
        Self::AddRelationRow { field: field.into(), reference: Some(reference.into()), data: object(data) }
    }

    /// Sets the values `data` holds on row `id` of the relationship repeater `field`.
    pub fn update_relation_row(field: impl Into<String>, id: RowId, data: Value) -> Self {
        Self::UpdateRelationRow { field: field.into(), id, data: object(data) }
    }

    /// Removes row `id` of the relationship repeater `field`.
    pub fn remove_relation_row(field: impl Into<String>, id: RowId) -> Self {
        Self::RemoveRelationRow { field: field.into(), id }
    }

    /// A warning under row `id` of the relationship repeater `field`.
    pub fn flag_relation_row(field: impl Into<String>, id: RowId, message: impl Into<String>) -> Self {
        Self::FlagRelationRow { field: field.into(), id, message: message.into() }
    }

    /// The field it changes or flags (`None` for a notice).
    pub(crate) fn field(&self) -> Option<&str> {
        match self {
            Self::Set { field, .. }
            | Self::SetIfEmpty { field, .. }
            | Self::AddRow { field, .. }
            | Self::RemoveRow { field, .. }
            | Self::MoveRow { field, .. }
            | Self::FlagRow { field, .. }
            | Self::AddRelationRow { field, .. }
            | Self::UpdateRelationRow { field, .. }
            | Self::RemoveRelationRow { field, .. }
            | Self::FlagRelationRow { field, .. } => Some(field),
            Self::Notice { .. } => None,
        }
    }

    /// Whether it changes or flags rows of a relationship repeater.
    pub(crate) fn is_relation_row(&self) -> bool {
        matches!(
            self,
            Self::AddRelationRow { .. }
                | Self::UpdateRelationRow { .. }
                | Self::RemoveRelationRow { .. }
                | Self::FlagRelationRow { .. }
        )
    }
}

/// `value` as an object (anything else: empty).
fn object(value: Value) -> Data {
    match value {
        Value::Object(data) => data,
        _ => Data::new(),
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
