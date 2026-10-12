//! Admin API routes (`<admin_path>/api/…`), behind the admin session guard.

use std::sync::Arc;

use axum::Router;
use axum::extract::{FromRequestParts, Path, Query, State};
use axum::http::request::Parts;
use axum::http::{Method, StatusCode};
use axum::routing::{get, post};
use rustcms_auth::{ADMIN_GUARD, Auth};
use rustcms_core::{AppError, AppResult, AppState, Json};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::context::Ctx;
use crate::policy::ability;
use crate::registry::{DynResource, Registry};
use crate::resource::{Filter, FilterOp, ListQuery};
use crate::schema::SchemaOperation;
use crate::{ActionOutcome, Data};

/// The admin panel: brand and resources. Bound by [`crate::AdminProvider`].
pub struct AdminPanel {
    /// Name shown in the panel.
    pub brand: String,
    /// Registered resources.
    pub registry: Arc<Registry>,
}

/// `GET panel`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PanelResponse {
    /// Name shown in the panel.
    pub brand: String,
    /// Signed-in user.
    pub user: PanelUser,
    /// Resources the user may list (`view_any`), in registration order.
    pub navigation: Vec<crate::ResourceInfo>,
    /// System pages the user may open.
    pub system: crate::system::SystemLinks,
}

/// The signed-in user in [`PanelResponse`].
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PanelUser {
    /// User id (string).
    pub id: String,
    /// Display name.
    pub name: String,
    /// Email.
    pub email: String,
}

/// `{data: …}` wrapper of single records and options.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct DataResponse<T> {
    /// Payload.
    pub data: T,
}

/// Every admin API route as `(method, path)` with `{slug}`-style placeholders,
/// for route listings and the authorization oracle.
pub fn routes(admin_path: &str) -> Vec<(Method, String)> {
    let base = format!("{}/api", admin_path.trim_end_matches('/'));
    let resource = format!("{base}/resources/{{slug}}");
    vec![
        (Method::GET, format!("{base}/panel")),
        (Method::GET, format!("{resource}/schema")),
        (Method::GET, format!("{resource}/records")),
        (Method::POST, format!("{resource}/records")),
        (Method::GET, format!("{resource}/records/{{key}}")),
        (Method::PATCH, format!("{resource}/records/{{key}}")),
        (Method::DELETE, format!("{resource}/records/{{key}}")),
        (Method::POST, format!("{resource}/actions/{{action}}")),
        (Method::GET, format!("{resource}/fields/{{field}}/options")),
    ]
    .into_iter()
    .chain(crate::system::routes(admin_path))
    .collect()
}

pub(crate) fn router(router: Router<AppState>, admin_path: &str) -> Router<AppState> {
    let base = format!("{}/api", admin_path.trim_end_matches('/'));
    let resource = format!("{base}/resources/{{slug}}");
    router
        .route(&format!("{base}/panel"), get(panel))
        .route(&format!("{resource}/schema"), get(schema))
        .route(&format!("{resource}/records"), get(list).post(create))
        .route(&format!("{resource}/records/{{key}}"), get(find).patch(update).delete(destroy))
        .route(&format!("{resource}/actions/{{action}}"), post(run_action))
        .route(&format!("{resource}/fields/{{field}}/options"), get(options))
}

/// Request context for admin handlers: an authenticated admin-guard user.
pub struct AdminCtx(pub Ctx);

impl FromRequestParts<AppState> for AdminCtx {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, AppError> {
        let Auth(auth) = Auth::from_request_parts(parts, state).await?;
        if auth.principal.guard != ADMIN_GUARD {
            return Err(AppError::Unauthenticated);
        }
        let panel = state.service::<AdminPanel>().ok_or_else(|| AppError::internal("AdminProvider not registered"))?;
        Ok(Self(Ctx::new(state.db().clone(), auth.principal, panel.registry.clone())))
    }
}

fn resource(ctx: &Ctx, slug: &str) -> AppResult<Arc<dyn DynResource>> {
    ctx.registry.get(slug).cloned().ok_or(AppError::NotFound)
}

async fn panel(
    State(state): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Auth(auth): Auth,
) -> AppResult<Json<PanelResponse>> {
    let panel = state.service::<AdminPanel>().ok_or_else(|| AppError::internal("AdminProvider not registered"))?;
    let mut navigation = Vec::new();
    for resource in ctx.registry.all() {
        if resource.allows(&ctx, ability::VIEW_ANY).await {
            navigation.push(resource.info());
        }
    }
    Ok(Json(PanelResponse {
        brand: panel.brand.clone(),
        user: PanelUser { id: auth.user.id.to_string(), name: auth.user.name.clone(), email: auth.user.email.clone() },
        navigation,
        system: crate::system::SystemLinks::of(&ctx.principal),
    }))
}

#[derive(Deserialize)]
struct SchemaQuery {
    operation: Option<SchemaOperation>,
    key: Option<String>,
}

async fn schema(
    AdminCtx(ctx): AdminCtx,
    Path(slug): Path<String>,
    Query(query): Query<SchemaQuery>,
) -> AppResult<Json<crate::ResourceSchema>> {
    let op =
        query.operation.unwrap_or(if query.key.is_some() { SchemaOperation::Edit } else { SchemaOperation::Create });
    Ok(Json(resource(&ctx, &slug)?.schema(&ctx, op, query.key.as_deref()).await?))
}

/// `page`, `per_page`, `sort`, `search`, `filter[field][op]=value`.
fn list_query(pairs: Vec<(String, String)>) -> AppResult<ListQuery> {
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

async fn list(
    AdminCtx(ctx): AdminCtx,
    Path(slug): Path<String>,
    Query(pairs): Query<Vec<(String, String)>>,
) -> AppResult<Json<crate::Page>> {
    let query = list_query(pairs)?;
    Ok(Json(resource(&ctx, &slug)?.list(&ctx, &query).await?))
}

async fn find(
    AdminCtx(ctx): AdminCtx,
    Path((slug, key)): Path<(String, String)>,
) -> AppResult<Json<DataResponse<crate::Envelope>>> {
    let record = resource(&ctx, &slug)?.find(&ctx, &key).await?;
    Ok(Json(DataResponse { data: record }))
}

async fn create(
    AdminCtx(ctx): AdminCtx,
    Path(slug): Path<String>,
    Json(input): Json<Data>,
) -> AppResult<(StatusCode, Json<DataResponse<crate::Envelope>>)> {
    let record = resource(&ctx, &slug)?.create(&ctx, input).await?;
    Ok((StatusCode::CREATED, Json(DataResponse { data: record })))
}

async fn update(
    AdminCtx(ctx): AdminCtx,
    Path((slug, key)): Path<(String, String)>,
    Json(input): Json<Data>,
) -> AppResult<Json<DataResponse<crate::Envelope>>> {
    let record = resource(&ctx, &slug)?.update(&ctx, &key, input).await?;
    Ok(Json(DataResponse { data: record }))
}

async fn destroy(AdminCtx(ctx): AdminCtx, Path((slug, key)): Path<(String, String)>) -> AppResult<StatusCode> {
    resource(&ctx, &slug)?.delete(&ctx, &key).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct ActionInput {
    keys: Vec<String>,
    #[serde(default)]
    data: Data,
}

async fn run_action(
    AdminCtx(ctx): AdminCtx,
    Path((slug, action)): Path<(String, String)>,
    Json(input): Json<ActionInput>,
) -> AppResult<Json<ActionOutcome>> {
    Ok(Json(resource(&ctx, &slug)?.run_action(&ctx, &action, &input.keys, input.data).await?))
}

#[derive(Deserialize)]
struct OptionsQuery {
    search: Option<String>,
    keys: Option<String>,
    /// Key of the record being edited or viewed.
    record: Option<String>,
}

async fn options(
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
