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
    for resource in ctx.registry.all() {
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
    Ok(Json(PanelResponse {
        brand: panel.brand.clone(),
        user: PanelUser { id: auth.user.id.to_string(), name: auth.user.name.clone(), email: auth.user.email.clone() },
        navigation,
        system: crate::system::SystemLinks::of(&ctx.principal),
        settings: panel.settings.links(&ctx),
        pages: crate::custom::links(&panel.custom, &ctx),
        home: crate::custom::home(&panel.custom, &ctx),
        permissions,
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
    Ok(Json(resource(&ctx, &slug)?.list(&ctx, &query).await?))
}

pub(super) async fn find(
    AdminCtx(ctx): AdminCtx,
    Path((slug, key)): Path<(String, String)>,
) -> AppResult<Json<DataResponse<crate::Envelope>>> {
    let record = resource(&ctx, &slug)?.find(&ctx, &key).await?;
    Ok(Json(DataResponse { data: record }))
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
