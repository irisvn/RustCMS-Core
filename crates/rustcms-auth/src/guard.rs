//! The authentication middleware and extractors.
//!
//! One middleware serves both guards, chosen by path:
//!
//! - **admin** (`auth.admin_path`): session cookie; state-changing requests need
//!   an allowed `Origin` (or `Referer`) and the session's `X-CSRF-Token`.
//!   A session whose user is disabled or whose `auth_version` changed is ended.
//! - **api** (everything else): `Authorization: Bearer rcms_…`. A credential that
//!   is present but invalid is a 401: there is no fallback to another driver.

use std::sync::Arc;

use axum::extract::{FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, Method, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use rustcms_core::models::user;
use rustcms_core::{AppError, AppResult, AppState, Principal};
use sea_orm::EntityTrait;

use crate::config::origin_of;
use crate::session::{self, Session};
use crate::state::{ADMIN_GUARD, API_GUARD, AuthState};
use crate::tokens;

/// Header carrying the CSRF token of the admin session.
pub const CSRF_HEADER: &str = "x-csrf-token";

/// The authenticated user of a request.
#[derive(Debug, Clone)]
pub struct Authenticated {
    /// Permissions for the guard used.
    pub principal: Principal,
    /// User record (loaded this request).
    pub user: user::Model,
    /// API token used, if any.
    pub token_id: Option<i64>,
}

/// Requires an authenticated user (401 otherwise).
#[derive(Debug, Clone)]
pub struct Auth(pub Authenticated);

impl Auth {
    /// The principal, for permission checks.
    pub fn principal(&self) -> &Principal {
        &self.0.principal
    }

    /// 403 unless the principal has `permission`.
    pub fn authorize(&self, permission: &str) -> AppResult<()> {
        if self.0.principal.can(permission) { Ok(()) } else { Err(AppError::Forbidden) }
    }
}

impl<S: Send + Sync> FromRequestParts<S> for Auth {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, AppError> {
        parts.extensions.get::<Authenticated>().cloned().map(Auth).ok_or(AppError::Unauthenticated)
    }
}

/// The authenticated user, if any.
#[derive(Debug, Clone)]
pub struct MaybeAuth(pub Option<Authenticated>);

impl<S: Send + Sync> FromRequestParts<S> for MaybeAuth {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        Ok(Self(parts.extensions.get::<Authenticated>().cloned()))
    }
}

impl<S: Send + Sync> FromRequestParts<S> for Session {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, AppError> {
        parts
            .extensions
            .get::<Session>()
            .cloned()
            .ok_or_else(|| AppError::internal("no session outside the admin path"))
    }
}

pub(crate) async fn authenticate(State(app): State<AppState>, req: Request, next: Next) -> Response {
    let Some(auth) = app.service::<AuthState>() else {
        return AppError::internal("AuthProvider not registered").into_response();
    };
    if auth.is_admin_asset_path(req.uri().path()) {
        // Public, immutable files: no session is loaded or renewed, so no cookie
        // ever rides on a cacheable response.
        return next.run(req).await;
    }
    if auth.is_admin_path(req.uri().path()) {
        let private = auth.is_admin_api_path(req.uri().path());
        let mut response = admin(&app, &auth, req, next).await.unwrap_or_else(IntoResponse::into_response);
        // Session-bound data (records, schema, CSRF token, errors), and anything
        // setting the session cookie, is never cached or stored.
        if private || response.headers().contains_key(header::SET_COOKIE) {
            response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
        }
        response
    } else {
        api(&app, req, next).await.unwrap_or_else(IntoResponse::into_response)
    }
}

async fn admin(app: &AppState, auth: &Arc<AuthState>, mut req: Request, next: Next) -> AppResult<Response> {
    let session = session::load(auth, app.db(), req.headers()).await?;
    if !is_safe(req.method()) {
        check_origin(auth, req.headers())?;
        let header = req.headers().get(CSRF_HEADER).and_then(|v| v.to_str().ok()).unwrap_or_default();
        if !session.csrf_matches(header) {
            return Err(AppError::CsrfMismatch);
        }
    }
    let mut signed_in = None;
    if let Some(user_id) = session.user_id() {
        let user = user::Entity::find_by_id(user_id).one(app.db()).await?;
        match user.filter(|u| u.is_active) {
            Some(user) if user.auth_version == session.auth_version() => {
                signed_in = Some(user.id);
                let principal = auth.resolver.principal(app.db(), ADMIN_GUARD, &user, None).await?;
                req.extensions_mut().insert(Authenticated { principal, user, token_id: None });
            }
            // Credentials moved on: this request is anonymous. The session is not
            // ended here, because a concurrent request of the same browser may be
            // re-issuing it right now (password change, sign-out of other sessions);
            // ending the lineage would sign out the user who just proved their password.
            Some(_) => {}
            None => session.logout(),
        }
    }
    req.extensions_mut().insert(session.clone());
    let mut response = next.run(req).await;
    // Who this response was produced for: a page that was built for someone else
    // (the cookie changed accounts in another tab) must not show it.
    if let Some(id) = signed_in {
        response.headers_mut().insert(USER_HEADER, HeaderValue::from(id));
    }
    if let Some(cookie) = session::persist(auth, app.db(), &session).await? {
        response.headers_mut().append(header::SET_COOKIE, cookie);
    }
    Ok(response)
}

async fn api(app: &AppState, mut req: Request, next: Next) -> AppResult<Response> {
    if let Some(value) = req.headers().get(header::AUTHORIZATION) {
        let value = value.to_str().map_err(|_| AppError::Unauthenticated)?;
        let bearer = value.strip_prefix("Bearer ").ok_or(AppError::Unauthenticated)?.trim();
        if !bearer.starts_with(tokens::PREFIX) {
            return Err(AppError::Unauthenticated);
        }
        let (token, user) = tokens::authenticate(app.db(), bearer).await?;
        let auth = app.service::<AuthState>().ok_or_else(|| AppError::internal("AuthProvider not registered"))?;
        let principal = auth.resolver.principal(app.db(), API_GUARD, &user, Some(tokens::abilities(&token))).await?;
        req.extensions_mut().insert(Authenticated { principal, user, token_id: Some(token.id) });
    }
    Ok(next.run(req).await)
}

/// Response header naming the signed-in admin user (`id`), on every admin response.
pub const USER_HEADER: &str = "x-rustcms-user";

fn is_safe(method: &Method) -> bool {
    matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

/// `Origin` must be an allowed origin; without it, `Referer` must point to one.
fn check_origin(auth: &AuthState, headers: &HeaderMap) -> AppResult<()> {
    let source = headers
        .get(header::ORIGIN)
        .or_else(|| headers.get(header::REFERER))
        .and_then(|v| v.to_str().ok())
        .and_then(origin_of)
        .ok_or(AppError::Forbidden)?;
    if auth.allowed_origins.contains(&source) { Ok(()) } else { Err(AppError::Forbidden) }
}
