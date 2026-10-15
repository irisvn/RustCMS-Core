//! Handlers of relations: has-many (list and create through a parent) and
//! belongs-to-many (list, attach, detach, pivot updates, options).

use super::AdminCtx;
use std::sync::Arc;

use std::collections::BTreeMap;

use axum::extract::{Path, Query, Request, State};
use axum::http::StatusCode;
use rustcms_core::{AppError, AppResult, AppState, Json, ValidationErrors};
use serde::{Deserialize, Serialize};

use crate::Data;
use crate::context::Ctx;
use crate::links::{LinkOp, LinkOption, LinkOutcome, LinkRequest};
use crate::policy::ability;
use crate::registry::DynResource;

use super::extract::json_body;
use super::resources::{idempotency_key, list_query};
use super::{DataResponse, resource};

/// The parent, the relation and its target resource.
pub(super) type RelationParts = (Arc<dyn DynResource>, crate::resource::Relation, Arc<dyn DynResource>);

pub(super) fn relation_of(ctx: &Ctx, slug: &str, name: &str) -> AppResult<RelationParts> {
    let parent = resource(ctx, slug)?;
    let relation = parent.relations().iter().find(|r| r.name == name).cloned().ok_or(AppError::NotFound)?;
    let target = resource(ctx, relation.target)?;
    Ok((parent, relation, target))
}

pub(super) async fn relation_list(
    AdminCtx(ctx): AdminCtx,
    Path((slug, key, name)): Path<(String, String, String)>,
    Query(pairs): Query<Vec<(String, String)>>,
) -> AppResult<Json<LinkPage>> {
    let (parent, relation, target) = relation_of(&ctx, &slug, &name)?;
    // Everything that does not depend on the parent first (no probing of keys).
    if !parent.admits(&ctx, ability::VIEW).await || !target.allows(&ctx, ability::VIEW_ANY).await {
        return Err(AppError::Forbidden);
    }
    let mut query = list_query(pairs)?;
    if relation.is_many() {
        target.admit_list(&ctx, &query).await?;
        let (parent_key, through) = parent.link_targets(&ctx, &name, &key).await?;
        query.through = Some(through);
        let page = target.list(&ctx, &query).await?;
        let keys: Vec<String> = page.data.iter().map(|e| e.key.clone()).collect();
        let pivot = parent.link_values(&ctx, &name, &parent_key, &keys).await?;
        return Ok(Json(LinkPage { page, pivot }));
    }
    query.parent = Some((relation.field.to_owned(), String::new()));
    target.admit_list(&ctx, &query).await?;
    let parent_key = parent.visible_key(&ctx, &key).await?;
    query.parent = Some((relation.field.to_owned(), parent_key));
    Ok(Json(LinkPage { page: target.list(&ctx, &query).await?, pivot: BTreeMap::new() }))
}

/// A relation's records; belongs-to-many lists add the readable pivot
/// columns of each link, by target key.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LinkPage {
    /// The records.
    #[serde(flatten)]
    pub page: crate::Page,
    /// Pivot columns by target key (belongs-to-many).
    #[cfg_attr(feature = "ts", ts(as = "Option<BTreeMap<String, Data>>", optional))]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub pivot: BTreeMap<String, Data>,
}

/// The parent and relation of a link write, admitted before the body is read.
async fn admitted_link(ctx: &Ctx, slug: &str, name: &str, op: LinkOp) -> AppResult<Arc<dyn DynResource>> {
    let parent = resource(ctx, slug)?;
    parent.admit_link(ctx, name, op).await?;
    Ok(parent)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LinkKeys {
    keys: Vec<String>,
    #[serde(default)]
    pivot: Data,
}

/// `POST …/relations/{relation}/attach` `{keys, pivot?}`.
pub(super) async fn relation_attach(
    State(state): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Path((slug, key, name)): Path<(String, String, String)>,
    request: Request,
) -> AppResult<Json<LinkOutcome>> {
    let parent = admitted_link(&ctx, &slug, &name, LinkOp::Attach).await?;
    let body: LinkKeys = json_body(request, &state).await?;
    let request = LinkRequest::Attach { keys: body.keys, values: body.pivot };
    Ok(Json(parent.link(&ctx, &name, &key, request).await?))
}

/// `POST …/relations/{relation}/detach` `{keys}`.
pub(super) async fn relation_detach(
    State(state): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Path((slug, key, name)): Path<(String, String, String)>,
    request: Request,
) -> AppResult<Json<LinkOutcome>> {
    let parent = admitted_link(&ctx, &slug, &name, LinkOp::Detach).await?;
    let body: LinkKeys = json_body(request, &state).await?;
    if !body.pivot.is_empty() {
        return Err(ValidationErrors::field("pivot", "is not allowed").into());
    }
    Ok(Json(parent.link(&ctx, &name, &key, LinkRequest::Detach { keys: body.keys }).await?))
}

/// `PATCH …/relations/{relation}/links/{target}` `{…pivot columns}`.
pub(super) async fn relation_update_pivot(
    State(state): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Path((slug, key, name, target)): Path<(String, String, String, String)>,
    request: Request,
) -> AppResult<Json<LinkOutcome>> {
    let parent = admitted_link(&ctx, &slug, &name, LinkOp::UpdatePivot).await?;
    let values: Data = json_body(request, &state).await?;
    Ok(Json(parent.link(&ctx, &name, &key, LinkRequest::UpdatePivot { key: target, values }).await?))
}

#[derive(Deserialize)]
pub(super) struct LinkOptionsQuery {
    search: Option<String>,
    keys: Option<String>,
}

/// `GET …/relations/{relation}/options?search=&keys=`: targets to attach.
pub(super) async fn relation_options(
    AdminCtx(ctx): AdminCtx,
    Path((slug, key, name)): Path<(String, String, String)>,
    Query(query): Query<LinkOptionsQuery>,
) -> AppResult<Json<DataResponse<Vec<LinkOption>>>> {
    let parent = admitted_link(&ctx, &slug, &name, LinkOp::Attach).await?;
    let keys: Vec<String> = query
        .keys
        .as_deref()
        .map(|k| k.split(',').filter(|k| !k.is_empty()).map(str::to_owned).collect())
        .unwrap_or_default();
    let options = parent.link_options(&ctx, &name, &key, query.search.as_deref(), &keys).await?;
    Ok(Json(DataResponse { data: options }))
}

pub(super) async fn relation_create(
    AdminCtx(ctx): AdminCtx,
    Path((slug, key, name)): Path<(String, String, String)>,
    headers: axum::http::HeaderMap,
    Json(input): Json<Data>,
) -> AppResult<(StatusCode, Json<DataResponse<crate::Envelope>>)> {
    let (parent, relation, target) = relation_of(&ctx, &slug, &name)?;
    // Belongs-to-many targets are attached, not created through the record.
    if relation.is_many() {
        return Err(AppError::MethodNotAllowed);
    }
    // The foreign key is written by the user's create: they must be allowed to set it.
    let (_, creatable) = target.field_access(&ctx, relation.field);
    if !creatable || !parent.admits(&ctx, ability::VIEW).await || !target.allows(&ctx, ability::CREATE).await {
        return Err(AppError::Forbidden);
    }
    let canonical = parent.canonical_key(&key).ok_or(AppError::NotFound)?;
    let ctx = ctx.with_idempotency_key(idempotency_key(&headers)?).with_bound((
        format!("{slug}.{name}"),
        relation.field.to_owned(),
        canonical,
    ));
    // A committed retry is answered even if the parent changed since.
    if let Some(envelope) = target.replay_create(&ctx, &input).await? {
        return Ok((StatusCode::CREATED, Json(DataResponse { data: envelope })));
    }
    parent.visible_key(&ctx, &key).await?;
    let record = target.create(&ctx, input).await?;
    Ok((StatusCode::CREATED, Json(DataResponse { data: record })))
}
