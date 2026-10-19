//! `GET pages/{slug}` and `GET slots?resource=&key=`.

use axum::Router;
use axum::extract::{Path, RawQuery, State};
use axum::http::Method;
use axum::routing::get;
use rustcms_core::{AppError, AppResult, AppState, Json, ValidationErrors};
use serde::Serialize;
use serde_json::Value;

use super::{CustomPanel, MAX_DATA_BYTES, PageView, Placement};
use crate::context::Ctx;
use crate::http::{AdminCtx, AdminPanel};
use crate::policy::ability;

/// Every custom-page route as `(method, path)`.
pub(crate) fn routes(admin_path: &str) -> Vec<(Method, String)> {
    let base = format!("{}/api", admin_path.trim_end_matches('/'));
    vec![
        (Method::GET, format!("{base}/pages/{{slug}}")),
        (Method::GET, format!("{base}/pages/{{slug}}/{{*path}}")),
        (Method::GET, format!("{base}/slots")),
    ]
}

pub(crate) fn router(router: Router<AppState>, admin_path: &str) -> Router<AppState> {
    let base = format!("{}/api", admin_path.trim_end_matches('/'));
    router
        .route(&format!("{base}/pages/{{slug}}"), get(page))
        .route(&format!("{base}/pages/{{slug}}/{{*path}}"), get(sub_page))
        .route(&format!("{base}/slots"), get(slots))
}

/// A custom page in the navigation.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PageLink {
    /// URL segment (`/p/{slug}`).
    pub slug: String,
    /// Title.
    pub label: String,
    /// Icon name.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Navigation group.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// Navigation position (`PanelPageMeta::sort`), absent when 0.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort: Option<i32>,
}

/// What a page or slot shows: sanitized HTML, or a script and its data.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CustomContent {
    /// HTML from a server template, cleaned for the panel.
    Html {
        /// The markup.
        html: String,
    },
    /// Drawn by the extension script registered under `script`.
    Script {
        /// `vendor/name`.
        script: String,
        /// What the script gets as `ctx.data`.
        #[cfg_attr(feature = "ts", ts(type = "JsonValue"))]
        data: Value,
    },
}

/// `GET pages/{slug}`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CustomPageResponse {
    /// URL segment.
    pub slug: String,
    /// Title (translated).
    pub title: String,
    /// Content.
    pub content: CustomContent,
}

/// One slot beside a record.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SlotContent {
    /// Slot name.
    pub name: String,
    /// Heading (translated).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Content.
    pub content: CustomContent,
}

/// `GET slots`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SlotsResponse {
    /// The slots the user may see, in registration order.
    pub data: Vec<SlotContent>,
}

fn custom(state: &AppState) -> AppResult<std::sync::Arc<CustomPanel>> {
    Ok(state.service::<AdminPanel>().ok_or_else(|| AppError::internal("AdminProvider not registered"))?.custom.clone())
}

fn allowed(ctx: &Ctx, permissions: &[String]) -> bool {
    permissions.iter().all(|p| ctx.can(p))
}

/// Draws `view` with `data`: a template is rendered and cleaned; a script
/// gets the data (bounded).
fn draw(custom: &CustomPanel, ctx: &Ctx, name: &str, view: PageView, data: Value) -> AppResult<CustomContent> {
    let size = serde_json::to_vec(&data).map_or(usize::MAX, |b| b.len());
    if size > MAX_DATA_BYTES {
        return Err(AppError::internal(format!("{name}: data larger than {MAX_DATA_BYTES} bytes")));
    }
    Ok(match view {
        PageView::Template(_) | PageView::File(_) => {
            let html = custom.templates.render(name, ctx, &custom.base, &data)?;
            CustomContent::Html { html: custom.sanitizer.clean(&html) }
        }
        PageView::Script(script) => CustomContent::Script { script: script.to_owned(), data },
    })
}

/// The home page's slug, when `ctx` may open it.
pub(crate) fn home(custom: &CustomPanel, ctx: &Ctx) -> Option<String> {
    let slug = custom.home.as_deref()?;
    let page = custom.pages.iter().find(|p| p.slug() == slug)?;
    page.meta().allows(ctx).then(|| slug.to_owned())
}

/// Pages `ctx` may open, for the navigation.
pub(crate) fn links(custom: &CustomPanel, ctx: &Ctx) -> Vec<PageLink> {
    custom
        .pages
        .iter()
        .filter(|p| p.meta().allows(ctx))
        .map(|p| PageLink {
            slug: p.slug().to_owned(),
            label: ctx.t(&p.meta().label),
            icon: p.meta().icon.clone(),
            group: p.meta().group.as_deref().map(|g| ctx.t(g)),
            sort: (p.meta().sort != 0).then_some(p.meta().sort),
        })
        .collect()
}

/// `GET pages/{slug}`: 403 for a user who may open no page (whatever the
/// slug), 404 for an unknown slug otherwise, 403 without the page's
/// permissions; `data` runs only when allowed.
async fn page(
    State(state): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Path(slug): Path<String>,
) -> AppResult<Json<CustomPageResponse>> {
    serve_page(&state, ctx, slug, None).await
}

/// `GET pages/{slug}/{*path}`: the same, for a sub-page. The sub-path is read
/// from the raw request path (never axum's decoded one) and checked after the
/// page's permissions (400 when it is not one).
async fn sub_page(
    State(state): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    // Raw, without axum's path extractors: they decode (and refuse invalid
    // UTF-8) before the permissions are checked.
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
) -> AppResult<Json<CustomPageResponse>> {
    let auth = state.service::<rustcms_auth::AuthState>().ok_or_else(|| AppError::internal("AuthProvider missing"))?;
    let prefix = format!("{}/api/pages/", auth.config.admin_path.trim_end_matches('/'));
    let rest = uri.path().strip_prefix(&prefix).ok_or(AppError::NotFound)?;
    let (slug, path) = rest.split_once('/').unwrap_or((rest, ""));
    serve_page(&state, ctx, slug.to_owned(), Some(path.to_owned())).await
}

async fn serve_page(
    state: &AppState,
    mut ctx: Ctx,
    slug: String,
    raw_path: Option<String>,
) -> AppResult<Json<CustomPageResponse>> {
    let custom = custom(state)?;
    let any = custom.pages.iter().any(|p| p.meta().allows(&ctx));
    let page = match custom.pages.iter().find(|p| p.slug() == slug) {
        Some(page) if page.meta().allows(&ctx) => page.clone(),
        Some(_) => return Err(AppError::Forbidden),
        None if any => return Err(AppError::NotFound),
        None => return Err(AppError::Forbidden),
    };
    if let Some(raw) = raw_path {
        ctx.page_path = super::subpath::canonical(&raw).ok_or_else(|| ValidationErrors::field("path", "is invalid"))?;
    }
    let data = page.data(&ctx).await?;
    let content = draw(&custom, &ctx, &format!("page:{slug}"), page.view(), data)?;
    Ok(Json(CustomPageResponse { slug, title: ctx.t(&page.meta().label), content }))
}

/// `GET slots?resource=&key=`: the slots beside a record the user may view
/// (as its page: 404 missing or out of scope, 403 denied); slots without
/// their permissions are left out before their data is read.
async fn slots(
    State(state): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    RawQuery(query): RawQuery,
) -> AppResult<Json<SlotsResponse>> {
    let mut resource = None;
    let mut key = None;
    let mut errors = ValidationErrors::new();
    for (name, value) in form_urlencoded::parse(query.unwrap_or_default().as_bytes()) {
        let slot = match name.as_ref() {
            "resource" => &mut resource,
            "key" => &mut key,
            other => {
                errors.add(other, "is not allowed");
                continue;
            }
        };
        if slot.replace(value.into_owned()).is_some() {
            errors.add(name.as_ref(), "may only be given once");
        }
    }
    // The resource admits the user before any parameter error (no probing).
    let target = match &resource {
        Some(slug) => {
            let target = ctx.registry.get(slug).cloned().ok_or(AppError::NotFound)?;
            if !target.admits(&ctx, ability::VIEW).await {
                return Err(AppError::Forbidden);
            }
            Some(target)
        }
        None => {
            errors.add("resource", "is required");
            None
        }
    };
    if key.is_none() {
        errors.add("key", "is required");
    }
    errors.into_result()?;
    let (Some(resource), Some(key), Some(target)) = (resource, key, target) else {
        return Err(AppError::internal("checked above"));
    };
    // As the record's page: in scope and allowed, readable fields only.
    let record = target.find(&ctx, &key).await?;
    let custom = custom(&state)?;
    let mut data = Vec::new();
    for slot in &custom.slots {
        let Placement::RecordAside(beside) = slot.placement();
        if beside != resource || !allowed(&ctx, &slot.meta().permissions) {
            continue;
        }
        let values = slot.data(&ctx, &record).await?;
        let content = draw(&custom, &ctx, &format!("slot:{}", slot.name()), slot.view(), values)?;
        data.push(SlotContent {
            name: slot.name().to_owned(),
            label: slot.meta().label.as_deref().map(|l| ctx.t(l)),
            content,
        });
    }
    Ok(Json(SlotsResponse { data }))
}
