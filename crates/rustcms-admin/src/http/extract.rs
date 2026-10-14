//! `AdminCtx`: the authenticated admin user of a request.

use super::AdminPanel;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use rustcms_auth::{ADMIN_GUARD, Auth};
use rustcms_core::{AppError, AppState};

use crate::context::Ctx;

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
        let locale =
            rustcms_auth::routes::display_locale(state, &parts.headers, auth.principal.locale.as_deref()).to_owned();
        let ctx = Ctx::new(state.db().clone(), auth.principal, panel.registry.clone());
        Ok(Self(ctx.with_locale(locale, panel.translator.clone())))
    }
}
