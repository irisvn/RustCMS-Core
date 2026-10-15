//! Admin API routes (`<admin_path>/api/…`), behind the admin session guard.

use std::sync::Arc;

use axum::Router;

use axum::http::Method;
use axum::routing::{delete, get, patch, post};
use rustcms_core::{AppError, AppResult, AppState};
use serde::Serialize;

use crate::context::Ctx;
use crate::registry::{DynResource, Registry};

/// The admin panel: brand and resources. Bound by [`crate::AdminProvider`].
pub struct AdminPanel {
    /// Name shown in the panel.
    pub brand: String,
    /// Registered resources.
    pub registry: Arc<Registry>,
    /// Translations of labels (framework and application catalogs).
    pub translator: Arc<rustcms_core::i18n::Translator>,
    /// Settings pages.
    pub settings: Arc<crate::settings::SettingsRegistry>,
    /// Custom pages and slots.
    pub custom: Arc<crate::custom::CustomPanel>,
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
    /// Settings pages the user may open.
    pub settings: Vec<crate::settings::SettingsLink>,
    /// Custom pages the user may open, in registration order.
    pub pages: Vec<crate::custom::PageLink>,
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
        (Method::POST, format!("{resource}/records/{{key}}/restore")),
        (Method::DELETE, format!("{resource}/records/{{key}}/force")),
        (Method::GET, format!("{resource}/records/{{key}}/relations/{{relation}}")),
        (Method::POST, format!("{resource}/records/{{key}}/relations/{{relation}}")),
        (Method::POST, format!("{resource}/records/{{key}}/relations/{{relation}}/attach")),
        (Method::POST, format!("{resource}/records/{{key}}/relations/{{relation}}/detach")),
        (Method::PATCH, format!("{resource}/records/{{key}}/relations/{{relation}}/links/{{target}}")),
        (Method::GET, format!("{resource}/records/{{key}}/relations/{{relation}}/options")),
        (Method::POST, format!("{resource}/actions/{{action}}")),
        (Method::GET, format!("{resource}/actions/{{action}}/fields/{{field}}/options")),
        (Method::GET, format!("{resource}/fields/{{field}}/options")),
        (Method::POST, format!("{base}/media")),
        (Method::GET, format!("{base}/media/{{id}}/file")),
    ]
    .into_iter()
    .chain(crate::system::routes(admin_path))
    .chain(crate::settings::routes(admin_path))
    .chain(crate::audit::routes(admin_path))
    .chain(crate::queue::routes(admin_path))
    .chain(crate::custom::routes(admin_path))
    .chain(crate::exports::routes(admin_path))
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
        .route(&format!("{resource}/records/{{key}}/restore"), post(restore))
        .route(&format!("{resource}/records/{{key}}/force"), delete(force_delete))
        .route(&format!("{resource}/records/{{key}}/relations/{{relation}}"), get(relation_list).post(relation_create))
        .route(&format!("{resource}/records/{{key}}/relations/{{relation}}/attach"), post(relation_attach))
        .route(&format!("{resource}/records/{{key}}/relations/{{relation}}/detach"), post(relation_detach))
        .route(
            &format!("{resource}/records/{{key}}/relations/{{relation}}/links/{{target}}"),
            patch(relation_update_pivot),
        )
        .route(&format!("{resource}/records/{{key}}/relations/{{relation}}/options"), get(relation_options))
        .route(&format!("{resource}/actions/{{action}}"), post(run_action))
        .route(&format!("{resource}/actions/{{action}}/fields/{{field}}/options"), get(action_options))
        .route(&format!("{resource}/fields/{{field}}/options"), get(options))
        .route(&format!("{base}/media"), post(crate::media::upload))
        .route(&format!("{base}/media/{{id}}/file"), get(crate::media::file))
}

mod actions;
mod extract;
mod relations;
mod resources;

use actions::*;
pub use extract::AdminCtx;
pub use relations::LinkPage;
use relations::*;
use resources::*;
pub(crate) use resources::{idempotency_key, list_query};

fn resource(ctx: &Ctx, slug: &str) -> AppResult<Arc<dyn DynResource>> {
    ctx.registry.get(slug).cloned().ok_or(AppError::NotFound)
}
