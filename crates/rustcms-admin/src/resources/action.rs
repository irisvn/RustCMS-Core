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
        ) -> BoxFuture<'a, AppResult<ActionOutcome>>
        + Send
        + Sync,
>;

/// Gives a handler closure its higher-ranked signature.
fn handler<E: EntityTrait, F>(f: F) -> F
where
    F: for<'a> Fn(&'a Ctx, &'a DatabaseTransaction, Vec<E::Model>, Data) -> BoxFuture<'a, AppResult<ActionOutcome>>,
{
    f
}

type Visibility<E> = Arc<dyn Fn(&Ctx, &<E as EntityTrait>::Model) -> bool + Send + Sync>;

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
    pub(crate) visible: Option<Visibility<E>>,
    pub(crate) primary: bool,
    pub(crate) submit_label: Option<String>,
}

impl<E: EntityTrait> Action<E> {
    /// Declares an action. The handler receives the authorized records.
    pub fn new<F>(name: &'static str, run: F) -> Self
    where
        F: for<'a> Fn(&'a Ctx, &'a DatabaseTransaction, Vec<E::Model>, Data) -> BoxFuture<'a, AppResult<()>>
            + Send
            + Sync
            + 'static,
    {
        Self::with_outcome(
            name,
            handler::<E, _>(move |ctx, txn, models, data| {
                let ran = run(ctx, txn, models, data);
                Box::pin(async move { ran.await.map(|()| ActionOutcome::default()) })
            }),
        )
    }

    /// Declares an action whose handler also tells the panel what to do
    /// next: show a message ([`ActionOutcome::message`]) and, on a run on
    /// one record, open a page in a new tab ([`ActionOutcome::open`]).
    ///
    /// ```rust,ignore
    /// Action::<invoice::Entity>::with_outcome("pdf", |_ctx, _txn, invoices, _data| {
    ///     Box::pin(async move {
    ///         let url = format!("https://files.example.com/invoices/{}.pdf", invoices[0].id);
    ///         Ok(ActionOutcome::open(url).with_message("The PDF opens in a new tab."))
    ///     })
    /// })
    /// ```
    pub fn with_outcome<F>(name: &'static str, run: F) -> Self
    where
        F: for<'a> Fn(&'a Ctx, &'a DatabaseTransaction, Vec<E::Model>, Data) -> BoxFuture<'a, AppResult<ActionOutcome>>
            + Send
            + Sync
            + 'static,
    {
        Self {
            name,
            ability: name,
            handler: Arc::new(run),
            label: None,
            bulk: false,
            confirm: None,
            color: None,
            icon: None,
            required_permission: None,
            form: Vec::new(),
            visible: None,
            primary: false,
            submit_label: None,
        }
    }

    /// Offers the action only on records for which `predicate` holds,
    /// decided by the server for each record as stored, for the user of
    /// `ctx`. The list and record answers say so per record (`abilities`).
    /// Running the action decides again, on each record locked in the
    /// action's transaction: a run on one record it is hidden on is refused
    /// (403, nothing runs); a run on several records skips those records and
    /// reports them in `denied` (the handler never gets them).
    ///
    /// ```rust,ignore
    /// Action::<post::Entity>::new("unpublish", handler).visible_when(|_ctx, post| post.published)
    /// ```
    pub fn visible_when(mut self, predicate: impl Fn(&Ctx, &E::Model) -> bool + Send + Sync + 'static) -> Self {
        self.visible = Some(Arc::new(predicate));
        self
    }

    /// The single row action shown with its label in lists and on record
    /// pages; the others go in the row's ⋯ menu. When several are primary
    /// (made exclusive by [`Self::visible_when`]), a row shows the first one
    /// it offers.
    pub fn primary(mut self) -> Self {
        self.primary = true;
        self
    }

    /// Marks the action as destructive (color `danger`): shown last in a
    /// row's menu, after a separator.
    pub fn destructive(self) -> Self {
        self.color("danger")
    }

    /// Whether the action is offered on `model` to the user of `ctx` ([`Self::visible_when`]).
    pub(crate) fn shows_for(&self, ctx: &Ctx, model: &E::Model) -> bool {
        self.visible.as_ref().is_none_or(|visible| visible(ctx, model))
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

    /// Label of the button that runs the action from its form dialog
    /// ([`Self::form`]); default: the action's label without a trailing `…`.
    pub fn submit_label(mut self, label: impl Into<String>) -> Self {
        self.submit_label = Some(label.into());
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

/// Per-record result of an action, and what the panel does next.
///
/// A handler declared with [`Action::with_outcome`] returns one to show a
/// message or open a page; the framework fills in the keys (`processed`,
/// `denied`, `missing`, `replayed`) and keeps only the handler's `message`
/// and `open`.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ActionOutcome {
    /// Keys the handler processed.
    pub processed: Vec<String>,
    /// Keys the policy denied, or the action is hidden on (several records).
    pub denied: Vec<String>,
    /// Keys not found in the user's scope.
    pub missing: Vec<String>,
    /// This is the outcome of an earlier request with the same `Idempotency-Key`:
    /// the action did not run again.
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub replayed: bool,
    /// A message for the user, shown by the panel (plain text, at most
    /// [`MAX_OUTCOME_MESSAGE`] characters).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// A page the panel opens in a new tab (`noopener`, `noreferrer`) once
    /// the action committed: an `https://` URL, or a path of this site
    /// (`/admin/…`, `/blog/…`). Only for a run on one record; any other
    /// value fails the action (rolled back).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<String>,
}

/// Longest [`ActionOutcome::message`], in characters.
pub const MAX_OUTCOME_MESSAGE: usize = 500;

/// Longest [`ActionOutcome::open`], in bytes.
pub const MAX_OUTCOME_URL: usize = 2048;

impl ActionOutcome {
    /// Opens `url` in a new tab: `https://…`, or a path of this site
    /// (`/admin/r/posts`). Checked when the action runs.
    pub fn open(url: impl Into<String>) -> Self {
        Self::default().with_open(url)
    }

    /// Shows `text` to the user.
    pub fn message(text: impl Into<String>) -> Self {
        Self::default().with_message(text)
    }

    /// The same outcome, also opening `url` ([`Self::open`]).
    pub fn with_open(mut self, url: impl Into<String>) -> Self {
        self.open = Some(url.into());
        self
    }

    /// The same outcome, also showing `text`.
    pub fn with_message(mut self, text: impl Into<String>) -> Self {
        self.message = Some(text.into());
        self
    }

    /// What the handler returned for the panel, checked: the message's
    /// length; a URL that is `https://` or a path of this site, and never
    /// for a run on several records (`bulk`).
    pub(crate) fn effects(self, bulk: bool) -> AppResult<(Option<String>, Option<String>)> {
        if let Some(message) = &self.message
            && message.chars().count() > MAX_OUTCOME_MESSAGE
        {
            return Err(rustcms_core::AppError::internal(format!(
                "action outcome: the message is longer than {MAX_OUTCOME_MESSAGE} characters"
            )));
        }
        if let Some(url) = &self.open {
            if !openable(url) {
                return Err(rustcms_core::AppError::internal(
                    "action outcome: `open` must be an https URL or a path of this site",
                ));
            }
            if bulk {
                let mut errors = rustcms_core::ValidationErrors::new();
                errors.add_form("Run this action on one record at a time.");
                return Err(errors.into());
            }
        }
        Ok((self.message, self.open))
    }
}

/// `host` or `host:port` of an https URL: a DNS name (labels of letters,
/// digits and `-`), an IPv4 address or a bracketed IPv6 address; a port of
/// 1–65535. No credentials.
fn valid_authority(authority: &str) -> bool {
    let (host, port) = match authority.strip_prefix('[') {
        Some(rest) => match rest.split_once(']') {
            Some((ip, after)) => {
                if ip.parse::<std::net::Ipv6Addr>().is_err() {
                    return false;
                }
                match after {
                    "" => (ip, None),
                    _ => match after.strip_prefix(':') {
                        Some(port) => (ip, Some(port)),
                        None => return false,
                    },
                }
            }
            None => return false,
        },
        None => match authority.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        },
    };
    let port_ok = port.is_none_or(|p| {
        !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) && p.parse::<u16>().is_ok_and(|n| n > 0)
    });
    let bracketed = authority.starts_with('[');
    let host_ok = bracketed
        || (!host.is_empty()
            && host.len() <= 253
            && host.split('.').all(|label| {
                !label.is_empty()
                    && label.len() <= 63
                    && !label.starts_with('-')
                    && !label.ends_with('-')
                    && label.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            }));
    port_ok && host_ok
}

/// `https://host…` without credentials, or an absolute path of this site
/// (`/…`, never `//…`); never whitespace, controls, backslashes or dot
/// segments (also percent-encoded); at most [`MAX_OUTCOME_URL`] bytes.
pub(crate) fn openable(url: &str) -> bool {
    if url.is_empty()
        || url.len() > MAX_OUTCOME_URL
        || url.chars().any(|c| c.is_control() || c.is_whitespace() || c == '\\')
    {
        return false;
    }
    if url.get(..8).is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://")) {
        let authority = url[8..].split(['/', '?', '#']).next().unwrap_or_default();
        return valid_authority(authority);
    }
    let path = url.split(['?', '#']).next().unwrap_or_default();
    let dotted = path.split('/').any(|segment| {
        let lower = segment.to_ascii_lowercase().replace("%2e", ".");
        lower == "." || lower == ".."
    });
    url.starts_with('/') && !url.starts_with("//") && !dotted
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
