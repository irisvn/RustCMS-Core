//! Handlers of the panel, schemas, records and options.

use super::AdminCtx;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use rustcms_auth::Auth;
use rustcms_core::{AppError, AppResult, AppState, Json};
use serde::Deserialize;
use serde_json::Value;

use crate::Data;
use crate::policy::ability;
use crate::resource::{Filter, FilterOp, ListQuery};
use crate::schema::SchemaOperation;

use super::AdminPanel;
use super::PanelResponse;
use super::PanelUser;
use super::{DataResponse, resource};

pub(super) async fn panel(
    State(state): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Auth(auth): Auth,
) -> AppResult<Json<PanelResponse>> {
    let panel = state.service::<AdminPanel>().ok_or_else(|| AppError::internal("AdminProvider not registered"))?;
    let mut navigation = Vec::new();
    // In registration order: the application's, not the alphabet's.
    for resource in ctx.registry.in_registration_order() {
        if resource.allows(&ctx, ability::VIEW_ANY).await {
            navigation.push(resource.info().translated(&ctx));
        }
    }
    // Declared admin-guard names only: other guards and undeclared or
    // orphaned grants are never listed.
    let declared = state.service::<rustcms_auth::AuthState>().map(|auth| auth.permissions.all()).unwrap_or_default();
    let permissions: Vec<String> = declared
        .into_keys()
        .filter(|(guard, name)| guard == rustcms_auth::ADMIN_GUARD && ctx.principal.can(name))
        .map(|(_, name)| name)
        .collect();
    // The current branding's name (the SPA shows the one its page loaded with).
    let brand = match state.service::<crate::custom::AdminBranding>() {
        Some(branding) => branding.current(&state).await.brand.clone(),
        None => panel.brand.clone(),
    };
    Ok(Json(PanelResponse {
        brand,
        user: PanelUser { id: auth.user.id.to_string(), name: auth.user.name.clone(), email: auth.user.email.clone() },
        navigation,
        system: crate::system::SystemLinks::of(&ctx.principal),
        settings: panel.settings.links(&ctx),
        pages: crate::custom::links(&panel.custom, &ctx),
        home: crate::custom::home(&panel.custom, &ctx),
        permissions,
        uploads: crate::media::upload_limits(&state, &ctx),
        media_collections: crate::media::panel_collections(&state, &ctx),
    }))
}

#[derive(Deserialize)]
pub(super) struct SchemaQuery {
    operation: Option<SchemaOperation>,
    key: Option<String>,
}

pub(super) async fn schema(
    AdminCtx(ctx): AdminCtx,
    Path(slug): Path<String>,
    Query(query): Query<SchemaQuery>,
) -> AppResult<Json<crate::ResourceSchema>> {
    let op =
        query.operation.unwrap_or(if query.key.is_some() { SchemaOperation::Edit } else { SchemaOperation::Create });
    Ok(Json(resource(&ctx, &slug)?.schema(&ctx, op, query.key.as_deref()).await?))
}

/// `page`, `per_page`, `sort`, `search`, `trashed`, `filter[field][op]=value`.
pub(crate) fn list_query(pairs: Vec<(String, String)>) -> AppResult<ListQuery> {
    let mut query = ListQuery { from_query_string: true, ..Default::default() };
    let number = |name: &str, value: &str| {
        value
            .parse::<u64>()
            .map_err(|_| AppError::from(rustcms_core::ValidationErrors::field(name, "must be a positive integer")))
    };
    for (key, value) in pairs {
        match key.as_str() {
            "page" => query.page = Some(number("page", &value)?),
            "per_page" => query.per_page = Some(number("per_page", &value)?),
            "sort" => query.sort = Some(value),
            "search" => query.search = Some(value),
            "trashed" => {
                query.trashed = serde_json::from_value(Value::String(value)).map_err(|_| {
                    AppError::from(rustcms_core::ValidationErrors::field("trashed", "must be `with` or `only`"))
                })?;
            }
            _ => {
                let Some(rest) = key.strip_prefix("filter[") else { continue };
                let parsed = rest.strip_suffix(']').and_then(|r| r.split_once("]["));
                let Some((field, op)) = parsed else {
                    return Err(rustcms_core::ValidationErrors::field("filters", format!("malformed `{key}`")).into());
                };
                let op: FilterOp = serde_json::from_value(Value::String(op.to_owned())).map_err(|_| {
                    AppError::from(rustcms_core::ValidationErrors::field(field, format!("unknown operator `{op}`")))
                })?;
                query.filters.push(Filter { field: field.to_owned(), op, value: Value::String(value) });
            }
        }
    }
    Ok(query)
}

pub(super) async fn list(
    AdminCtx(ctx): AdminCtx,
    Path(slug): Path<String>,
    Query(pairs): Query<Vec<(String, String)>>,
) -> AppResult<Json<crate::Page>> {
    let query = list_query(pairs)?;
    let resource = resource(&ctx, &slug)?;
    let mut page = resource.list(&ctx, &query).await?;
    // The table's count columns (`TextColumn::counts`): this route only.
    resource.add_counts(&ctx, &mut page.data).await;
    Ok(Json(page))
}

pub(super) async fn find(
    AdminCtx(ctx): AdminCtx,
    Path((slug, key)): Path<(String, String)>,
) -> AppResult<Json<DataResponse<crate::Envelope>>> {
    let record = resource(&ctx, &slug)?.find(&ctx, &key).await?;
    Ok(Json(DataResponse { data: record }))
}

/// Computed-value requests per user per minute (each runs application code).
const COMPUTED_PER_MINUTE: u32 = 60;

/// The computed values of a record's view page (ADR 0039), limited per user.
pub(super) async fn computed(
    State(app): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Path((slug, key)): Path<(String, String)>,
) -> AppResult<Json<crate::schema::ComputedValues>> {
    let auth =
        app.service::<rustcms_auth::AuthState>().ok_or_else(|| AppError::internal("AuthProvider not registered"))?;
    auth.limiter.reserve(&[(&format!("computed:{}", ctx.principal.id), COMPUTED_PER_MINUTE)]).await?;
    Ok(Json(resource(&ctx, &slug)?.computed(&ctx, &key).await?))
}

/// Form reaction and options requests per user per minute (each runs
/// application code while the user types).
const REACTIONS_PER_MINUTE: u32 = 120;

/// The changes a form's reaction asks for (ADR 0043), limited per user. Who
/// may fill one of the resource's forms is checked, and the request counted,
/// before the body is read (the same 403 whatever it holds).
pub(super) async fn react(
    State(app): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Path(slug): Path<String>,
    request: axum::extract::Request,
) -> AppResult<Json<crate::schema::FormReactResponse>> {
    let target = form_writer(&app, &ctx, &slug).await?;
    let request: crate::schema::FormReactRequest = super::extract::json_body(request, &app).await?;
    let ctx = through(ctx, target.as_ref(), &slug, request.via.as_ref(), request.operation).await?;
    Ok(Json(target.react(&ctx, request).await?))
}

/// The options computed on the server of a form's input (ADR 0044, part E),
/// authorized and limited as reactions are (one bucket for both).
pub(super) async fn form_options(
    State(app): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Path(slug): Path<String>,
    request: axum::extract::Request,
) -> AppResult<Json<crate::schema::FormOptionsResponse>> {
    let target = form_writer(&app, &ctx, &slug).await?;
    let request: crate::schema::FormOptionsRequest = super::extract::json_body(request, &app).await?;
    let ctx = through(ctx, target.as_ref(), &slug, request.via.as_ref(), request.operation).await?;
    Ok(Json(target.form_options(&ctx, request).await?))
}

/// The resource `slug` when the user may fill one of its forms (create or
/// update admitted), the request counted in the user's `form-react` bucket.
async fn form_writer(
    app: &AppState,
    ctx: &crate::Ctx,
    slug: &str,
) -> AppResult<std::sync::Arc<dyn crate::registry::DynResource>> {
    let target = resource(ctx, slug)?;
    if !target.admits(ctx, ability::CREATE).await && !target.admits(ctx, ability::UPDATE).await {
        return Err(AppError::Forbidden);
    }
    let auth =
        app.service::<rustcms_auth::AuthState>().ok_or_else(|| AppError::internal("AuthProvider not registered"))?;
    auth.limiter.reserve(&[(&format!("form-react:{}", ctx.principal.id), REACTIONS_PER_MINUTE)]).await?;
    Ok(target)
}

/// A create form opened through a relation: authorized as that create is
/// (`relation_create`), the relation's key bound as the save binds it.
async fn through(
    ctx: crate::Ctx,
    target: &dyn crate::registry::DynResource,
    slug: &str,
    via: Option<&crate::schema::FormReactVia>,
    operation: SchemaOperation,
) -> AppResult<crate::Ctx> {
    let Some(via) = via else { return Ok(ctx) };
    let (parent, relation, _) = super::relations::relation_of(&ctx, &via.parent, &via.relation)?;
    if relation.is_many()
        || relation.target != slug
        || operation != SchemaOperation::Create
        || !crate::registry::relation_access(&ctx, target, &relation).creatable
        || !parent.admits(&ctx, ability::VIEW).await
    {
        return Err(AppError::Forbidden);
    }
    let canonical = parent.visible_key(&ctx, &via.record).await?;
    Ok(ctx.with_bound((format!("{}.{}", via.parent, via.relation), relation.field.to_owned(), canonical)))
}

/// The request's `Idempotency-Key`, validated.
pub(crate) fn idempotency_key(headers: &axum::http::HeaderMap) -> AppResult<Option<String>> {
    match headers.get(crate::idempotency::HEADER) {
        None => Ok(None),
        Some(value) => {
            let text = value.to_str().unwrap_or("\u{0}");
            crate::idempotency::parse(text).map(Some)
        }
    }
}

pub(super) async fn create(
    AdminCtx(ctx): AdminCtx,
    Path(slug): Path<String>,
    headers: axum::http::HeaderMap,
    Json(input): Json<Data>,
) -> AppResult<(StatusCode, Json<DataResponse<crate::Envelope>>)> {
    let ctx = ctx.with_idempotency_key(idempotency_key(&headers)?);
    let record = resource(&ctx, &slug)?.create(&ctx, input).await?;
    Ok((StatusCode::CREATED, Json(DataResponse { data: record })))
}

pub(super) async fn update(
    AdminCtx(ctx): AdminCtx,
    Path((slug, key)): Path<(String, String)>,
    Json(input): Json<Data>,
) -> AppResult<Json<DataResponse<crate::Envelope>>> {
    let record = resource(&ctx, &slug)?.update(&ctx, &key, input).await?;
    Ok(Json(DataResponse { data: record }))
}

pub(super) async fn destroy(
    AdminCtx(ctx): AdminCtx,
    Path((slug, key)): Path<(String, String)>,
) -> AppResult<StatusCode> {
    resource(&ctx, &slug)?.delete(&ctx, &key).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn restore(
    AdminCtx(ctx): AdminCtx,
    Path((slug, key)): Path<(String, String)>,
) -> AppResult<Json<DataResponse<crate::Envelope>>> {
    let record = resource(&ctx, &slug)?.restore(&ctx, &key).await?;
    Ok(Json(DataResponse { data: record }))
}

pub(super) async fn force_delete(
    AdminCtx(ctx): AdminCtx,
    Path((slug, key)): Path<(String, String)>,
) -> AppResult<StatusCode> {
    resource(&ctx, &slug)?.force_delete(&ctx, &key).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub(super) struct OptionsQuery {
    search: Option<String>,
    keys: Option<String>,
    /// Key of the record being edited or viewed.
    record: Option<String>,
}

pub(super) async fn options(
    AdminCtx(ctx): AdminCtx,
    Path((slug, field)): Path<(String, String)>,
    Query(query): Query<OptionsQuery>,
) -> AppResult<Json<DataResponse<Vec<crate::ui::SelectOption>>>> {
    let keys: Vec<String> = query
        .keys
        .as_deref()
        .map(|k| k.split(',').filter(|k| !k.is_empty()).map(str::to_owned).collect())
        .unwrap_or_default();
    let options =
        resource(&ctx, &slug)?.options(&ctx, &field, query.search.as_deref(), &keys, query.record.as_deref()).await?;
    Ok(Json(DataResponse { data: options }))
}
