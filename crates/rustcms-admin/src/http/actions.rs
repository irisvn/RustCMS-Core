//! Handler of resource actions.

use super::{AdminCtx, idempotency_key};

use axum::extract::Path;
use rustcms_core::{AppResult, Json};
use serde::Deserialize;

use crate::{ActionOutcome, Data};

use super::resource;

#[derive(Deserialize)]
pub(super) struct ActionInput {
    keys: Vec<String>,
    #[serde(default)]
    data: Data,
}

pub(super) async fn run_action(
    AdminCtx(ctx): AdminCtx,
    Path((slug, action)): Path<(String, String)>,
    headers: axum::http::HeaderMap,
    Json(input): Json<ActionInput>,
) -> AppResult<Json<ActionOutcome>> {
    let ctx = ctx.with_idempotency_key(idempotency_key(&headers)?);
    Ok(Json(resource(&ctx, &slug)?.run_action(&ctx, &action, &input.keys, input.data).await?))
}
