//! Handlers of resource actions.

use super::{AdminCtx, idempotency_key};

use axum::extract::{Path, Query, Request, State};
use rustcms_core::{AppResult, AppState, Json};
use serde::Deserialize;

use crate::{ActionOutcome, Data};

use super::extract::json_body;
use super::{DataResponse, resource};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ActionInput {
    keys: Vec<String>,
    #[serde(default)]
    data: Data,
}

/// `POST {slug}/actions/{action}` `{keys, data?}`: admitted before the body is read.
pub(super) async fn run_action(
    State(state): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Path((slug, action)): Path<(String, String)>,
    request: Request,
) -> AppResult<Json<ActionOutcome>> {
    let resource = resource(&ctx, &slug)?;
    resource.admit_action(&ctx, &action).await?;
    let ctx = ctx.with_idempotency_key(idempotency_key(request.headers())?);
    let input: ActionInput = json_body(request, &state).await?;
    Ok(Json(resource.run_action(&ctx, &action, &input.keys, input.data).await?))
}

#[derive(Deserialize)]
pub(super) struct ActionOptionsQuery {
    search: Option<String>,
    keys: Option<String>,
}

/// `GET {slug}/actions/{action}/fields/{field}/options`: records a `belongs_to`
/// field of the action's form may name.
pub(super) async fn action_options(
    AdminCtx(ctx): AdminCtx,
    Path((slug, action, field)): Path<(String, String, String)>,
    Query(query): Query<ActionOptionsQuery>,
) -> AppResult<Json<DataResponse<Vec<crate::ui::SelectOption>>>> {
    let resource = resource(&ctx, &slug)?;
    resource.admit_action(&ctx, &action).await?;
    let keys: Vec<String> = query
        .keys
        .as_deref()
        .map(|k| k.split(',').filter(|k| !k.is_empty()).map(str::to_owned).collect())
        .unwrap_or_default();
    let options = resource.action_options(&ctx, &action, &field, query.search.as_deref(), &keys).await?;
    Ok(Json(DataResponse { data: options }))
}
