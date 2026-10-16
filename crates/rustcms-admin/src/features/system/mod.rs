//! System pages of the panel: users, roles and permissions (`<admin>/api/system/…`).
//!
//! Every change goes through [`RbacService`] (escalation rules, last super admin)
//! or the credential helpers; nothing here writes RBAC tables directly.

mod dto;
pub(crate) mod roles;
mod users;

pub use dto::*;
use roles::*;
use users::*;

use std::collections::{BTreeMap, HashMap};

use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, put};
use rustcms_auth::entities::{USER_MODEL, model_has_roles as user_role, permission, role, role_has_permissions};
use rustcms_auth::{ADMIN_GUARD, Actor, Auth, AuthState, NewUser, RbacService, Session};
use rustcms_core::models::user;
use rustcms_core::{AppError, AppResult, AppState, Json, Principal, ValidationErrors};
use sea_orm::{ColumnTrait, Condition, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder};
use serde::{Deserialize, Serialize};

use crate::http::AdminCtx;
use crate::resource::PageMeta;

/// Every system route as `(method, path)` (route listings, authorization oracle).
pub fn routes(admin_path: &str) -> Vec<(Method, String)> {
    let base = format!("{}/api/system", admin_path.trim_end_matches('/'));
    vec![
        (Method::GET, format!("{base}/users")),
        (Method::POST, format!("{base}/users")),
        (Method::GET, format!("{base}/users/{{id}}")),
        (Method::PATCH, format!("{base}/users/{{id}}")),
        (Method::DELETE, format!("{base}/users/{{id}}")),
        (Method::PUT, format!("{base}/users/{{id}}/roles")),
        (Method::PUT, format!("{base}/users/{{id}}/active")),
        (Method::PUT, format!("{base}/users/{{id}}/password")),
        (Method::POST, format!("{base}/users/invitations")),
        (Method::POST, format!("{base}/users/{{id}}/invitation")),
        (Method::GET, format!("{base}/roles")),
        (Method::POST, format!("{base}/roles")),
        (Method::GET, format!("{base}/roles/{{id}}")),
        (Method::DELETE, format!("{base}/roles/{{id}}")),
        (Method::PUT, format!("{base}/roles/{{id}}/permissions")),
        (Method::GET, format!("{base}/permissions")),
    ]
}

pub(crate) fn router(router: Router<AppState>, admin_path: &str) -> Router<AppState> {
    let base = format!("{}/api/system", admin_path.trim_end_matches('/'));
    router
        .route(&format!("{base}/users"), get(list_users).post(create_user))
        .route(&format!("{base}/users/{{id}}"), get(show_user).patch(update_user).delete(delete_user))
        .route(&format!("{base}/users/{{id}}/roles"), put(set_roles))
        .route(&format!("{base}/users/{{id}}/active"), put(set_active))
        .route(&format!("{base}/users/{{id}}/password"), put(reset_password))
        .route(&format!("{base}/users/invitations"), axum::routing::post(invite_user))
        .route(&format!("{base}/users/{{id}}/invitation"), axum::routing::post(resend_invitation))
        .route(&format!("{base}/roles"), get(list_roles).post(create_role))
        .route(&format!("{base}/roles/{{id}}"), get(show_role).delete(delete_role))
        .route(&format!("{base}/roles/{{id}}/permissions"), put(set_permissions))
        .route(&format!("{base}/permissions"), get(list_permissions))
}

fn auth(app: &AppState) -> AppResult<std::sync::Arc<AuthState>> {
    app.service::<AuthState>().ok_or_else(|| AppError::internal("AuthProvider not registered"))
}

fn require(principal: &Principal, permission: &str) -> AppResult<()> {
    if principal.can(permission) { Ok(()) } else { Err(AppError::Forbidden) }
}

fn id_of(raw: &str) -> AppResult<i64> {
    raw.parse().map_err(|_| AppError::NotFound)
}

fn ids_of(field: &str, raw: &[String]) -> AppResult<Vec<i64>> {
    if raw.len() > 1000 {
        return Err(ValidationErrors::field(field, "has too many roles").into());
    }
    raw.iter()
        .map(|id| id.parse().map_err(|_| AppError::from(ValidationErrors::field(field, "has unknown roles"))))
        .collect()
}

/// Ids deduplicated and split so no query binds more parameters than every
/// backend accepts (SQLite: 32 766; PostgreSQL: 65 535).
fn chunks(ids: impl IntoIterator<Item = i64>) -> Vec<Vec<i64>> {
    let unique: std::collections::BTreeSet<i64> = ids.into_iter().collect();
    unique.into_iter().collect::<Vec<_>>().chunks(500).map(<[i64]>::to_vec).collect()
}

/// Roles held by each of `users`, in every guard (one query each).
async fn roles_of(app: &AppState, users: &[i64]) -> AppResult<HashMap<i64, Vec<role::Model>>> {
    let pivots = user_role::Entity::find()
        .filter(user_role::Column::ModelType.eq(USER_MODEL))
        .filter(user_role::Column::ModelId.is_in(users.iter().map(i64::to_string)))
        .all(app.db())
        .await?;
    let mut roles: HashMap<i64, role::Model> = HashMap::new();
    for chunk in chunks(pivots.iter().map(|p| p.role_id)) {
        roles.extend(
            role::Entity::find()
                .filter(role::Column::Id.is_in(chunk))
                .all(app.db())
                .await?
                .into_iter()
                .map(|r| (r.id, r)),
        );
    }
    let mut out: HashMap<i64, Vec<role::Model>> = HashMap::new();
    for pivot in pivots {
        if let (Ok(user), Some(role)) = (pivot.model_id.parse::<i64>(), roles.get(&pivot.role_id)) {
            out.entry(user).or_default().push(role.clone());
        }
    }
    for list in out.values_mut() {
        list.sort_by(|a, b| a.name.cmp(&b.name));
    }
    Ok(out)
}
