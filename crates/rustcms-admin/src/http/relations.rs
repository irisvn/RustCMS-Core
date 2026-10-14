//! Handlers of has-many relations (list and create through a parent).

use super::AdminCtx;
use std::sync::Arc;

use axum::extract::{Path, Query};
use axum::http::StatusCode;
use rustcms_core::{AppError, AppResult, Json};

use crate::Data;
use crate::context::Ctx;
use crate::policy::ability;
use crate::registry::DynResource;

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
) -> AppResult<Json<crate::Page>> {
    let (parent, relation, target) = relation_of(&ctx, &slug, &name)?;
    // Everything that does not depend on the parent first (no probing of keys).
    if !parent.admits(&ctx, ability::VIEW).await || !target.allows(&ctx, ability::VIEW_ANY).await {
        return Err(AppError::Forbidden);
    }
    let mut query = list_query(pairs)?;
    query.parent = Some((relation.field.to_owned(), String::new()));
    target.admit_list(&ctx, &query).await?;
    let parent_key = parent.visible_key(&ctx, &key).await?;
    query.parent = Some((relation.field.to_owned(), parent_key));
    Ok(Json(target.list(&ctx, &query).await?))
}

pub(super) async fn relation_create(
    AdminCtx(ctx): AdminCtx,
    Path((slug, key, name)): Path<(String, String, String)>,
    headers: axum::http::HeaderMap,
    Json(input): Json<Data>,
) -> AppResult<(StatusCode, Json<DataResponse<crate::Envelope>>)> {
    let (parent, relation, target) = relation_of(&ctx, &slug, &name)?;
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
