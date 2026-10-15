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
        let audit = rustcms_core::audit::AuditContext::new(
            rustcms_core::audit::AuditActor::user(&auth.principal),
            rustcms_core::audit::RequestMeta::from_parts(parts),
        );
        let ctx = Ctx::new(state.db().clone(), auth.principal, panel.registry.clone());
        Ok(Self(ctx.with_locale(locale, panel.translator.clone()).with_audit(audit)))
    }
}

/// The JSON body of a request whose access was checked first (a caller
/// without access sends nothing the server reads): 413 when too large, 422
/// when it is not the expected JSON.
pub(crate) async fn json_body<T: serde::de::DeserializeOwned>(
    request: axum::extract::Request,
    state: &AppState,
) -> Result<T, AppError> {
    use axum::extract::FromRequest;
    let body = axum::body::Bytes::from_request(request, state).await.map_err(|e| {
        if e.status() == axum::http::StatusCode::PAYLOAD_TOO_LARGE {
            AppError::PayloadTooLarge
        } else {
            AppError::BadRequest("the body could not be read".into())
        }
    })?;
    serde_json::from_slice(&body).map_err(|_| {
        let mut errors = rustcms_core::ValidationErrors::new();
        errors.add_form("is invalid");
        errors.into()
    })
}
