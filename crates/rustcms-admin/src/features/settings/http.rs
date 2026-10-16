//! `GET` and `PUT <admin>/api/settings/{group}`.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{FromRequest, Path, Request, State};
use axum::http::Method;
use axum::routing::get;
use rustcms_core::audit::{self, AuditEntry, REDACTED};
use rustcms_core::settings::{self, SecretState, Stored};
use rustcms_core::{AppError, AppResult, AppState, Json, Principal, ValidationErrors};
use serde::Serialize;
use serde_json::{Map, Value, json};

use super::{DynSettings, SettingField, SettingsRegistry};
use crate::context::Ctx;
use crate::engine::validate::{InputRule, check_input};
use crate::http::{AdminCtx, AdminPanel};
use crate::schema::{ComponentSchema, FormSchema, InputSchema, LayoutSchema, Rule, SCHEMA_VERSION, VOCABULARY_VERSION};
use crate::ui::{Component, SelectOption};

/// Every settings route as `(method, path)`.
pub(crate) fn routes(admin_path: &str) -> Vec<(Method, String)> {
    let path = format!("{}/api/settings/{{group}}", admin_path.trim_end_matches('/'));
    #[cfg_attr(not(feature = "mail"), allow(unused_mut))]
    let mut routes = vec![(Method::GET, path.clone()), (Method::PUT, path)];
    #[cfg(feature = "mail")]
    routes.push((Method::POST, format!("{}/api/settings/mail/test", admin_path.trim_end_matches('/'))));
    routes
}

pub(crate) fn router(router: Router<AppState>, admin_path: &str) -> Router<AppState> {
    let path = format!("{}/api/settings/{{group}}", admin_path.trim_end_matches('/'));
    let router = router.route(&path, get(show).put(update));
    #[cfg(feature = "mail")]
    let router = router.route(
        &format!("{}/api/settings/mail/test", admin_path.trim_end_matches('/')),
        axum::routing::post(super::mail::test),
    );
    router
}

/// A settings page in the navigation (`PanelResponse.settings`).
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SettingsLink {
    /// The group (URL segment).
    pub group: String,
    /// Title.
    pub label: String,
    /// Icon name.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

/// Whether a secret is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum SecretStatus {
    /// A value is stored.
    Set,
    /// No value.
    Unset,
    /// A value is stored but cannot be decrypted (`APP_KEY` changed): replace or clear it.
    Unreadable,
}

/// What the user may do on a settings page.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SettingsAbilities {
    /// Save changes.
    pub update: bool,
    /// Send a test mail with the saved settings (the Mail page).
    pub test: bool,
}

/// `GET` / `PUT settings/{group}`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SettingsResponse {
    /// See [`SCHEMA_VERSION`].
    pub schema_version: u32,
    /// See [`VOCABULARY_VERSION`].
    pub vocabulary_version: u32,
    /// The group.
    pub group: String,
    /// Title.
    pub label: String,
    /// The form (inputs disabled without `update`).
    pub form: FormSchema,
    /// Values of the settings other than secrets (defaults for unsaved ones).
    #[cfg_attr(feature = "ts", ts(type = "Record<string, JsonValue>"))]
    pub data: Map<String, Value>,
    /// State of each secret; their values are never sent.
    pub secrets: BTreeMap<String, SecretStatus>,
    /// Version to send back when saving (`"0"`: never saved).
    pub version: String,
    /// What the user may do.
    pub abilities: SettingsAbilities,
}

impl SettingsRegistry {
    /// Pages `principal` may open.
    pub(crate) fn links(&self, ctx: &Ctx) -> Vec<SettingsLink> {
        self.all()
            .filter(|page| ctx.principal.can(&super::permissions(page.group())[0]))
            .map(|page| SettingsLink {
                group: page.group().to_owned(),
                label: ctx.t(&page.meta().label),
                icon: page.meta().icon.clone(),
            })
            .collect()
    }
}

pub(super) fn registry(state: &AppState) -> AppResult<Arc<SettingsRegistry>> {
    Ok(state
        .service::<AdminPanel>()
        .ok_or_else(|| AppError::internal("AdminProvider not registered"))?
        .settings
        .clone())
}

/// The page `group` if `principal` may view it (and update it, with
/// `update`). An unknown group is a 404 only for users who may view some
/// settings: others learn nothing about which groups exist.
pub(super) fn admit(
    settings: &SettingsRegistry,
    principal: &Principal,
    group: &str,
    update: bool,
) -> AppResult<Arc<dyn DynSettings>> {
    let allowed = |group: &str| {
        let [view, edit] = super::permissions(group);
        principal.can(&view) && (!update || principal.can(&edit))
    };
    match settings.get(group) {
        Some(page) if allowed(group) => Ok(page.clone()),
        Some(_) => Err(AppError::Forbidden),
        None if settings.all().any(|p| principal.can(&super::permissions(p.group())[0])) => Err(AppError::NotFound),
        None => Err(AppError::Forbidden),
    }
}

async fn show(
    State(state): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Path(group): Path<String>,
) -> AppResult<Json<SettingsResponse>> {
    let page = admit(registry(&state)?.as_ref(), &ctx.principal, &group, false)?;
    let stored = settings::read(state.db(), &state.config().app.key, page.group(), false).await?;
    Ok(Json(response(&ctx, page.as_ref(), &stored)))
}

fn response(ctx: &Ctx, page: &dyn DynSettings, stored: &Stored) -> SettingsResponse {
    let update = ctx.principal.can(&super::permissions(page.group())[1]);
    let mut data = Map::new();
    let mut secrets = BTreeMap::new();
    for field in page.fields() {
        if field.is_secret() {
            let status = match stored.secret(&field.name) {
                SecretState::Set(_) => SecretStatus::Set,
                SecretState::Unset => SecretStatus::Unset,
                SecretState::Unreadable => SecretStatus::Unreadable,
            };
            secrets.insert(field.name.clone(), status);
        } else {
            let value = stored.values.get(&field.name).unwrap_or(&page.defaults()[&field.name]);
            data.insert(field.name.clone(), value.clone());
        }
    }
    let components = if page.form().components.is_empty() {
        page.fields().iter().map(|f| ComponentSchema::Input(input_schema(ctx, f, &f.input, !update))).collect()
    } else {
        components(ctx, page, &page.form().components, !update)
    };
    SettingsResponse {
        schema_version: SCHEMA_VERSION,
        vocabulary_version: VOCABULARY_VERSION,
        group: page.group().to_owned(),
        label: ctx.t(&page.meta().label),
        form: FormSchema { columns: page.form().columns.max(1), components },
        data,
        secrets,
        version: stored.version.to_string(),
        abilities: super::SettingsAbilities {
            update,
            test: update && page.group() == "mail" && cfg!(feature = "mail"),
        },
    }
}

fn components(ctx: &Ctx, page: &dyn DynSettings, items: &[Component], disabled: bool) -> Vec<ComponentSchema> {
    items
        .iter()
        .filter_map(|component| match component {
            Component::Input(input) => page
                .fields()
                .iter()
                .find(|f| f.name == input.name)
                .map(|field| ComponentSchema::Input(input_schema(ctx, field, input, disabled))),
            Component::Layout(layout) => Some(ComponentSchema::Layout(LayoutSchema {
                kind: layout.kind,
                id: layout.id.clone(),
                label: layout.label.as_deref().map(|l| ctx.t(l)),
                columns: layout.columns,
                components: components(ctx, page, &layout.children, disabled),
            })),
        })
        .collect()
}

fn input_schema(ctx: &Ctx, field: &SettingField, input: &crate::ui::Input, disabled: bool) -> InputSchema {
    let mut rules = Vec::new();
    if field.required && !disabled && !field.is_secret() {
        rules.push(Rule::Required);
    }
    if let Some((items, length)) = field.key_value {
        rules.push(Rule::MaxItems { value: items });
        rules.push(Rule::MaxLength { value: length });
    } else if let Some(max) = field.max_length {
        rules.push(Rule::MaxLength { value: max });
    }
    let options = if input.options.is_empty() { &field.input.options } else { &input.options };
    InputSchema {
        kind: input.kind,
        name: field.name.clone(),
        label: ctx.t(&input.label.clone().unwrap_or_else(|| field.display_label())),
        value_type: field.kind.value_kind().name(),
        nullable: field.nullable,
        default: None,
        disabled,
        write_only: field.is_secret(),
        rules,
        column_span: input.column_span,
        placeholder: input.placeholder.as_deref().or(field.input.placeholder.as_deref()).map(|p| ctx.t(p)),
        help: input.help.as_deref().or(field.input.help.as_deref()).map(|h| ctx.t(h)),
        mode: input.mode.or(field.input.mode),
        options: options.iter().map(|o| SelectOption { value: o.value.clone(), label: ctx.t(&o.label) }).collect(),
        relationship: None,
        widget: None,
        props: None,
        media: None,
    }
}

/// Parses the body of a `PUT`: `{version, data}`.
fn parse_body(body: &[u8]) -> AppResult<(i64, Map<String, Value>)> {
    let invalid = |field: &str, message: &str| -> AppError { ValidationErrors::field(field, message).into() };
    let Ok(Value::Object(mut body)) = serde_json::from_slice::<Value>(body) else {
        let mut errors = ValidationErrors::new();
        errors.add_form("is invalid");
        return Err(errors.into());
    };
    let version = match body.remove("version") {
        Some(Value::String(v)) => v.parse::<i64>().ok().filter(|v| *v >= 0),
        _ => None,
    }
    .ok_or_else(|| invalid("version", "must be an integer string"))?;
    let data = match body.remove("data") {
        Some(Value::Object(data)) => data,
        _ => return Err(invalid("data", "is required")),
    };
    Ok((version, data))
}

/// What the request does to a secret.
enum SecretWrite {
    Keep,
    Replace(String),
    Clear,
}

async fn update(
    State(state): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Path(group): Path<String>,
    request: Request,
) -> AppResult<Json<SettingsResponse>> {
    let settings = registry(&state)?;
    // Authorized before the body is even read: a caller without access sends
    // nothing the server waits for, and learns nothing from 422s.
    let page = admit(&settings, &ctx.principal, &group, true)?;
    let body = Bytes::from_request(request, &state).await.map_err(|e| {
        if e.status() == axum::http::StatusCode::PAYLOAD_TOO_LARGE {
            AppError::PayloadTooLarge
        } else {
            AppError::BadRequest("the body could not be read".into())
        }
    })?;
    let (expected, data) = parse_body(&body)?;
    let key = &state.config().app.key;

    // Values checked against their fields before any lock.
    let mut errors = ValidationErrors::new();
    let mut writes = BTreeMap::new();
    for (name, value) in &data {
        let Some(field) = page.fields().iter().find(|f| &f.name == name) else {
            errors.add(name.as_str(), "is not allowed");
            continue;
        };
        if field.is_secret() {
            match value {
                Value::Null => {
                    writes.insert(name.clone(), SecretWrite::Clear);
                }
                Value::String(s) if s.is_empty() => errors.add(name.as_str(), "is empty"),
                Value::String(s) => {
                    let rule = InputRule { required: false, ..field.rule() };
                    if check_input(name, &rule, value, &mut errors) {
                        writes.insert(name.clone(), SecretWrite::Replace(s.clone()));
                    }
                }
                _ => errors.add(name.as_str(), "must be a string"),
            }
            continue;
        }
        let options = field.options();
        let rule = InputRule { options: (!options.is_empty()).then_some(options.as_slice()), ..field.rule() };
        check_input(name, &rule, value, &mut errors);
    }
    errors.into_result()?;

    // Under the RBAC lock, with the user's grants as they are now.
    let (txn, current) = rustcms_auth::rbac::begin_as(state.db(), &ctx.principal).await?;
    let txn = Arc::new(txn);
    admit(&settings, &current, &group, true)?;
    let stored = settings::read(txn.as_ref(), key, page.group(), true).await?;
    if stored.version != expected {
        let mut errors = ValidationErrors::new();
        errors.add_form("This was changed by someone else meanwhile. Reload and try again.");
        return Err(errors.into());
    }

    let mut values = Map::new();
    let mut secrets = BTreeMap::new();
    let mut errors = ValidationErrors::new();
    // A secret bound to other fields is not kept when one of them changes.
    let changed = |name: &String| {
        data.get(name).is_some_and(|value| Some(value) != stored.values.get(name).or_else(|| page.defaults().get(name)))
    };
    for field in page.fields() {
        if field.is_secret() {
            let mut write = writes.remove(&field.name).unwrap_or(SecretWrite::Keep);
            if matches!(write, SecretWrite::Keep) && field.kept_while.iter().any(changed) {
                write = SecretWrite::Clear;
            }
            let after = match (write, stored.secret(&field.name)) {
                (SecretWrite::Replace(plain), _) => Some(plain),
                (SecretWrite::Clear, _) | (SecretWrite::Keep, SecretState::Unset) => None,
                (SecretWrite::Keep, SecretState::Set(plain)) => Some(plain.clone()),
                // Never dropped silently: the user replaces or clears it.
                (SecretWrite::Keep, SecretState::Unreadable) => {
                    errors.add(field.name.as_str(), "cannot be decrypted; replace or clear it");
                    None
                }
            };
            match after {
                Some(plain) => {
                    secrets.insert(field.name.clone(), plain);
                }
                None if field.required => errors.add(field.name.as_str(), "is required"),
                None => {}
            }
        } else {
            let value = data
                .get(&field.name)
                .or_else(|| stored.values.get(&field.name))
                .unwrap_or(&page.defaults()[&field.name])
                .clone();
            if value.is_null() && field.required {
                errors.add(field.name.as_str(), "is required");
            }
            values.insert(field.name.clone(), value);
        }
    }
    errors.into_result()?;

    let after = Stored {
        version: stored.version + 1,
        values: values.clone(),
        secrets: secrets.iter().map(|(k, v)| (k.clone(), SecretState::Set(v.clone()))).collect(),
    };
    // The application's check runs inside the write, as the user is now.
    let mut within = ctx.within(txn.clone());
    within.principal = current.clone();
    page.validate(&state, &within, &after).await?;
    drop(within);
    let txn =
        Arc::try_unwrap(txn).map_err(|_| AppError::internal("settings: the write transaction is still in use"))?;

    let changes = changes(page.as_ref(), &stored, &after);
    if changes.is_empty() {
        // Nothing changed: no new version, no entry.
        txn.rollback().await?;
        return Ok(Json(response(&ctx, page.as_ref(), &stored)));
    }
    let user_id = current.id.parse().ok();
    settings::write(&txn, key, page.group(), after.version, values, &secrets, user_id).await?;
    let entry = AuditEntry::new("settings.updated", "settings").key(page.group()).changes(Value::Object(changes));
    audit::record(&txn, &ctx.audit_context, entry).await?;
    txn.commit().await?;
    Ok(Json(response(&ctx, page.as_ref(), &after)))
}

/// `{field: [old, new]}` of what changed; secrets and redacted settings as
/// `"[redacted]"` (or `null` when unset).
fn changes(page: &dyn DynSettings, before: &Stored, after: &Stored) -> Map<String, Value> {
    let mut out = Map::new();
    for field in page.fields() {
        let name = &field.name;
        if field.is_secret() {
            let (old, new) = (before.secret(name), after.secret(name));
            if old != new {
                let shown = |s: &SecretState| if *s == SecretState::Unset { Value::Null } else { json!(REDACTED) };
                out.insert(name.clone(), json!([shown(old), shown(new)]));
            }
            continue;
        }
        let default = &page.defaults()[name];
        let old = before.values.get(name).unwrap_or(default);
        let new = after.values.get(name).unwrap_or(default);
        if old != new {
            let pair = if field.audit_redact { json!([json!(REDACTED), json!(REDACTED)]) } else { json!([old, new]) };
            out.insert(name.clone(), pair);
        }
    }
    out
}
