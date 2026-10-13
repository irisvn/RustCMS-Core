//! Server-side sessions for the admin guard.
//!
//! The cookie holds 43 random characters; the database stores only their
//! SHA-256, so a leaked `sessions` table does not leak usable sessions.
//! The id is regenerated on sign-in and when credentials change (fixation).
//! A session re-issued by a credential change keeps its *lineage* (the id of
//! the first session of the chain). Sign-out and re-issue both take a lock on
//! the lineage first, so they never interleave: sign-out ends every session of
//! the lineage, and a re-issue whose predecessor is gone does nothing. Anonymous sessions (a CSRF token only) have a fixed lifetime:
//! activity never extends them, which bounds how many can be alive.

use std::sync::{Arc, Mutex, MutexGuard};

use axum::http::{HeaderMap, HeaderValue, header};
use chrono::{DateTime, Duration, Utc};
use rustcms_core::AppResult;
use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, ConnectionTrait, DatabaseConnection, DbBackend, EntityTrait, QueryFilter,
    Set,
};
use serde::{Deserialize, Serialize};

use crate::entities::session as entity;
use crate::secret::random_string;
use crate::state::AuthState;

const TOKEN_LEN: usize = 43;
/// Sliding expiry is written back at most once per this many seconds.
const TOUCH_EVERY_SECS: i64 = 60;
/// Longest wait for an optional sliding renewal.
const RENEWAL_BUDGET: std::time::Duration = std::time::Duration::from_secs(2);
/// Time kept before the request deadline to send the response.
const RENEWAL_MARGIN: std::time::Duration = std::time::Duration::from_millis(250);
/// Namespace of the per-lineage PostgreSQL advisory locks.
const LINEAGE_LOCK_SPACE: i32 = 0x7365_7373;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Data {
    user_id: Option<i64>,
    auth_version: i64,
    csrf: Option<String>,
}

#[derive(Debug, Default)]
struct State {
    stored_id: Option<String>,
    token: Option<String>,
    stored_expires_at: Option<DateTime<Utc>>,
    lineage: Option<String>,
    data: Data,
    changed: bool,
    regenerate: bool,
    restamp: bool,
    destroy: bool,
    touch: bool,
}

impl State {
    /// A session ended earlier in this request is replaced by a new one (new id).
    fn revive(&mut self) {
        if self.destroy {
            self.destroy = false;
            self.regenerate = true;
        }
    }
}

/// The current admin session (extractor). Only present under the admin path.
#[derive(Debug, Clone, Default)]
pub struct Session(Arc<Mutex<State>>);

impl Session {
    fn state(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// CSRF token of this session, created (and the session persisted) if missing.
    /// After [`Session::logout`] in the same request, this starts a new session.
    pub fn csrf_token(&self) -> String {
        let mut state = self.state();
        if let Some(token) = &state.data.csrf {
            return token.clone();
        }
        let token = random_string(40);
        state.data.csrf = Some(token.clone());
        state.changed = true;
        state.revive();
        token
    }

    pub(crate) fn csrf_matches(&self, candidate: &str) -> bool {
        self.state().data.csrf.as_deref().is_some_and(|token| crate::secret::ct_eq(token, candidate))
    }

    /// Whether this session already exists in storage.
    pub fn is_stored(&self) -> bool {
        self.state().stored_id.is_some()
    }

    /// Signed-in user, if any.
    pub fn user_id(&self) -> Option<i64> {
        self.state().data.user_id
    }

    pub(crate) fn auth_version(&self) -> i64 {
        self.state().data.auth_version
    }

    /// Signs `user_id` in: new session id and CSRF token.
    pub fn login(&self, user_id: i64, auth_version: i64) {
        let mut state = self.state();
        state.data = Data { user_id: Some(user_id), auth_version, csrf: Some(random_string(40)) };
        state.changed = true;
        state.regenerate = true;
        state.restamp = false;
        state.destroy = false;
    }

    /// Keeps the user signed in after their credentials changed elsewhere in this request.
    /// Leaves the stored session untouched by this request: no sliding renewal and
    /// no cookie (a stale request must not re-send a cookie that a concurrent
    /// rotation is replacing).
    pub(crate) fn freeze(&self) {
        self.state().touch = false;
    }

    /// Re-issues the session for a new credential version (keeps the user signed in).
    pub fn restamp(&self, auth_version: i64) {
        let mut state = self.state();
        state.data.auth_version = auth_version;
        state.data.csrf = Some(random_string(40));
        state.changed = true;
        state.regenerate = true;
        state.restamp = true;
    }

    /// Ends the session.
    pub fn logout(&self) {
        let mut state = self.state();
        state.data = Data::default();
        state.destroy = true;
    }
}

/// Loads the session named by the request cookie.
pub(crate) async fn load<C: ConnectionTrait>(auth: &AuthState, db: &C, headers: &HeaderMap) -> AppResult<Session> {
    let Some(token) = cookie(headers, &auth.config.cookie_name) else { return Ok(Session::default()) };
    let id = auth.session_id(&token);
    let row = entity::Entity::find_by_id(id.clone()).filter(entity::Column::ExpiresAt.gt(Utc::now())).one(db).await?;
    let Some(row) = row else { return Ok(Session::default()) };
    let data: Data = serde_json::from_str(&row.data).unwrap_or_default();
    let fresh_until = Utc::now() + lifetime_of(auth, &data) - Duration::seconds(TOUCH_EVERY_SECS);
    let state = State {
        stored_id: Some(id.clone()),
        token: Some(token),
        stored_expires_at: Some(row.expires_at),
        lineage: Some(row.lineage.unwrap_or_else(|| id.clone())),
        touch: data.user_id.is_some() && row.expires_at < fresh_until,
        data,
        ..Default::default()
    };
    Ok(Session(Arc::new(Mutex::new(state))))
}

/// Writes the session back and returns the `Set-Cookie` header to add, if any.
///
/// - A **restamp** (credential change) replaces the row within its lineage,
///   under the lineage lock, only if the old session still exists: a concurrent
///   sign-out stands. Sign-in starts a new lineage.
/// - Sign-out ends every session of the lineage, under the same lock.
/// - A pure **renewal** (sliding expiry of a signed-in session) is best effort:
///   a failure is logged and never replaces the handler's response.
/// - Sign-in, sign-out, restamp and new data are essential: failures propagate.
pub(crate) async fn persist(
    auth: &AuthState,
    db: &DatabaseConnection,
    session: &Session,
) -> AppResult<Option<HeaderValue>> {
    let (stored_id, token, stored_expires_at, lineage, data, changed, regenerate, restamp, destroy, touch) = {
        let s = session.state();
        (
            s.stored_id.clone(),
            s.token.clone(),
            s.stored_expires_at,
            s.lineage.clone(),
            s.data.clone(),
            s.changed,
            s.regenerate,
            s.restamp,
            s.destroy,
            s.touch,
        )
    };
    let lifetime = lifetime_of(auth, &data);
    let expires_at = Utc::now() + lifetime;
    if destroy {
        if let (Some(id), Some(lineage)) = (stored_id, lineage) {
            end(db, &id, &lineage).await?;
            return Ok(Some(clear_cookie(auth)));
        }
        return Ok(None);
    }
    if regenerate || (stored_id.is_none() && changed) {
        let token = match (stored_id, lineage) {
            // Credential change: same lineage, only if the predecessor still exists.
            (Some(old), Some(lineage)) if restamp => {
                let txn = rustcms_core::db::begin_write(db).await?;
                lock_lineage(&txn, &lineage).await?;
                let deleted = entity::Entity::delete_by_id(old).exec(&txn).await?.rows_affected;
                if deleted == 0 {
                    // Signed out by a concurrent request: do not sign the browser back in.
                    return Ok(Some(clear_cookie(auth)));
                }
                let token = insert(auth, &txn, &data, expires_at, Some(lineage.clone())).await?;
                txn.commit().await?;
                state_lineage(session, lineage);
                token
            }
            // Sign-in (or a fresh anonymous session): a new lineage.
            (old, _) => {
                if let Some(old) = old {
                    entity::Entity::delete_by_id(old).exec(db).await?;
                }
                let token = insert(auth, db, &data, expires_at, None).await?;
                state_lineage(session, auth.session_id(&token));
                token
            }
        };
        let mut state = session.state();
        state.stored_id = Some(auth.session_id(&token));
        state.token = Some(token.clone());
        state.stored_expires_at = Some(expires_at);
        state.changed = false;
        state.regenerate = false;
        state.restamp = false;
        return Ok(Some(set_cookie(auth, &token)));
    }
    if let Some(id) = stored_id
        && token.is_some()
        && (changed || touch)
    {
        // Anonymous sessions keep their original expiry.
        let expires_at = if data.user_id.is_some() { expires_at } else { stored_expires_at.unwrap_or(expires_at) };
        let update = entity::Entity::update_many()
            .col_expr(entity::Column::Data, Expr::value(encode(&data)))
            .col_expr(entity::Column::ExpiresAt, Expr::value(expires_at))
            .filter(entity::Column::Id.eq(id))
            .exec(db);
        let renewed = if changed {
            update.await
        } else {
            // A pure renewal is optional: it never outlasts the request deadline
            // (e.g. waiting on a locked row), so a committed write still answers.
            let budget = rustcms_core::http::request_deadline()
                .map_or(RENEWAL_BUDGET, |deadline| {
                    deadline.saturating_duration_since(tokio::time::Instant::now()).saturating_sub(RENEWAL_MARGIN)
                })
                .min(RENEWAL_BUDGET);
            match tokio::time::timeout(budget, update).await {
                Ok(result) => result,
                Err(_) => {
                    tracing::warn!("session renewal timed out; keeping the response");
                    return Ok(None);
                }
            }
        };
        match renewed {
            Err(err) if changed => return Err(err.into()),
            Err(err) => tracing::warn!(%err, "session renewal failed; keeping the response"),
            // No row updated: another request ended or rotated this session
            // meanwhile, and it stays that way.
            Ok(_) => {}
        }
        // The cookie value is unchanged and carries no expiry (see `set_cookie`):
        // nothing is re-sent, so a slow response can never put back a cookie
        // that a newer one replaced.
        return Ok(None);
    }
    Ok(None)
}

fn state_lineage(session: &Session, lineage: String) {
    session.state().lineage = Some(lineage);
}

/// Serializes sign-out and re-issue of one lineage until the transaction ends:
/// a transaction-scoped advisory lock on PostgreSQL; on SQLite the
/// [`rustcms_core::db::begin_write`] transaction already holds the write lock.
pub async fn lock_lineage<C: ConnectionTrait>(db: &C, lineage: &str) -> AppResult<()> {
    if db.get_database_backend() == DbBackend::Postgres {
        let key = i32::from_str_radix(&lineage[..7.min(lineage.len())], 16).unwrap_or_default();
        db.execute_unprepared(&format!("SELECT pg_advisory_xact_lock({LINEAGE_LOCK_SPACE}, {key})")).await?;
    }
    Ok(())
}

/// Ends session `id` and every session of its `lineage`, under the lineage
/// lock: a re-issue either completed before (and is deleted here) or runs after
/// (and finds its predecessor gone).
pub async fn end(db: &DatabaseConnection, id: &str, lineage: &str) -> AppResult<()> {
    let txn = rustcms_core::db::begin_write(db).await?;
    lock_lineage(&txn, lineage).await?;
    entity::Entity::delete_many()
        .filter(
            Condition::any()
                .add(entity::Column::Id.eq(id))
                .add(entity::Column::Id.eq(lineage))
                .add(entity::Column::Lineage.eq(lineage)),
        )
        .exec(&txn)
        .await?;
    txn.commit().await?;
    Ok(())
}

/// Creates a session for `user_id` without a request (tests, impersonation tools).
/// Returns `(cookie value, csrf token)`.
pub async fn create_for_user<C: ConnectionTrait>(
    auth: &AuthState,
    db: &C,
    user_id: i64,
    auth_version: i64,
) -> AppResult<(String, String)> {
    let csrf = random_string(40);
    let data = Data { user_id: Some(user_id), auth_version, csrf: Some(csrf.clone()) };
    let token = insert(auth, db, &data, Utc::now() + lifetime(auth), None).await?;
    Ok((token, csrf))
}

/// Deletes expired sessions; returns how many.
pub async fn prune_expired<C: ConnectionTrait>(db: &C) -> AppResult<u64> {
    Ok(entity::Entity::delete_many().filter(entity::Column::ExpiresAt.lte(Utc::now())).exec(db).await?.rows_affected)
}

async fn insert<C: ConnectionTrait>(
    auth: &AuthState,
    db: &C,
    data: &Data,
    expires_at: DateTime<Utc>,
    lineage: Option<String>,
) -> AppResult<String> {
    let token = random_string(TOKEN_LEN);
    entity::ActiveModel {
        id: Set(auth.session_id(&token)),
        data: Set(encode(data)),
        expires_at: Set(expires_at),
        lineage: Set(lineage),
    }
    .insert(db)
    .await?;
    Ok(token)
}

fn encode(data: &Data) -> String {
    serde_json::to_string(data).unwrap_or_default()
}

/// Anonymous sessions (only a CSRF token) live shorter than signed-in ones.
fn lifetime_of(auth: &AuthState, data: &Data) -> Duration {
    if data.user_id.is_some() {
        lifetime(auth)
    } else {
        Duration::minutes(i64::try_from(auth.config.anonymous_session_lifetime_minutes).unwrap_or(15))
    }
}

fn lifetime(auth: &AuthState) -> Duration {
    Duration::minutes(i64::try_from(auth.config.session_lifetime_minutes).unwrap_or(i64::MAX / 120_000))
}

/// The session cookie. It lives until the browser closes: expiry (idle for
/// signed-in sessions, fixed for anonymous ones) is enforced by the server, so
/// the cookie is only sent when its value changes.
fn set_cookie(auth: &AuthState, value: &str) -> HeaderValue {
    cookie_header(auth, value, "")
}

/// Removes the session cookie.
fn clear_cookie(auth: &AuthState) -> HeaderValue {
    cookie_header(auth, "", "; Max-Age=0")
}

fn cookie_header(auth: &AuthState, value: &str, max_age: &str) -> HeaderValue {
    let secure = if auth.secure_cookies { "; Secure" } else { "" };
    let cookie = format!(
        "{}={value}; Path={}{max_age}; HttpOnly; SameSite=Lax{secure}",
        auth.config.cookie_name, auth.config.admin_path
    );
    HeaderValue::from_str(&cookie).unwrap_or_else(|_| HeaderValue::from_static(""))
}

/// Value of cookie `name` in the request.
pub(crate) fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v.to_owned())
        .filter(|v| !v.is_empty())
}
