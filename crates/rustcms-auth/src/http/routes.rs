//! Auth endpoints of both guards.

use std::net::IpAddr;
use std::sync::Arc;

use axum::Extension;
use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, delete, get, post};
use rustcms_core::audit::{AuditActor, AuditContext, AuditEntry, RequestMeta};
use rustcms_core::models::user;
use rustcms_core::{AppError, AppResult, AppState, ClientIp, Json, Principal, ValidationErrors};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::credentials;
use crate::guard::{Auth, MaybeAuth};
use crate::guards::jwt_session;
use crate::session::Session;
use crate::state::{ADMIN_GUARD, API_GUARD, AuthState};
use crate::tokens;

const BAD_CREDENTIALS: &str = "These credentials do not match our records.";
const MAX_ABILITIES: usize = 100;
/// Abilities of a JWT sign-in, in bytes: they are copied into every access
/// token, which must stay under `jwt::MAX_LEN` (4 KiB).
const JWT_ABILITY_BYTES: usize = 2048;
/// Longest accepted email; longer input is rejected before any throttling key or hash is built.
pub const MAX_EMAIL_LEN: usize = 255;
/// Longest accepted password; bounds the hashing work per request.
pub const MAX_PASSWORD_LEN: usize = 1024;

/// Every auth route as `(method, path)`, for route listings and the authorization oracle.
pub fn routes(admin_path: &str) -> Vec<(Method, String)> {
    let admin = format!("{}/api/auth", admin_path.trim_end_matches('/'));
    vec![
        (Method::GET, format!("{admin}/csrf")),
        (Method::POST, format!("{admin}/login")),
        (Method::POST, format!("{admin}/logout")),
        (Method::GET, format!("{admin}/me")),
        (Method::POST, format!("{admin}/password")),
        (Method::POST, format!("{admin}/logout-others")),
        (Method::POST, format!("{admin}/locale")),
        (Method::POST, "/api/v1/auth/token".into()),
        (Method::DELETE, "/api/v1/auth/token".into()),
        (Method::GET, "/api/v1/auth/me".into()),
        (Method::POST, "/api/v1/auth/jwt".into()),
        (Method::POST, "/api/v1/auth/refresh".into()),
        (Method::POST, "/api/v1/auth/jwt/logout".into()),
    ]
}

pub(crate) fn router(router: Router<AppState>, admin_path: &str) -> Router<AppState> {
    let base = format!("{}/api", admin_path.trim_end_matches('/'));
    let admin = format!("{base}/auth");
    router
        .route(&format!("{admin}/csrf"), get(csrf))
        .route(&format!("{admin}/login"), post(login))
        .route(&format!("{admin}/logout"), post(logout))
        .route(&format!("{admin}/me"), get(me))
        .route(&format!("{admin}/password"), post(change_password))
        .route(&format!("{admin}/logout-others"), post(logout_others))
        .route(&format!("{admin}/locale"), post(set_locale))
        .route("/api/v1/auth/token", post(issue_token))
        .route("/api/v1/auth/token", delete(revoke_token))
        .route("/api/v1/auth/me", get(me))
        .route("/api/v1/auth/jwt", post(issue_jwt))
        .route("/api/v1/auth/refresh", post(refresh_jwt))
        .route("/api/v1/auth/jwt/logout", post(logout_jwt))
        // Unknown admin API paths answer JSON 404 inside the admin stack (session
        // guard, no-store), and never fall through to the panel's HTML pages.
        .route(&base, any(api_not_found))
        .route(&format!("{base}/{{*rest}}"), any(api_not_found))
}

async fn api_not_found() -> AppError {
    AppError::NotFound
}

fn auth_state(app: &AppState) -> AppResult<Arc<AuthState>> {
    app.service::<AuthState>().ok_or_else(|| AppError::internal("AuthProvider not registered"))
}

/// Response of `me`, `login`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Me {
    user: Value,
    guard: String,
    permissions: Vec<String>,
    super_admin: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    abilities: Option<Vec<String>>,
    /// Language of the admin interface for this user and browser.
    locale: String,
    /// The user's saved choice (`null`: follow the browser).
    preferred_locale: Option<String>,
    /// Languages the user may choose.
    locales: Vec<String>,
}

fn me_of(app: &AppState, headers: &HeaderMap, user: &user::Model, principal: &Principal) -> Me {
    let mut permissions: Vec<String> = principal.permissions.iter().cloned().collect();
    permissions.sort();
    let abilities = principal.token_abilities.as_ref().map(|a| {
        let mut a: Vec<String> = a.iter().cloned().collect();
        a.sort();
        a
    });
    Me {
        user: json!({ "id": user.id.to_string(), "name": user.name, "email": user.email }),
        guard: principal.guard.clone(),
        permissions,
        super_admin: principal.super_admin,
        abilities,
        locale: request_locale(app, headers, user.locale.as_deref()).to_owned(),
        preferred_locale: user.locale.clone(),
        locales: app.config().app.locales.clone(),
    }
}

/// Header the admin panel sends with the language it shows: labels in a
/// response then always match the language the panel cached them under.
pub const LOCALE_HEADER: &str = "x-rustcms-locale";

/// The language of the labels in an admin response: the one the panel shows
/// (`X-Rustcms-Locale`) when offered, else [`request_locale`].
pub fn display_locale<'a>(app: &'a AppState, headers: &HeaderMap, preferred: Option<&str>) -> &'a str {
    let shown = headers.get(LOCALE_HEADER).and_then(|v| v.to_str().ok());
    match shown.and_then(|l| app.config().app.locales.iter().find(|offered| offered.as_str() == l)) {
        Some(offered) => offered,
        None => request_locale(app, headers, preferred),
    }
}

/// The interface language a user should see: their choice when still
/// offered, else the browser's, else `app.locale`.
pub fn request_locale<'a>(app: &'a AppState, headers: &HeaderMap, preferred: Option<&str>) -> &'a str {
    let config = &app.config().app;
    let accept = headers.get(header::ACCEPT_LANGUAGE).and_then(|v| v.to_str().ok());
    rustcms_core::i18n::negotiate(preferred, accept, &config.locales, &config.locale)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LocaleChoice {
    locale: Option<String>,
}

/// `POST {admin}/api/auth/locale`: saves the interface language of the
/// signed-in user (`null`: follow the browser). Not a credential change.
async fn set_locale(
    State(app): State<AppState>,
    headers: HeaderMap,
    Auth(current): Auth,
    Json(input): Json<LocaleChoice>,
) -> AppResult<Json<Value>> {
    if let Some(locale) = &input.locale
        && !app.config().app.locales.contains(locale)
    {
        return Err(ValidationErrors::field("locale", "is invalid").into());
    }
    user::Entity::update_many()
        .col_expr(user::Column::Locale, sea_orm::sea_query::Expr::value(input.locale.clone()))
        .filter(user::Column::Id.eq(current.user.id))
        .exec(app.db())
        .await?;
    Ok(Json(json!({ "locale": request_locale(&app, &headers, input.locale.as_deref()) })))
}

#[derive(Deserialize)]
struct Credentials {
    email: String,
    password: String,
}

/// Checks credentials with throttling per account+IP and per IP. The attempt is
/// counted before verifying; a success clears the account and refunds the IP.
async fn attempt(
    app: &AppState,
    auth: &AuthState,
    ip: IpAddr,
    email: &str,
    password: String,
    request: &RequestMeta,
    guard: &str,
) -> AppResult<user::Model> {
    let mut errors = ValidationErrors::new();
    if email.len() > MAX_EMAIL_LEN {
        errors.add("email", format!("may not be longer than {MAX_EMAIL_LEN} characters"));
    }
    if password.len() > MAX_PASSWORD_LEN {
        errors.add("password", format!("may not be longer than {MAX_PASSWORD_LEN} characters"));
    }
    errors.into_result()?;
    let email = email.trim().to_lowercase();
    let account_key = format!("login:{email}|{ip}");
    let ip_key = format!("login-ip:{ip}");
    auth.limiter
        .reserve(&[(&account_key, auth.config.attempts_per_identifier), (&ip_key, auth.config.attempts_per_ip)])
        .await?;

    let user = user::Entity::find().filter(user::Column::Email.eq(&email)).one(app.db()).await?;
    let matches = auth.passwords.verify(password, user.as_ref().map(|u| u.password.clone())).await;
    match user.filter(|u| matches && u.is_active) {
        Some(user) => {
            // Refunds are best effort: a failed one only leaves less budget.
            auth.limiter.clear(&account_key).await.ok();
            auth.limiter.release(&ip_key).await.ok();
            Ok(user)
        }
        None => {
            // One shape whatever the reason (unknown email, wrong password,
            // disabled user): the audit log tells no more than the answer.
            crate::audit::sign_in_failed(app, auth, request, &email, guard).await;
            Err(ValidationErrors::field("email", BAD_CREDENTIALS).into())
        }
    }
}

/// Returns the session's CSRF token. Creating a *new* (anonymous) session is
/// throttled per IP and globally, so anonymous traffic cannot flood storage.
async fn csrf(State(app): State<AppState>, ClientIp(ip): ClientIp, session: Session) -> AppResult<Json<Value>> {
    if !session.is_stored() {
        let auth = auth_state(&app)?;
        auth.limiter
            .reserve(&[
                (&format!("session-new:{ip}"), auth.config.anonymous_sessions_per_ip),
                ("session-new:*", auth.config.anonymous_sessions_per_minute),
            ])
            .await?;
    }
    // The signed-in user the token belongs to, so a client can tell when the
    // cookie changed accounts under an open page.
    Ok(Json(json!({ "csrfToken": session.csrf_token(), "userId": session.user_id().map(|id| id.to_string()) })))
}

async fn login(
    State(app): State<AppState>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    request: RequestMeta,
    session: Session,
    Json(input): Json<Credentials>,
) -> AppResult<(Extension<crate::audit::AfterSession>, Json<Me>)> {
    let auth = auth_state(&app)?;
    let user = attempt(&app, &auth, ip, &input.email, input.password, &request, ADMIN_GUARD).await?;
    session.login(user.id, user.auth_version);
    let principal = auth.resolver.principal(app.db(), ADMIN_GUARD, &user, None).await?;
    // Recorded once the session is stored (the guard does it after this handler).
    let actor = AuditActor::signed_in(user.id, ADMIN_GUARD, Some(rustcms_core::Credential::Session));
    let entry = AuditEntry::new("auth.signed_in", "user").key(user.id);
    let after = crate::audit::AfterSession(AuditContext::new(actor, request), entry);
    Ok((Extension(after), Json(me_of(&app, &headers, &user, &principal))))
}

async fn logout(session: Session, MaybeAuth(current): MaybeAuth, request: RequestMeta) -> Response {
    session.logout();
    match current {
        Some(current) => {
            let context = AuditContext::new(AuditActor::user(&current.principal), request);
            let entry = AuditEntry::new("auth.signed_out", "user").key(current.user.id);
            (Extension(crate::audit::AfterSession(context, entry)), StatusCode::NO_CONTENT).into_response()
        }
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

async fn me(State(app): State<AppState>, headers: HeaderMap, Auth(auth): Auth) -> Json<Me> {
    Json(me_of(&app, &headers, &auth.user, &auth.principal))
}

#[derive(Deserialize)]
struct PasswordChange {
    current_password: String,
    password: String,
    password_confirmation: String,
}

/// Verifies the current password of a signed-in user (throttled per user).
pub async fn confirm_password(auth: &AuthState, user: &user::Model, password: String) -> AppResult<()> {
    if password.len() > MAX_PASSWORD_LEN {
        return Err(ValidationErrors::field("current_password", "The password is incorrect.").into());
    }
    let key = format!("confirm:{}", user.id);
    auth.limiter.reserve(&[(&key, auth.config.attempts_per_identifier)]).await?;
    if auth.passwords.verify(password, Some(user.password.clone())).await {
        auth.limiter.clear(&key).await.ok();
        Ok(())
    } else {
        Err(ValidationErrors::field("current_password", "The password is incorrect.").into())
    }
}

/// Changes the password: other sessions end; API tokens are revoked unless configured otherwise.
async fn change_password(
    State(app): State<AppState>,
    Auth(current): Auth,
    request: RequestMeta,
    session: Session,
    Json(input): Json<PasswordChange>,
) -> AppResult<StatusCode> {
    let auth = auth_state(&app)?;
    confirm_password(&auth, &current.user, input.current_password).await?;
    let mut errors = ValidationErrors::new();
    if input.password.chars().count() < auth.config.password_min_length {
        errors.add("password", format!("must be at least {} characters", auth.config.password_min_length));
    }
    if input.password != input.password_confirmation {
        errors.add("password_confirmation", "does not match");
    }
    if input.password.len() > MAX_PASSWORD_LEN {
        errors.add("password", format!("may not be longer than {MAX_PASSWORD_LEN} characters"));
    }
    errors.into_result()?;
    let hash = auth.passwords.hash(input.password).await?;
    let audit = AuditContext::new(AuditActor::user(&current.principal), request);
    let version = credentials::change_password(
        app.db(),
        &current.user,
        hash,
        auth.config.revoke_tokens_on_password_change,
        &audit,
    )
    .await?;
    session.restamp(version);
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct Confirm {
    password: String,
}

/// Ends every other session and revokes every API token of the user.
async fn logout_others(
    State(app): State<AppState>,
    Auth(current): Auth,
    request: RequestMeta,
    session: Session,
    Json(input): Json<Confirm>,
) -> AppResult<StatusCode> {
    let auth = auth_state(&app)?;
    confirm_password(&auth, &current.user, input.password).await?;
    let audit = AuditContext::new(AuditActor::user(&current.principal), request);
    let version = credentials::logout_everywhere(app.db(), &current.user, &audit).await?;
    session.restamp(version);
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct TokenRequest {
    email: String,
    password: String,
    token_name: String,
    #[serde(default)]
    abilities: Vec<String>,
    expires_in_days: Option<u32>,
}

async fn issue_token(
    State(app): State<AppState>,
    ClientIp(ip): ClientIp,
    request: RequestMeta,
    Json(input): Json<TokenRequest>,
) -> AppResult<(StatusCode, Json<Value>)> {
    let auth = auth_state(&app)?;
    let mut errors = ValidationErrors::new();
    let name = input.token_name.trim();
    if name.is_empty() || name.chars().count() > 255 {
        errors.add("token_name", "must be 1 to 255 characters");
    }
    if input.abilities.len() > MAX_ABILITIES || input.abilities.iter().any(|a| a.is_empty() || a.len() > 255) {
        errors.add("abilities", format!("must be at most {MAX_ABILITIES} non-empty names"));
    }
    let expires_at = input.expires_in_days.map(tokens::expiry_in_days);
    if expires_at.is_some_and(|e| e.is_none()) {
        errors.add("expires_in_days", format!("must be between 1 and {}", tokens::MAX_LIFETIME_DAYS));
    }
    errors.into_result()?;
    let expires_at = expires_at.flatten();
    let user = attempt(&app, &auth, ip, &input.email, input.password, &request, API_GUARD).await?;
    // A concurrent password change between verification and insert means the
    // credentials no longer match: answer as for a wrong password.
    let issued = credentials::issue_token(app.db(), &user, name, &input.abilities, expires_at, &request)
        .await
        .map_err(|e| match e {
            AppError::Unauthenticated => ValidationErrors::field("email", BAD_CREDENTIALS).into(),
            other => other,
        })?;
    let mut abilities: Vec<String> = tokens::abilities(&issued.token).into_iter().collect();
    abilities.sort();
    Ok((
        StatusCode::CREATED,
        Json(json!({ "token": issued.plain_text, "abilities": abilities, "expiresAt": issued.token.expires_at })),
    ))
}

async fn revoke_token(State(app): State<AppState>, Auth(current): Auth, request: RequestMeta) -> AppResult<StatusCode> {
    let token_id = current.token_id.ok_or(AppError::Unauthenticated)?;
    // The revocation and its entry commit together.
    let txn = rustcms_core::db::begin_write(app.db()).await?;
    tokens::revoke(&txn, token_id).await?;
    let context = AuditContext::new(AuditActor::user(&current.principal), request);
    rustcms_core::audit::record(&txn, &context, AuditEntry::new("token.revoked", "user").key(current.user.id)).await?;
    txn.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Refreshes and logouts allowed per IP per minute (before any database work).
const REFRESHES_PER_IP: u32 = 60;
/// Refreshes allowed per family per minute (successes count too).
const REFRESHES_PER_FAMILY: u32 = 30;
/// JWT sign-ins allowed per user per minute (successes count too: bounds families).
const JWT_ISSUES_PER_USER: u32 = 20;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JwtRequest {
    email: String,
    password: String,
    /// Omitted: all of the user's API permissions (`*`).
    abilities: Option<Vec<String>>,
}

fn jwt_response(issued: jwt_session::Issued) -> Json<Value> {
    Json(json!({
        "accessToken": issued.access_token,
        "tokenType": "Bearer",
        "expiresIn": issued.expires_in,
        "refreshToken": issued.refresh_token,
        "refreshExpiresAt": issued.refresh_expires_at,
        "abilities": issued.abilities,
    }))
}

fn jwt_enabled(auth: &AuthState) -> AppResult<()> {
    if auth.config.jwt.enabled { Ok(()) } else { Err(AppError::NotFound) }
}

/// `POST /api/v1/auth/jwt`: signs in for an access token and a refresh token.
async fn issue_jwt(
    State(app): State<AppState>,
    ClientIp(ip): ClientIp,
    request: RequestMeta,
    Json(input): Json<JwtRequest>,
) -> AppResult<(StatusCode, Json<Value>)> {
    let auth = auth_state(&app)?;
    jwt_enabled(&auth)?;
    if let Some(abilities) = &input.abilities
        && (abilities.is_empty()
            || abilities.len() > MAX_ABILITIES
            || abilities.iter().any(|a| a.is_empty() || a.len() > 255)
            || abilities.iter().map(String::len).sum::<usize>() > JWT_ABILITY_BYTES)
    {
        return Err(ValidationErrors::field(
            "abilities",
            format!("must be 1 to {MAX_ABILITIES} non-empty names, {JWT_ABILITY_BYTES} bytes in all"),
        )
        .into());
    }
    let user = attempt(&app, &auth, ip, &input.email, input.password, &request, API_GUARD).await?;
    auth.limiter.reserve(&[(&format!("jwt-issue:{}", user.id), JWT_ISSUES_PER_USER)]).await?;
    let issued = jwt_session::issue(&auth, app.db(), &user, input.abilities.as_deref().unwrap_or_default(), &request)
        .await
        .map_err(|e| match e {
            // The credentials changed since they were checked: as for a wrong password.
            AppError::Unauthenticated => ValidationErrors::field("email", BAD_CREDENTIALS).into(),
            other => other,
        })?;
    Ok((StatusCode::CREATED, jwt_response(issued)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RefreshRequest {
    refresh_token: String,
}

/// Counts a refresh or logout: per IP first, then (once the token's secret is
/// known good) per family. Over the limit: 429; a bad token: 401.
async fn refresh_limits(app: &AppState, auth: &AuthState, ip: IpAddr, token: &str) -> AppResult<()> {
    auth.limiter.reserve(&[(&format!("refresh-ip:{ip}"), REFRESHES_PER_IP)]).await?;
    if token.len() > jwt_session::MAX_REFRESH_LEN {
        return Err(AppError::Unauthenticated);
    }
    let family = jwt_session::family_of(auth, app.db(), token).await?;
    auth.limiter.reserve(&[(&format!("refresh-family:{family}"), REFRESHES_PER_FAMILY)]).await?;
    Ok(())
}

/// `POST /api/v1/auth/refresh`: a new access token and the next refresh token.
/// The refresh token is single-use: using one twice ends the sign-in.
async fn refresh_jwt(
    State(app): State<AppState>,
    ClientIp(ip): ClientIp,
    request: RequestMeta,
    Json(input): Json<RefreshRequest>,
) -> AppResult<Json<Value>> {
    let auth = auth_state(&app)?;
    jwt_enabled(&auth)?;
    refresh_limits(&app, &auth, ip, &input.refresh_token).await?;
    Ok(jwt_response(jwt_session::rotate(&auth, app.db(), &input.refresh_token, &request).await?))
}

/// `POST /api/v1/auth/jwt/logout`: ends the sign-in of a refresh token (works
/// after the access token expired). 204, also when it had already ended.
async fn logout_jwt(
    State(app): State<AppState>,
    ClientIp(ip): ClientIp,
    request: RequestMeta,
    Json(input): Json<RefreshRequest>,
) -> AppResult<StatusCode> {
    let auth = auth_state(&app)?;
    jwt_enabled(&auth)?;
    refresh_limits(&app, &auth, ip, &input.refresh_token).await?;
    jwt_session::logout(&auth, app.db(), &input.refresh_token, &request).await?;
    Ok(StatusCode::NO_CONTENT)
}
