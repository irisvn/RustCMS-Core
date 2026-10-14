//! Handlers of `/api/v1/posts` (≈ a controller). Access is checked by the
//! routes before these run.

use rustcms::prelude::*;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, Set};

use super::json::PostJson;
use super::requests::NewPost;
use crate::models::post;

/// Drafts a user may create per minute through the API.
const CREATES_PER_MINUTE: u32 = 30;

/// Published posts (publication date reached, not trashed), newest first.
pub async fn list(State(app): State<AppState>, page: Pagination) -> AppResult<Json<ApiPage<PostJson>>> {
    let select = post::Entity::find()
        // Trashed posts (soft deletes) are not published.
        .filter(post::Column::DeletedAt.is_null())
        .filter(post::Column::Published.eq(true))
        .filter(post::Column::PublishedAt.is_not_null())
        .filter(post::Column::PublishedAt.lte(chrono::Utc::now()))
        .order_by_desc(post::Column::PublishedAt)
        .order_by_desc(post::Column::Id);
    Ok(Json(ApiPage::fetch(select, app.db(), &page, PostJson::from).await?))
}

/// Creates an unpublished draft owned by the token's user.
pub async fn create(
    State(app): State<AppState>,
    auth: Auth,
    _: Throttle<CREATES_PER_MINUTE>,
    Validated(input): Validated<NewPost>,
) -> AppResult<(StatusCode, Json<PostJson>)> {
    let now = chrono::Utc::now();
    let post = post::ActiveModel {
        title: Set(input.title),
        body: Set(input.body),
        author_id: Set(Some(auth.0.user.id)),
        published: Set(false),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(app.db())
    .await?;
    Ok((StatusCode::CREATED, Json(post.into())))
}
