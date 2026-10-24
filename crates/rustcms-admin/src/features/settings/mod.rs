//! Settings pages: a form per settings group (`<admin>/api/settings/{group}`),
//! validated like resource input, written under the RBAC lock with an audit
//! entry, secrets encrypted (ADR 0015).

mod http;
#[cfg(feature = "mail")]
mod mail;

use std::collections::{BTreeMap, BTreeSet};
use std::marker::PhantomData;
use std::sync::Arc;

use async_trait::async_trait;
use rustcms_core::settings::{Settings, Stored, typed, valid_group};
use rustcms_core::{AppError, AppResult, AppState, ValidationErrors};
use sea_orm::DatabaseTransaction;
use serde_json::{Map, Value};

pub use http::{SecretStatus, SettingsAbilities, SettingsLink, SettingsResponse};
pub(crate) use http::{router, routes};
#[cfg(feature = "mail")]
pub use mail::MailTestResult;

use crate::context::Ctx;
use crate::engine::validate::InputRule;
use crate::ui::{Component, Form, Input, InputType, SelectOption, humanize};
use crate::wire::FieldKind;

/// A media id the declaration checks round-trip through the struct.
const MEDIA_PROBE: &str = "0199d3a4-1b2c-7d3e-8f40-123456789abc";

/// What a setting holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingKind {
    /// Single-line text (`String`, or `Option<String>` when nullable).
    Text,
    /// Multi-line text.
    Textarea,
    /// A 32-bit integer (`i32`).
    Integer,
    /// On or off (`bool`).
    Boolean,
    /// One of the options (`String`).
    Select,
    /// Text keys and values (`BTreeMap<String, String>`).
    KeyValue,
    /// A calendar date (`chrono::NaiveDate`).
    Date,
    /// A [`rustcms_core::settings::Secret`], stored encrypted and never sent back.
    Secret,
    /// A file of the media library (`Option<uuid::Uuid>`; its id).
    Media,
    /// Files of the media library, in order (`Vec<uuid::Uuid>`; their ids).
    MediaList,
}

impl SettingKind {
    fn value_kind(self) -> FieldKind {
        match self {
            Self::Text | Self::Textarea | Self::Select | Self::Secret => FieldKind::String,
            Self::Integer => FieldKind::I32,
            Self::Boolean => FieldKind::Bool,
            Self::KeyValue | Self::MediaList => FieldKind::Json,
            Self::Date => FieldKind::Date,
            Self::Media => FieldKind::Uuid,
        }
    }

    fn input_type(self) -> InputType {
        match self {
            Self::Text => InputType::TextInput,
            Self::Textarea => InputType::Textarea,
            Self::Integer => InputType::NumberInput,
            Self::Boolean => InputType::Toggle,
            Self::Select => InputType::Select,
            Self::KeyValue => InputType::KeyValue,
            Self::Date => InputType::DatePicker,
            Self::Secret => InputType::Secret,
            Self::Media | Self::MediaList => InputType::FileUpload,
        }
    }
}

/// A setting of a group: the struct field it edits and its rules.
#[derive(Debug, Clone)]
pub struct SettingField {
    pub(crate) name: String,
    pub(crate) kind: SettingKind,
    pub(crate) input: Input,
    pub(crate) required: bool,
    pub(crate) nullable: bool,
    pub(crate) max_length: Option<usize>,
    pub(crate) key_value: Option<(usize, usize)>,
    pub(crate) audit_redact: bool,
    pub(crate) kept_while: Vec<String>,
    pub(crate) media: Option<crate::field::MediaRule>,
}

macro_rules! kinds {
    ($($(#[$doc:meta])* $fn:ident => $kind:ident),* $(,)?) => {$(
        $(#[$doc])*
        pub fn $fn(name: impl Into<String>) -> Self {
            Self::new(name.into(), SettingKind::$kind)
        }
    )*};
}

impl SettingField {
    fn new(name: String, kind: SettingKind) -> Self {
        let key_value = (kind == SettingKind::KeyValue).then_some((50, 1_000));
        Self {
            input: Input::with_kind(name.clone(), kind.input_type()),
            name,
            kind,
            required: false,
            nullable: false,
            max_length: None,
            key_value,
            audit_redact: false,
            kept_while: Vec::new(),
            media: None,
        }
    }

    kinds! {
        /// Single-line text.
        text => Text,
        /// Multi-line text.
        textarea => Textarea,
        /// A 32-bit integer.
        integer => Integer,
        /// On or off.
        boolean => Boolean,
        /// Text keys and values (at most 50 pairs of 1,000 characters by default).
        key_value => KeyValue,
        /// A calendar date.
        date => Date,
        /// A secret: stored encrypted, never sent back, redacted in the audit log.
        secret => Secret,
    }

    /// One of `options` (`(value, label)`).
    pub fn select<L: Into<String>>(
        name: impl Into<String>,
        options: impl IntoIterator<Item = (&'static str, L)>,
    ) -> Self {
        let mut field = Self::new(name.into(), SettingKind::Select);
        field.input = field.input.options(options);
        field
    }

    /// A file of the media library, by id (the struct field is an
    /// `Option<uuid::Uuid>`; nullable). On save, a new value must be a file
    /// that exists, is not trashed, is visible to the user saving it (scope of
    /// the media library) and matches [`Self::accept`] and [`Self::max_size`],
    /// as for a resource's [`crate::Field::media`]. Needs
    /// [`crate::AdminProvider::media`].
    pub fn media(name: impl Into<String>) -> Self {
        let mut field = Self::new(name.into(), SettingKind::Media);
        field.nullable = true;
        field.media =
            Some(crate::field::MediaRule { multiple: false, max_items: 1, accept: Vec::new(), max_size: None });
        field
    }

    /// Up to `max` files of the media library, in order (the struct field is a
    /// `Vec<uuid::Uuid>`), checked like [`Self::media`].
    pub fn media_list(name: impl Into<String>, max: usize) -> Self {
        let mut field = Self::new(name.into(), SettingKind::MediaList);
        field.media =
            Some(crate::field::MediaRule { multiple: true, max_items: max.max(1), accept: Vec::new(), max_size: None });
        field
    }

    /// Accepted MIME types of a media setting's files (`image/*`, `application/pdf`).
    pub fn accept(mut self, types: &[&str]) -> Self {
        if let Some(rule) = &mut self.media {
            rule.accept = types.iter().map(|t| (*t).to_owned()).collect();
        }
        self
    }

    /// Largest accepted file of a media setting, in bytes.
    pub fn max_size(mut self, bytes: u64) -> Self {
        if let Some(rule) = &mut self.media {
            rule.max_size = Some(bytes);
        }
        self
    }

    /// Must have a value (not `null`; for secrets: set).
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// May be empty (`null`; the struct field is an `Option`).
    pub fn nullable(mut self) -> Self {
        self.nullable = true;
        self
    }

    /// Longest text, in characters (for key/value: of each value).
    pub fn max_length(mut self, max: usize) -> Self {
        match &mut self.key_value {
            Some((_, length)) => *length = max,
            None => self.max_length = Some(max),
        }
        self
    }

    /// Most key/value pairs.
    pub fn max_items(mut self, max: usize) -> Self {
        if let Some((items, _)) = &mut self.key_value {
            *items = max;
        }
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

    /// Email keyboard and format hint.
    pub fn email(mut self) -> Self {
        self.input = self.input.email();
        self
    }

    /// URL keyboard and format hint.
    pub fn url(mut self) -> Self {
        self.input = self.input.url();
        self
    }

    /// A secret kept only while `fields` keep their values: changing one of
    /// them without giving the secret again clears it (a saved password is
    /// never sent to a server it was not given for).
    pub fn kept_while(mut self, fields: &[&str]) -> Self {
        self.kept_while = fields.iter().map(|f| (*f).to_owned()).collect();
        self
    }

    /// Never store this setting's values in the audit log (only that it changed).
    pub fn audit_redact(mut self) -> Self {
        self.audit_redact = true;
        self
    }

    pub(crate) fn rule(&self) -> InputRule<'_> {
        InputRule {
            kind: self.kind.value_kind(),
            nullable: self.nullable,
            required: self.required,
            max_length: self.max_length,
            key_value: self.key_value,
            rich_text: false,
            options: None,
        }
    }

    pub(crate) fn options(&self) -> Vec<Value> {
        self.input.options.iter().map(|o: &SelectOption| o.value.clone()).collect()
    }

    pub(crate) fn display_label(&self) -> String {
        self.input.label.clone().unwrap_or_else(|| humanize(&self.name))
    }

    fn is_secret(&self) -> bool {
        self.kind == SettingKind::Secret
    }
}

/// Navigation entry of a settings page.
#[derive(Debug, Clone)]
pub struct SettingsMeta {
    pub(crate) label: String,
    pub(crate) icon: Option<String>,
}

impl SettingsMeta {
    /// A page titled `label`.
    pub fn new(label: impl Into<String>) -> Self {
        Self { label: label.into(), icon: None }
    }

    /// Icon name (client icon set).
    pub fn icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = Some(icon.into());
        self
    }
}

/// A settings group edited in the panel.
///
/// ```rust,ignore
/// impl SettingsPage for GeneralSettings {
///     fn meta() -> SettingsMeta { SettingsMeta::new("General") }
///     fn fields() -> Vec<SettingField> {
///         vec![SettingField::text("site_name").required().max_length(100), SettingField::secret("smtp_password")]
///     }
/// }
/// ```
#[async_trait]
pub trait SettingsPage: Settings {
    /// Title and icon.
    fn meta() -> SettingsMeta;

    /// One per field of the struct, in order.
    fn fields() -> Vec<SettingField>;

    /// Layout (sections, tabs) of the fields' inputs; default: one input per field.
    fn form() -> Form {
        Form::default()
    }

    /// Checks the whole group before it is saved (after each value passed its
    /// field's rules). Messages go through the usual issue codes.
    async fn validate(_ctx: &Ctx, _settings: &Self) -> Result<(), ValidationErrors> {
        Ok(())
    }

    /// Like [`Self::validate`] with the application (its configuration and
    /// services); by default, [`Self::validate`].
    async fn validate_in(_state: &AppState, ctx: &Ctx, settings: &Self) -> Result<(), ValidationErrors> {
        Self::validate(ctx, settings).await
    }

    /// Runs in the save's transaction after the new values are written (and
    /// the audit entry recorded), before the commit, with the group as it was
    /// (`before`: defaults when never saved; a secret that could not be
    /// decrypted reads as unset) and as saved (`after`). An error rolls the
    /// save back. Not called when a save changes nothing (nothing is written).
    async fn after_save(
        _state: &AppState,
        _ctx: &Ctx,
        _txn: &DatabaseTransaction,
        _before: &Self,
        _after: &Self,
    ) -> AppResult<()> {
        Ok(())
    }
}

/// A registered settings page, type-erased.
#[async_trait]
pub(crate) trait DynSettings: Send + Sync {
    fn group(&self) -> &'static str;
    fn meta(&self) -> &SettingsMeta;
    fn fields(&self) -> &[SettingField];
    fn form(&self) -> &Form;
    fn defaults(&self) -> &Map<String, Value>;
    /// The group as stored, typed and checked by the application.
    async fn validate(&self, state: &AppState, ctx: &Ctx, stored: &Stored) -> AppResult<()>;
    /// The application's hook after a save, with the group before and after (typed).
    async fn after_save(
        &self,
        state: &AppState,
        ctx: &Ctx,
        txn: &DatabaseTransaction,
        before: &Stored,
        after: &Stored,
    ) -> AppResult<()>;
}

pub(crate) struct SettingsAdapter<T> {
    meta: SettingsMeta,
    fields: Vec<SettingField>,
    form: Form,
    defaults: Map<String, Value>,
    _settings: PhantomData<T>,
}

impl<T: SettingsPage> SettingsAdapter<T> {
    /// Checks the declaration against the struct.
    ///
    /// # Panics
    /// On an invalid group name, duplicate or missing fields, a secret field
    /// that is not a `Secret`, a default that breaks its field's rules, or a
    /// form input that is not a field (secrets: of type `Secret`).
    pub(crate) fn new() -> Self {
        let group = T::GROUP;
        assert!(valid_group(group), "settings `{group}`: groups are 1–32 of `a-z`, `0-9`, `_`");
        let fields = T::fields();
        let Ok(Value::Object(defaults)) = serde_json::to_value(T::default()) else {
            panic!("settings `{group}`: the struct must serialize to an object");
        };
        let mut names = BTreeSet::new();
        for field in &fields {
            assert!(names.insert(field.name.as_str()), "settings `{group}`: field `{}` declared twice", field.name);
            assert!(
                defaults.contains_key(&field.name),
                "settings `{group}`: `{}` is not a field of the struct",
                field.name
            );
        }
        for name in defaults.keys() {
            assert!(names.contains(name.as_str()), "settings `{group}`: struct field `{name}` has no SettingField");
        }
        // Each field round-trips a value of its kind through the struct: a
        // `Secret` hides it (so it must be declared `secret`), a wider or
        // narrower type changes or refuses it, `null` passes only for `Option`s.
        let round_trip = |name: &str, value: Value| -> Option<Value> {
            let mut probe = defaults.clone();
            probe.insert(name.to_owned(), value);
            let typed = serde_json::from_value::<T>(Value::Object(probe)).ok()?;
            serde_json::to_value(typed).ok().map(|v| v[name].clone())
        };
        let exposed = rustcms_core::settings::to_value_exposed(&T::default()).unwrap_or_default();
        for field in &fields {
            let name = field.name.as_str();
            if field.is_secret() {
                assert!(
                    round_trip(name, Value::String("rustcms-secret-probe".into())) == Some(Value::Null),
                    "settings `{group}`: secret field `{name}` must have the type `Secret`"
                );
                assert!(exposed[name].is_null(), "settings `{group}`: the default of secret `{name}` must be unset");
                continue;
            }
            let sample = match field.kind {
                SettingKind::Text | SettingKind::Textarea => Value::String("rustcms-probe".into()),
                SettingKind::Integer => Value::from(7),
                SettingKind::Boolean => Value::Bool(defaults[name] != Value::Bool(true)),
                SettingKind::Select => {
                    let options = field.options();
                    assert!(!options.is_empty(), "settings `{group}`: select `{name}` has no options");
                    options[0].clone()
                }
                SettingKind::KeyValue => serde_json::json!({"key": "value"}),
                SettingKind::Date => Value::String("2001-02-03".into()),
                SettingKind::Media => Value::String(MEDIA_PROBE.into()),
                SettingKind::MediaList => serde_json::json!([MEDIA_PROBE]),
                SettingKind::Secret => unreachable!(),
            };
            assert!(
                round_trip(name, sample.clone()) == Some(sample),
                "settings `{group}`: `{name}` does not hold a value of kind {:?} (a `Secret` is declared with `SettingField::secret`)",
                field.kind
            );
            if field.kind == SettingKind::Integer {
                assert!(
                    round_trip(name, Value::from(i64::from(i32::MAX) + 1)).is_none(),
                    "settings `{group}`: integer `{name}` must have the type `i32`"
                );
            }
            match (field.nullable, round_trip(name, Value::Null)) {
                (true, Some(Value::Null)) | (false, None) => {}
                (true, _) => panic!("settings `{group}`: `{name}` is nullable, so its type must be an `Option`"),
                (false, _) => panic!("settings `{group}`: `{name}` accepts `null`: declare it `.nullable()`"),
            }
            let mut errors = ValidationErrors::new();
            let rule = InputRule { required: false, ..field.rule() };
            crate::engine::validate::check_input(name, &rule, &defaults[name], &mut errors);
            assert!(
                errors.is_empty(),
                "settings `{group}`: the default of `{name}` does not fit its kind ({:?})",
                field.kind
            );
            // A default is never checked against the library: no file by default.
            if field.media.is_some() {
                assert!(
                    matches!(&defaults[name], Value::Null) || defaults[name] == Value::Array(Vec::new()),
                    "settings `{group}`: the default of media `{name}` must be empty"
                );
            }
        }
        let form = T::form();
        crate::engine::adapter::check_form_layout(&format!("settings `{group}`"), &form);
        let mut used = BTreeSet::new();
        for input in form.inputs() {
            let field = fields
                .iter()
                .find(|f| f.name == input.name)
                .unwrap_or_else(|| panic!("settings `{group}`: form input `{}` is not a field", input.name));
            assert!(used.insert(input.name.as_str()), "settings `{group}`: input `{}` appears twice", input.name);
            let compatible = input.kind == field.kind.input_type()
                || (field.kind == SettingKind::Text && input.kind == InputType::Textarea);
            assert!(compatible, "settings `{group}`: input `{}` does not match its field's kind", input.name);
            // One set of values: what the form offers is what the server accepts.
            if !input.options.is_empty() {
                let shown: BTreeSet<String> = input.options.iter().map(|o| o.value.to_string()).collect();
                let accepted: BTreeSet<String> = field.options().iter().map(Value::to_string).collect();
                assert!(
                    shown == accepted,
                    "settings `{group}`: input `{}` offers other values than its field",
                    input.name
                );
            }
        }
        Self { meta: T::meta(), fields, form, defaults, _settings: PhantomData }
    }
}

#[async_trait]
impl<T: SettingsPage> DynSettings for SettingsAdapter<T> {
    fn group(&self) -> &'static str {
        T::GROUP
    }

    fn meta(&self) -> &SettingsMeta {
        &self.meta
    }

    fn fields(&self) -> &[SettingField] {
        &self.fields
    }

    fn form(&self) -> &Form {
        &self.form
    }

    fn defaults(&self) -> &Map<String, Value> {
        &self.defaults
    }

    async fn validate(&self, state: &AppState, ctx: &Ctx, stored: &Stored) -> AppResult<()> {
        let settings: T = typed(stored)?;
        T::validate_in(state, ctx, &settings).await.map_err(|errors| redacted(errors, &[stored]).into())
    }

    async fn after_save(
        &self,
        state: &AppState,
        ctx: &Ctx,
        txn: &DatabaseTransaction,
        before: &Stored,
        after: &Stored,
    ) -> AppResult<()> {
        // A secret that could not be decrypted was replaced or cleared by this
        // save (or it would have been refused): it reads as unset.
        let mut readable = before.clone();
        readable.secrets.retain(|_, state| *state != rustcms_core::settings::SecretState::Unreadable);
        // Stored values that no longer fit the struct (a declaration changed)
        // read as their defaults: the save that replaces them must not fail.
        let old: T = typed(&readable).or_else(|_| {
            readable.values.retain(|name, value| {
                let mut one = Stored::default();
                one.values.insert(name.clone(), value.clone());
                typed::<T>(&one).is_ok()
            });
            // Secrets too (a field that is no longer a `Secret`).
            readable.secrets.retain(|name, state| {
                let mut one = Stored::default();
                one.secrets.insert(name.clone(), state.clone());
                typed::<T>(&one).is_ok()
            });
            typed(&readable)
        })?;
        let new: T = typed(after)?;
        // A message may quote a secret as it was or as it is now: neither is sent.
        // Sent (validation, bad request) or logged (internal): never with a secret.
        T::after_save(state, ctx, txn, &old, &new).await.map_err(|err| match err {
            AppError::Validation(errors) => redacted(errors, &[before, after]).into(),
            AppError::BadRequest(mut message) => {
                Redactor::new(&[before, after]).clean(&mut message);
                AppError::BadRequest(message)
            }
            AppError::Internal(mut message) => {
                Redactor::new(&[before, after]).clean(&mut message);
                AppError::Internal(message)
            }
            other => other,
        })
    }
}

/// Replaces the secrets of each version of the settings given in messages.
/// Longest first, so a secret containing another is never left half redacted.
struct Redactor<'a>(Vec<&'a str>);

impl<'a> Redactor<'a> {
    fn new(stored: &[&'a Stored]) -> Self {
        let mut secrets: Vec<&str> = stored
            .iter()
            .flat_map(|s| s.secrets.values())
            .filter_map(|s| match s {
                rustcms_core::settings::SecretState::Set(plain) if !plain.is_empty() => Some(plain.as_str()),
                _ => None,
            })
            .collect();
        secrets.sort_by(|a, b| b.len().cmp(&a.len()).then(a.cmp(b)));
        secrets.dedup();
        Self(secrets)
    }

    fn clean(&self, message: &mut String) {
        for secret in &self.0 {
            if message.contains(secret) {
                *message = message.replace(secret, rustcms_core::audit::REDACTED);
            }
        }
    }
}

/// `errors` without the secrets of `stored` (each version given): a hook's
/// message may quote one.
fn redacted(mut errors: ValidationErrors, stored: &[&Stored]) -> ValidationErrors {
    let redactor = Redactor::new(stored);
    for messages in errors.errors.values_mut() {
        messages.iter_mut().for_each(|m| redactor.clean(m));
    }
    errors.form_errors.iter_mut().for_each(|m| redactor.clean(m));
    errors
}

/// The settings pages of the panel, by group (see [`crate::AdminProvider::settings`]).
#[derive(Default, Clone)]
pub struct SettingsRegistry {
    pages: BTreeMap<&'static str, Arc<dyn DynSettings>>,
    order: Vec<&'static str>,
}

impl SettingsRegistry {
    pub(crate) fn register<T: SettingsPage>(&mut self) {
        let page = SettingsAdapter::<T>::new();
        assert!(!self.pages.contains_key(T::GROUP), "settings group `{}` registered twice", T::GROUP);
        self.order.push(T::GROUP);
        self.pages.insert(T::GROUP, Arc::new(page));
    }

    pub(crate) fn get(&self, group: &str) -> Option<&Arc<dyn DynSettings>> {
        self.pages.get(group)
    }

    pub(crate) fn all(&self) -> impl Iterator<Item = &Arc<dyn DynSettings>> {
        self.order.iter().filter_map(|g| self.pages.get(g))
    }

    /// Labels to translate (`lang:missing`).
    pub(crate) fn display_strings(&self) -> Vec<String> {
        let mut out = Vec::new();
        for page in self.all() {
            out.push(page.meta().label.clone());
            for field in page.fields() {
                out.push(field.display_label());
                out.extend(field.input.help.clone());
                out.extend(field.input.placeholder.clone());
                out.extend(field.input.options.iter().map(|o| o.label.clone()));
            }
            for component in page.form().components.iter() {
                walk_labels(component, &mut out);
            }
        }
        out
    }
}

fn walk_labels(component: &Component, out: &mut Vec<String>) {
    match component {
        Component::Layout(layout) => {
            out.extend(layout.label.clone());
            for child in &layout.children {
                walk_labels(child, out);
            }
        }
        // What an input of the form overrides is shown too.
        Component::Input(input) => {
            out.extend(input.label.clone());
            out.extend(input.help.clone());
            out.extend(input.placeholder.clone());
            out.extend(input.options.iter().map(|o| o.label.clone()));
        }
    }
}

/// `settings.<group>.view` and `.update`.
pub(crate) fn permissions(group: &str) -> [String; 2] {
    [format!("settings.{group}.view"), format!("settings.{group}.update")]
}
