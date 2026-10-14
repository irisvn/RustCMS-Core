//! Credential revocation (see the revocation table in the security docs).
//!
//! Operations that act on credentials verified earlier in the request (a
//! password checked with Argon2, a session's `auth_version`) re-check that
//! snapshot inside a write transaction holding the user row, so a concurrent
//! password change or revocation cannot be undone or bypassed.

use chrono::{DateTime, Utc};
use rustcms_core::models::user;
use rustcms_core::{AppError, AppResult};
use sea_orm::sea_query::{Expr, ExprTrait};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter, QuerySelect, Set,
};

use crate::entities::personal_access_token as token;
use crate::tokens::{self, IssuedToken};

/// Reloads the user under a lock (row lock on PostgreSQL; on SQLite the
/// [`rustcms_core::db::begin_write`] transaction already holds the write lock)
/// and fails unless it still matches `snapshot`: same password hash, same
/// `auth_version`, still active.
pub async fn lock_unchanged<C: ConnectionTrait>(db: &C, snapshot: &user::Model) -> AppResult<user::Model> {
    let current = user::Entity::find_by_id(snapshot.id).lock_exclusive().one(db).await?;
    current
        .filter(|u| u.is_active && u.auth_version == snapshot.auth_version && u.password == snapshot.password)
        .ok_or(AppError::Unauthenticated)
}

/// Invalidates every session of `user_id` (by bumping `auth_version`) and, if
/// `tokens`, revokes all of their API tokens. Returns the new `auth_version`.
///
/// Run inside the transaction that changes the account, so the revocation
/// commits or rolls back with it.
pub async fn revoke_all<C: ConnectionTrait>(db: &C, user_id: i64, tokens: bool) -> AppResult<i64> {
    let now = Utc::now();
    user::Entity::update_many()
        .col_expr(user::Column::AuthVersion, Expr::col(user::Column::AuthVersion).add(1))
        .col_expr(user::Column::UpdatedAt, Expr::value(now))
        .filter(user::Column::Id.eq(user_id))
        .exec(db)
        .await?;
    if tokens {
        revoke_tokens(db, user_id).await?;
    }
    let user = user::Entity::find_by_id(user_id).one(db).await?.ok_or(AppError::NotFound)?;
    Ok(user.auth_version)
}

/// Revokes every active API token of `user_id`.
pub async fn revoke_tokens<C: ConnectionTrait>(db: &C, user_id: i64) -> AppResult<()> {
    token::Entity::update_many()
        .col_expr(token::Column::RevokedAt, Expr::value(Utc::now()))
        .filter(token::Column::UserId.eq(user_id))
        .filter(token::Column::RevokedAt.is_null())
        .exec(db)
        .await?;
    Ok(())
}

/// Sets a new password hash for the user verified as `snapshot` and revokes
/// their other credentials. Returns the new `auth_version` for the caller's session.
pub async fn change_password(
    db: &DatabaseConnection,
    snapshot: &user::Model,
    new_hash: String,
    revoke_api_tokens: bool,
) -> AppResult<i64> {
    let txn = rustcms_core::db::begin_write(db).await?;
    let current = lock_unchanged(&txn, snapshot).await?;
    let mut model: user::ActiveModel = current.into();
    model.password = Set(new_hash);
    model.update(&txn).await?;
    let version = revoke_all(&txn, snapshot.id, revoke_api_tokens).await?;
    txn.commit().await?;
    Ok(version)
}

/// Ends every session and token of the user verified as `snapshot`; returns the new `auth_version`.
pub async fn logout_everywhere(db: &DatabaseConnection, snapshot: &user::Model) -> AppResult<i64> {
    let txn = rustcms_core::db::begin_write(db).await?;
    lock_unchanged(&txn, snapshot).await?;
    let version = revoke_all(&txn, snapshot.id, true).await?;
    txn.commit().await?;
    Ok(version)
}

/// Issues a token for the user whose password was verified as `snapshot`.
/// Fails if their credentials changed since, so a token cannot outlive the
/// revocation of the password it was obtained with.
pub async fn issue_token(
    db: &DatabaseConnection,
    snapshot: &user::Model,
    name: &str,
    abilities: &[String],
    expires_at: Option<DateTime<Utc>>,
) -> AppResult<IssuedToken> {
    let txn = rustcms_core::db::begin_write(db).await?;
    lock_unchanged(&txn, snapshot).await?;
    let issued = tokens::issue(&txn, snapshot.id, name, abilities, expires_at).await?;
    txn.commit().await?;
    Ok(issued)
}
