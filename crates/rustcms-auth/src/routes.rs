//! Auth endpoints of both guards.

use std::net::IpAddr;
use std::sync::Arc;

use axum::Router;
use axum::extract::State;
use axum::http::{Method, StatusCode};
use axum::routing::{any, delete, get, post};
use chrono::{Duration, Utc};
use rustcms_core::models::user;
use rustcms_core::{AppError, AppResult, AppState, ClientIp, Json, Principal, ValidationErrors};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::credentials;
use crate::guard::Auth;
use crate::session::Session;
use crate::state::{ADMIN_GUARD, AuthState};
use crate::tokens;

const BAD_CREDENTIALS: &str = "These credentials do not match our records.";
const MAX_ABILITIES: usize = 100;
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
        (Method::POST, "/api/v1/auth/token".into()),
        (Method::DELETE, "/api/v1/auth/token".into()),
        (Method::GET, "/api/v1/auth/me".into()),
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
        .route("/api/v1/auth/token", post(issue_token))
        .route("/api/v1/auth/token", delete(revoke_token))
        .route("/api/v1/auth/me", get(me))
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
}

fn me_of(user: &user::Model, principal: &Principal) -> Me {
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
    }
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
        .reserve(&[(&account_key, auth.config.attempts_per_identifier), (&ip_key, auth.config.attempts_per_ip)])?;

    let user = user::Entity::find().filter(user::Column::Email.eq(&email)).one(app.db()).await?;
    let matches = auth.passwords.verify(password, user.as_ref().map(|u| u.password.clone())).await;
    match user.filter(|u| matches && u.is_active) {
        Some(user) => {
            auth.limiter.clear(&account_key);
            auth.limiter.release(&ip_key);
            Ok(user)
        }
        None => Err(ValidationErrors::field("email", BAD_CREDENTIALS).into()),
    }
}

/// Returns the session's CSRF token. Creating a *new* (anonymous) session is
/// throttled per IP and globally, so anonymous traffic cannot flood storage.
async fn csrf(State(app): State<AppState>, ClientIp(ip): ClientIp, session: Session) -> AppResult<Json<Value>> {
    if !session.is_stored() {
        let auth = auth_state(&app)?;
        auth.limiter.reserve(&[
            (&format!("session-new:{ip}"), auth.config.anonymous_sessions_per_ip),
            ("session-new:*", auth.config.anonymous_sessions_per_minute),
        ])?;
    }
    // The signed-in user the token belongs to, so a client can tell when the
    // cookie changed accounts under an open page.
    Ok(Json(json!({ "csrfToken": session.csrf_token(), "userId": session.user_id().map(|id| id.to_string()) })))
}

async fn login(
    State(app): State<AppState>,
    ClientIp(ip): ClientIp,
    session: Session,
    Json(input): Json<Credentials>,
) -> AppResult<Json<Me>> {
    let auth = auth_state(&app)?;
    let user = attempt(&app, &auth, ip, &input.email, input.password).await?;
    session.login(user.id, user.auth_version);
    let principal = auth.resolver.principal(app.db(), ADMIN_GUARD, &user, None).await?;
    Ok(Json(me_of(&user, &principal)))
}

async fn logout(session: Session) -> StatusCode {
    session.logout();
    StatusCode::NO_CONTENT
}

async fn me(Auth(auth): Auth) -> Json<Me> {
    Json(me_of(&auth.user, &auth.principal))
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
    auth.limiter.reserve(&[(&key, auth.config.attempts_per_identifier)])?;
    if auth.passwords.verify(password, Some(user.password.clone())).await {
        auth.limiter.clear(&key);
        Ok(())
    } else {
        Err(ValidationErrors::field("current_password", "The password is incorrect.").into())
    }
}

/// Changes the password: other sessions end; API tokens are revoked unless configured otherwise.
async fn change_password(
    State(app): State<AppState>,
    Auth(current): Auth,
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
    let version =
        credentials::change_password(app.db(), &current.user, hash, auth.config.revoke_tokens_on_password_change)
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
    session: Session,
    Json(input): Json<Confirm>,
) -> AppResult<StatusCode> {
    let auth = auth_state(&app)?;
    confirm_password(&auth, &current.user, input.password).await?;
    let version = credentials::logout_everywhere(app.db(), &current.user).await?;
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
    errors.into_result()?;
    let user = attempt(&app, &auth, ip, &input.email, input.password).await?;
    let expires_at = input.expires_in_days.map(|days| Utc::now() + Duration::days(i64::from(days)));
    // A concurrent password change between verification and insert means the
    // credentials no longer match: answer as for a wrong password.
    let issued =
        credentials::issue_token(app.db(), &user, name, &input.abilities, expires_at).await.map_err(|e| match e {
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

async fn revoke_token(State(app): State<AppState>, Auth(current): Auth) -> AppResult<StatusCode> {
    let token_id = current.token_id.ok_or(AppError::Unauthenticated)?;
    tokens::revoke(app.db(), token_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
