//! Public JSON API under `/api/v1`, authenticated with API tokens
//! (`Authorization: Bearer rcms_…`, see `rustcms token:create`).

use rustcms::auth::{API_GUARD, Auth, AuthState};
use rustcms::axum::body::Bytes;
use rustcms::axum::extract::RawQuery;
use rustcms::prelude::*;
use rustcms::{AppError, ValidationErrors};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, Set};
use serde::{Deserialize, Serialize};

use crate::models::post;

/// Permissions of the API guard used here (declared for `permission:sync`).
pub const PERMISSIONS: &[&str] = &["posts.view_any", "posts.create"];
const PER_PAGE: u64 = 20;
/// Drafts a user may create per minute through the API.
const CREATES_PER_MINUTE: u32 = 30;

/// Registers the routes and describes them for `route:list` (one source of truth).
pub fn register(app: &mut AppBuilder) {
    app.routes(|router| router.route("/api/v1/posts", get(list).post(create)));
    app.describe_routes("app", [(Method::GET, "/api/v1/posts".to_owned()), (Method::POST, "/api/v1/posts".to_owned())]);
}

#[derive(Serialize)]
struct PostJson {
    id: String,
    title: String,
    body: Option<String>,
    published: bool,
    published_at: Option<String>,
}

impl From<post::Model> for PostJson {
    fn from(p: post::Model) -> Self {
        Self {
            id: p.id.to_string(),
            title: p.title,
            body: p.body,
            published: p.published,
            published_at: p.published_at.map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
        }
    }
}

/// `?page=N` (1-based), or 422.
fn page_of(query: Option<&str>) -> AppResult<u64> {
    let mut page = 1;
    for (key, value) in url_pairs(query.unwrap_or_default()) {
        if key == "page" {
            page =
                value.parse::<u64>().ok().filter(|p| (1..=10_000).contains(p)).ok_or_else(|| {
                    AppError::from(ValidationErrors::field("page", "must be a number from 1 to 10000"))
                })?;
        }
    }
    Ok(page)
}

fn url_pairs(query: &str) -> impl Iterator<Item = (&str, &str)> {
    query.split('&').filter(|p| !p.is_empty()).map(|p| p.split_once('=').unwrap_or((p, "")))
}

/// Published posts (publication date reached), newest first (`posts.view_any`).
async fn list(
    State(app): State<AppState>,
    auth: Auth,
    RawQuery(query): RawQuery,
) -> AppResult<Json<serde_json::Value>> {
    // Authorize before reading any input: the answer never depends on it.
    auth.authorize_guard(API_GUARD, "posts.view_any")?;
    let page = page_of(query.as_deref())?;
    let paginator = post::Entity::find()
        .filter(post::Column::Published.eq(true))
        .filter(post::Column::PublishedAt.is_not_null())
        .filter(post::Column::PublishedAt.lte(chrono::Utc::now()))
        .order_by_desc(post::Column::PublishedAt)
        .order_by_desc(post::Column::Id)
        .paginate(app.db(), PER_PAGE);
    let totals = paginator.num_items_and_pages().await?;
    let posts = paginator.fetch_page(page - 1).await?;
    Ok(Json(json!({
        "data": posts.into_iter().map(PostJson::from).collect::<Vec<_>>(),
        "meta": { "page": page, "perPage": PER_PAGE, "total": totals.number_of_items, "lastPage": totals.number_of_pages.max(1) },
    })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewPost {
    title: String,
    #[serde(default)]
    body: Option<String>,
}

/// Creates an unpublished draft owned by the token's user (`posts.create`).
async fn create(State(app): State<AppState>, auth: Auth, body: Bytes) -> AppResult<(StatusCode, Json<PostJson>)> {
    auth.authorize_guard(API_GUARD, "posts.create")?;
    let state = app.service::<AuthState>().ok_or_else(|| AppError::internal("AuthProvider not registered"))?;
    state.limiter.reserve(&[(&format!("api-posts:{}", auth.0.user.id), CREATES_PER_MINUTE)])?;
    let input: NewPost = serde_json::from_slice(&body)
        .map_err(|e| AppError::from(ValidationErrors::field("body", format!("invalid JSON: {e}"))))?;
    let title = input.title.trim();
    let body = input.body.map(|b| b.trim().to_owned()).filter(|b| !b.is_empty());
    let mut errors = ValidationErrors::new();
    if title.is_empty() || title.chars().count() > 200 || title.chars().any(char::is_control) {
        errors.add("title", "must be 1 to 200 characters, without control characters");
    }
    if body.as_ref().is_some_and(|b| b.chars().count() > 100_000) {
        errors.add("body", "may not be longer than 100000 characters");
    }
    errors.into_result()?;
    let now = chrono::Utc::now();
    let post = post::ActiveModel {
        title: Set(title.to_owned()),
        body: Set(body),
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
