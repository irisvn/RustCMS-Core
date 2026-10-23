//! Row and bulk actions.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use rustcms_core::AppResult;
use sea_orm::{DatabaseTransaction, EntityTrait};
use serde::Serialize;

use crate::Data;
use crate::context::Ctx;

/// Boxed `Send` future, used to store async closures.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

type Handler<E> = Arc<
    dyn for<'a> Fn(
            &'a Ctx,
            &'a DatabaseTransaction,
            Vec<<E as EntityTrait>::Model>,
            Data,
        ) -> BoxFuture<'a, AppResult<()>>
        + Send
        + Sync,
>;

/// A named operation run on selected records inside a transaction.
///
/// Each record is authorized individually with the policy ability `ability`
/// (default: the action name); denied records are reported, not processed.
///
/// Name the entity so the closure's parameter types are inferred:
///
/// ```rust,ignore
/// Action::<post::Entity>::new("publish", |_ctx, _txn, _posts, _data| Box::pin(async move { Ok(()) }))
/// ```
pub struct Action<E: EntityTrait> {
    pub(crate) name: &'static str,
    pub(crate) ability: &'static str,
    pub(crate) handler: Handler<E>,
    pub(crate) label: Option<String>,
    pub(crate) bulk: bool,
    pub(crate) confirm: Option<String>,
    pub(crate) color: Option<String>,
    pub(crate) icon: Option<String>,
    pub(crate) required_permission: Option<&'static str>,
    pub(crate) form: Vec<ActionField>,
}

impl<E: EntityTrait> Action<E> {
    /// Declares an action. The handler receives the authorized records.
    pub fn new<F>(name: &'static str, handler: F) -> Self
    where
        F: for<'a> Fn(&'a Ctx, &'a DatabaseTransaction, Vec<E::Model>, Data) -> BoxFuture<'a, AppResult<()>>
            + Send
            + Sync
            + 'static,
    {
        Self {
            name,
            ability: name,
            handler: Arc::new(handler),
            label: None,
            bulk: false,
            confirm: None,
            color: None,
            icon: None,
            required_permission: None,
            form: Vec::new(),
        }
    }

    /// Asks for `fields` in a dialog before running. The values are checked
    /// on the server (types, rules, options; a `belongs_to` value must be a
    /// record the user may view, checked in the action's transaction) and
    /// reach the handler as `data`, with every field present (`null` when
    /// left empty). An action without a form refuses any `data`.
    pub fn form(mut self, fields: Vec<ActionField>) -> Self {
        self.form = fields;
        self
    }

    /// Refuses the action outright (403, and hides it from the schema) for users
    /// without `permission`. Per-record policy checks still apply to the others.
    pub fn requires_permission(mut self, permission: &'static str) -> Self {
        self.required_permission = Some(permission);
        self
    }

    /// Button label (default: the name, humanized).
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Also offered for selections of records.
    pub fn bulk(mut self) -> Self {
        self.bulk = true;
        self
    }

    /// Asks this question before running.
    pub fn requires_confirmation(mut self, question: impl Into<String>) -> Self {
        self.confirm = Some(question.into());
        self
    }

    /// Color (`primary`, `success`, `warning`, `danger`, `gray`).
    pub fn color(mut self, color: impl Into<String>) -> Self {
        self.color = Some(color.into());
        self
    }

    /// Icon name.
    pub fn icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    /// Overrides the policy ability checked per record.
    pub fn ability(mut self, ability: &'static str) -> Self {
        self.ability = ability;
        self
    }
}

/// Per-record result of an action.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ActionOutcome {
    /// Keys the handler processed.
    pub processed: Vec<String>,
    /// Keys the policy denied.
    pub denied: Vec<String>,
    /// Keys not found in the user's scope.
    pub missing: Vec<String>,
    /// This is the outcome of an earlier request with the same `Idempotency-Key`:
    /// the action did not run again.
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub replayed: bool,
}

/// A value asked for by an action's form ([`Action::form`]).
#[derive(Debug, Clone)]
pub struct ActionField {
    pub(crate) name: String,
    pub(crate) kind: crate::wire::FieldKind,
    pub(crate) input: crate::ui::Input,
    pub(crate) required: bool,
    pub(crate) max_length: Option<usize>,
    pub(crate) default: Option<serde_json::Value>,
    pub(crate) belongs_to: Option<&'static str>,
}

impl ActionField {
    fn new(name: impl Into<String>, kind: crate::wire::FieldKind, input: crate::ui::InputType) -> Self {
        let name = name.into();
        Self {
            input: crate::ui::Input::with_kind(name.clone(), input),
            name,
            kind,
            required: false,
            max_length: None,
            default: None,
            belongs_to: None,
        }
    }

    /// Single-line text.
    pub fn text(name: impl Into<String>) -> Self {
        Self::new(name, crate::wire::FieldKind::String, crate::ui::InputType::TextInput)
    }

    /// Multi-line text.
    pub fn textarea(name: impl Into<String>) -> Self {
        Self::new(name, crate::wire::FieldKind::String, crate::ui::InputType::Textarea)
    }

    /// A 32-bit integer (a JSON number).
    pub fn integer(name: impl Into<String>) -> Self {
        Self::new(name, crate::wire::FieldKind::I32, crate::ui::InputType::NumberInput)
    }

    /// On or off.
    pub fn boolean(name: impl Into<String>) -> Self {
        Self::new(name, crate::wire::FieldKind::Bool, crate::ui::InputType::Toggle)
    }

    /// A calendar date (`YYYY-MM-DD`).
    pub fn date(name: impl Into<String>) -> Self {
        Self::new(name, crate::wire::FieldKind::Date, crate::ui::InputType::DatePicker)
    }

    /// A point in time, edited and labelled in UTC like the form's
    /// `DateTimePicker` of an instant column. Sent as RFC 3339 with an offset
    /// (ADR 0003); the handler gets it in canonical form, in UTC
    /// (`"2026-10-12T02:30:00Z"`): parse it with
    /// `chrono::DateTime::parse_from_rfc3339`.
    pub fn datetime(name: impl Into<String>) -> Self {
        Self::new(name, crate::wire::FieldKind::Instant, crate::ui::InputType::DateTimePicker)
    }

    /// One of `options` (`(value, label)`).
    pub fn select<L: Into<String>>(
        name: impl Into<String>,
        options: impl IntoIterator<Item = (&'static str, L)>,
    ) -> Self {
        let mut field = Self::new(name, crate::wire::FieldKind::String, crate::ui::InputType::Select);
        field.input = field.input.options(options);
        field
    }

    /// A record of the resource `target` the user may view, chosen from its
    /// options; the handler gets its key as text (`"12"`, a UUID, …).
    pub fn belongs_to(name: impl Into<String>, target: &'static str) -> Self {
        let mut field = Self::new(name, crate::wire::FieldKind::String, crate::ui::InputType::Select);
        field.belongs_to = Some(target);
        field
    }

    /// Must have a value (otherwise `null` is accepted).
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// Longest text, in characters.
    pub fn max_length(mut self, max: usize) -> Self {
        self.max_length = Some(max);
        self
    }

    /// Value used when the field is left out, in wire format (checked at boot).
    pub fn default(mut self, value: impl Into<serde_json::Value>) -> Self {
        self.default = Some(value.into());
        self
    }

    /// Label (default: the name in words).
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.input = self.input.label(label);
        self
    }

    /// Help text below the input.
    pub fn help(mut self, text: impl Into<String>) -> Self {
        self.input = self.input.help(text);
        self
    }

    /// Placeholder.
    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.input = self.input.placeholder(text);
        self
    }

    pub(crate) fn display_label(&self) -> String {
        self.input.label.clone().unwrap_or_else(|| crate::ui::humanize(&self.name))
    }

    pub(crate) fn options(&self) -> Vec<serde_json::Value> {
        self.input.options.iter().map(|o| o.value.clone()).collect()
    }
}
