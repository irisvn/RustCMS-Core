//! JWT sign-ins: refresh families and their rotating tokens.
//!
//! Lock order everywhere: user → family → token, in a write transaction, so
//! issuance, rotation, logout and credential revocation never interleave.
//! A refresh token's secret is checked before anything is decided about it:
//! someone who only knows (or guesses) a token id changes nothing.

use std::collections::HashSet;

use chrono::{DateTime, Duration, Utc};
use rustcms_core::models::user;
use rustcms_core::{AppError, AppResult};
use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, DatabaseTransaction, DbBackend, EntityTrait,
    QueryFilter, QuerySelect, Set,
};

use crate::entities::{jwt_family as family, jwt_refresh_token as refresh};
use crate::guards::jwt::{self, Claims};
use crate::secret::{ct_eq, hmac_sha256_hex, random_string};
use crate::state::AuthState;
use rustcms_core::Credential;
use rustcms_core::audit::{AuditActor, AuditContext, AuditEntry, RequestMeta};

/// Prefix of refresh tokens: `rcmsr_<id>_<secret>`.
pub const REFRESH_PREFIX: &str = "rcmsr_";
const SECRET_LEN: usize = 40;
/// Longest refresh token read (bytes).
pub const MAX_REFRESH_LEN: usize = 80;
/// Families deleted per prune statement.
const PRUNE_BATCH: u64 = 500;

/// What a client receives after signing in or refreshing.
#[derive(Debug)]
pub struct Issued {
    /// The access token (a JWT).
    pub access_token: String,
    /// Its lifetime in seconds.
    pub expires_in: u64,
    /// The next refresh token.
    pub refresh_token: String,
    /// When the refresh token stops working if unused.
    pub refresh_expires_at: DateTime<Utc>,
    /// The family's abilities.
    pub abilities: Vec<String>,
}

/// The user and family of a valid access token.
#[derive(Debug)]
pub struct Verified {
    /// The user (active, same `auth_version`).
    pub user: user::Model,
    /// The family's abilities.
    pub abilities: HashSet<String>,
    /// The family id.
    pub family: String,
}

fn locked<E: EntityTrait>(find: sea_orm::Select<E>, txn: &DatabaseTransaction) -> sea_orm::Select<E> {
    // SQLite: the write transaction already holds the database lock.
    if txn.get_database_backend() == DbBackend::Sqlite { find } else { find.lock_exclusive() }
}

fn abilities_of(family: &family::Model) -> Vec<String> {
    let mut abilities: Vec<String> = serde_json::from_str(&family.abilities).unwrap_or_default();
    abilities.sort();
    abilities
}

/// `rcmsr_<id>_<secret>` → (id, secret), or `None` for anything else.
pub fn parse_refresh(token: &str) -> Option<(i64, &str)> {
    if token.len() > MAX_REFRESH_LEN {
        return None;
    }
    let rest = token.strip_prefix(REFRESH_PREFIX)?;
    let (id, secret) = rest.split_once('_')?;
    let canonical = !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) && !(id.len() > 1 && id.starts_with('0'));
    if !canonical || secret.len() != SECRET_LEN || !secret.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return None;
    }
    Some((id.parse().ok()?, secret))
}

/// Signs an access token for `family` and creates its next refresh token.
async fn next_pair(
    auth: &AuthState,
    txn: &DatabaseTransaction,
    family: &family::Model,
    now: DateTime<Utc>,
) -> AppResult<Issued> {
    let config = &auth.config.jwt;
    let secret = random_string(SECRET_LEN);
    let expires_at = (now + Duration::days(config.refresh_ttl_days as i64)).min(family.expires_at);
    let token = refresh::ActiveModel {
        family_id: Set(family.id.clone()),
        token_hash: Set(hmac_sha256_hex(&auth.refresh_key, &secret)),
        created_at: Set(now),
        expires_at: Set(expires_at),
        used_at: Set(None),
        ..Default::default()
    }
    .insert(txn)
    .await?;
    let abilities = abilities_of(family);
    let ttl = config.access_ttl_secs;
    let claims = Claims {
        sub: family.user_id.to_string(),
        iat: now.timestamp(),
        exp: now.timestamp() + ttl as i64,
        jti: random_string(24),
        ver: family.auth_version,
        fam: family.id.clone(),
        scope: abilities.clone(),
    };
    let access_token = jwt::encode(&auth.jwt_key, &claims);
    // Never hand out a token the guard would refuse (abilities are bounded
    // when signing in; this keeps the two limits consistent).
    if access_token.len() > jwt::MAX_LEN {
        return Err(rustcms_core::ValidationErrors::field("abilities", "too long for an access token").into());
    }
    Ok(Issued {
        access_token,
        expires_in: ttl,
        refresh_token: format!("{REFRESH_PREFIX}{}_{secret}", token.id),
        refresh_expires_at: expires_at,
        abilities,
    })
}

/// Starts a family for the user verified as `snapshot` (its credentials must
/// not have changed since: the same check as for API tokens).
pub async fn issue(
    auth: &AuthState,
    db: &DatabaseConnection,
    snapshot: &user::Model,
    abilities: &[String],
    request: &RequestMeta,
) -> AppResult<Issued> {
    let txn = rustcms_core::db::begin_write(db).await?;
    let user = crate::credentials::lock_unchanged(&txn, snapshot).await?;
    let now = Utc::now();
    let abilities = if abilities.is_empty() { vec!["*".to_owned()] } else { abilities.to_vec() };
    let family = family::ActiveModel {
        id: Set(random_string(32)),
        user_id: Set(user.id),
        abilities: Set(serde_json::to_string(&abilities).map_err(AppError::internal)?),
        auth_version: Set(user.auth_version),
        created_at: Set(now),
        expires_at: Set(now + Duration::days(auth.config.jwt.refresh_family_days as i64)),
        revoked_at: Set(None),
    }
    .insert(&txn)
    .await?;
    let issued = next_pair(auth, &txn, &family, now).await?;
    txn.commit().await?;
    audit(db, request, &family, "jwt.signed_in").await;
    Ok(issued)
}

/// Records a JWT event of `family`, acted by its user with that sign-in, once
/// the change committed: best effort, like every sign-in event (ADR 0014), so
/// a failing log never undoes a sign-out or a revocation.
async fn audit(db: &DatabaseConnection, request: &RequestMeta, family: &family::Model, event: &str) {
    let actor = AuditActor::signed_in(family.user_id, crate::API_GUARD, Some(Credential::Jwt(family.id.clone())));
    let context = AuditContext::new(actor, request.clone());
    rustcms_core::audit::record_detached(db, &context, AuditEntry::new(event, "user").key(family.user_id)).await;
}

/// The token row of `plain` if its secret matches (constant time), and its family.
async fn verify_secret(
    auth: &AuthState,
    db: &impl ConnectionTrait,
    plain: &str,
) -> AppResult<(refresh::Model, family::Model)> {
    let (id, secret) = parse_refresh(plain).ok_or(AppError::Unauthenticated)?;
    let token = refresh::Entity::find_by_id(id).one(db).await?.ok_or(AppError::Unauthenticated)?;
    if !ct_eq(&token.token_hash, &hmac_sha256_hex(&auth.refresh_key, secret)) {
        return Err(AppError::Unauthenticated);
    }
    let family = family::Entity::find_by_id(token.family_id.clone()).one(db).await?.ok_or(AppError::Unauthenticated)?;
    Ok((token, family))
}

/// Locks user, family and token in that order and re-reads them.
async fn lock_all(
    txn: &DatabaseTransaction,
    token_id: i64,
    family_id: &str,
    user_id: i64,
) -> AppResult<(user::Model, family::Model, refresh::Model)> {
    let user = locked(user::Entity::find_by_id(user_id), txn).one(txn).await?.ok_or(AppError::Unauthenticated)?;
    let family = locked(family::Entity::find_by_id(family_id.to_owned()), txn)
        .one(txn)
        .await?
        .ok_or(AppError::Unauthenticated)?;
    let token = locked(refresh::Entity::find_by_id(token_id), txn).one(txn).await?.ok_or(AppError::Unauthenticated)?;
    Ok((user, family, token))
}

/// Whether `family` may still be used by `user` at `now`.
fn live(family: &family::Model, user: &user::Model, now: DateTime<Utc>) -> bool {
    family.revoked_at.is_none()
        && family.expires_at > now
        && family.user_id == user.id
        && user.is_active
        && user.auth_version == family.auth_version
}

/// Ends a family; whether it was still live.
async fn revoke_family(txn: &DatabaseTransaction, family_id: &str, now: DateTime<Utc>) -> AppResult<bool> {
    let result = family::Entity::update_many()
        .col_expr(family::Column::RevokedAt, Expr::value(now))
        .filter(family::Column::Id.eq(family_id))
        .filter(family::Column::RevokedAt.is_null())
        .exec(txn)
        .await?;
    Ok(result.rows_affected > 0)
}

/// The family of a valid refresh token, after its secret was checked (for
/// limits keyed by family).
pub async fn family_of(auth: &AuthState, db: &DatabaseConnection, plain: &str) -> AppResult<String> {
    Ok(verify_secret(auth, db, plain).await?.1.id)
}

/// Exchanges a refresh token for a new pair. A token used before (a replay,
/// or a client retrying after a lost response) ends its family: 401.
pub async fn rotate(
    auth: &AuthState,
    db: &DatabaseConnection,
    plain: &str,
    request: &RequestMeta,
) -> AppResult<Issued> {
    let (token, family) = verify_secret(auth, db, plain).await?;
    let txn = rustcms_core::db::begin_write(db).await?;
    let (user, family, token) = lock_all(&txn, token.id, &family.id, family.user_id).await?;
    let now = Utc::now();
    if token.used_at.is_some() {
        // Theft detection: whoever holds the newer token loses it too. Recorded
        // as a guest: the replay may come from someone else than the user.
        // The revocation commits first: the log never holds it back. Recorded
        // once, when it ended a live family (later replays change nothing).
        let ended = revoke_family(&txn, &family.id, now).await?;
        txn.commit().await?;
        if ended {
            let entry = AuditEntry::new("jwt.reuse_detected", "user")
                .key(family.user_id)
                .meta(FamilyMeta { family: &family.id });
            let guest = AuditContext::new(AuditActor::guest(), request.clone());
            rustcms_core::audit::record_detached(db, &guest, entry).await;
        }
        return Err(AppError::Unauthenticated);
    }
    if !live(&family, &user, now) || token.expires_at <= now {
        return Err(AppError::Unauthenticated);
    }
    let mut used: refresh::ActiveModel = token.into();
    used.used_at = Set(Some(now));
    used.update(&txn).await?;
    let issued = next_pair(auth, &txn, &family, now).await?;
    txn.commit().await?;
    Ok(issued)
}

/// Ends the family of a refresh token (any of its tokens, used or not).
pub async fn logout(auth: &AuthState, db: &DatabaseConnection, plain: &str, request: &RequestMeta) -> AppResult<()> {
    let (token, family) = verify_secret(auth, db, plain).await?;
    let txn = rustcms_core::db::begin_write(db).await?;
    let (_, family, _) = lock_all(&txn, token.id, &family.id, family.user_id).await?;
    let ended = revoke_family(&txn, &family.id, Utc::now()).await?;
    txn.commit().await?;
    if ended {
        audit(db, request, &family, "jwt.signed_out").await;
    }
    Ok(())
}

/// Checks an access token on a request: signature, expiry, and that its user
/// and family are still what they were (active user, same `auth_version`,
/// family not revoked or expired). Abilities come from the family row.
pub async fn verify_access(auth: &AuthState, db: &DatabaseConnection, token: &str) -> AppResult<Verified> {
    let now = Utc::now();
    let claims = jwt::decode(&auth.jwt_key, token, now.timestamp()).map_err(|_| AppError::Unauthenticated)?;
    let user_id: i64 = claims.sub.parse().map_err(|_| AppError::Unauthenticated)?;
    if user_id.to_string() != claims.sub
        || claims.exp <= claims.iat
        || claims.scope.len() > 100
        || claims.scope.iter().any(|s| s.is_empty() || s.len() > 255)
    {
        return Err(AppError::Unauthenticated);
    }
    let family = family::Entity::find_by_id(claims.fam.clone()).one(db).await?.ok_or(AppError::Unauthenticated)?;
    let user = user::Entity::find_by_id(user_id).one(db).await?.ok_or(AppError::Unauthenticated)?;
    if !live(&family, &user, now) || claims.ver != family.auth_version {
        return Err(AppError::Unauthenticated);
    }
    let abilities = abilities_of(&family).into_iter().collect();
    Ok(Verified { user, abilities, family: family.id })
}

/// Revokes every live family of `user_id` (inside the transaction that
/// changes the account; the user row is locked first by the caller).
pub async fn revoke_user<C: ConnectionTrait>(db: &C, user_id: i64) -> AppResult<()> {
    family::Entity::update_many()
        .col_expr(family::Column::RevokedAt, Expr::value(Utc::now()))
        .filter(family::Column::UserId.eq(user_id))
        .filter(family::Column::RevokedAt.is_null())
        .exec(db)
        .await?;
    Ok(())
}

/// Deletes ended families (expired or revoked) with their tokens, in batches.
pub async fn prune(db: &DatabaseConnection) -> AppResult<u64> {
    let mut total = 0;
    loop {
        let now = Utc::now();
        let ended: Vec<String> = family::Entity::find()
            .select_only()
            .column(family::Column::Id)
            .filter(
                sea_orm::Condition::any()
                    .add(family::Column::ExpiresAt.lte(now))
                    .add(family::Column::RevokedAt.is_not_null()),
            )
            .limit(PRUNE_BATCH)
            .into_tuple()
            .all(db)
            .await?;
        if ended.is_empty() {
            return Ok(total);
        }
        refresh::Entity::delete_many().filter(refresh::Column::FamilyId.is_in(ended.clone())).exec(db).await?;
        total += family::Entity::delete_many().filter(family::Column::Id.is_in(ended)).exec(db).await?.rows_affected;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_tokens_are_parsed_strictly() {
        let secret = "a".repeat(SECRET_LEN);
        assert_eq!(parse_refresh(&format!("rcmsr_12_{secret}")), Some((12, secret.as_str())));
        for bad in [
            format!("rcmsr_012_{secret}"),
            format!("rcmsr_+12_{secret}"),
            format!("rcmsr__{secret}"),
            format!("rcmsr_12_{}", "a".repeat(SECRET_LEN - 1)),
            format!("rcmsr_12_{}!", "a".repeat(SECRET_LEN - 1)),
            format!("rcms_12_{secret}"),
            format!("rcmsr_99999999999999999999_{secret}"),
        ] {
            assert_eq!(parse_refresh(&bad), None, "{bad}");
        }
    }
}

/// The sign-in an event is about.
#[derive(serde::Serialize)]
struct FamilyMeta<'a> {
    family: &'a str,
}
