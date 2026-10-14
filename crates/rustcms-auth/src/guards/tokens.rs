//! Personal access tokens (`rcms_<id>_<secret>`), like Laravel Sanctum.

use std::collections::HashSet;

use chrono::{DateTime, Duration, Utc};
use rustcms_core::models::user;
use rustcms_core::{AppError, AppResult};
use sea_orm::sea_query::Expr;
use sea_orm::{ActiveModelTrait, ColumnTrait, Condition, ConnectionTrait, EntityTrait, QueryFilter, Set};

use crate::entities::personal_access_token as entity;
use crate::secret::{ct_eq, random_string, sha256_hex};

/// Prefix identifying rustcms tokens (lets secret scanners and the guard recognise them).
pub const PREFIX: &str = "rcms_";
/// Longest token lifetime that can be requested, in days (about ten years).
pub const MAX_LIFETIME_DAYS: u32 = 3650;
const SECRET_LEN: usize = 40;
const LAST_USED_EVERY_SECS: i64 = 60;

/// A freshly issued token. `plain_text` is shown once and never stored.
#[derive(Debug, Clone)]
pub struct IssuedToken {
    /// Stored record.
    pub token: entity::Model,
    /// `rcms_<id>_<secret>`.
    pub plain_text: String,
}

/// Expiry of a token issued now for `days` (1 to [`MAX_LIFETIME_DAYS`]);
/// `None` when `days` is outside that range.
pub fn expiry_in_days(days: u32) -> Option<DateTime<Utc>> {
    if !(1..=MAX_LIFETIME_DAYS).contains(&days) {
        return None;
    }
    Utc::now().checked_add_signed(Duration::days(i64::from(days)))
}

/// Issues a token for `user_id` with `abilities` (`["*"]` = all of the user's permissions).
pub async fn issue<C: ConnectionTrait>(
    db: &C,
    user_id: i64,
    name: &str,
    abilities: &[String],
    expires_at: Option<DateTime<Utc>>,
) -> AppResult<IssuedToken> {
    let secret = random_string(SECRET_LEN);
    let abilities = if abilities.is_empty() { vec!["*".to_owned()] } else { abilities.to_vec() };
    let token = entity::ActiveModel {
        user_id: Set(user_id),
        name: Set(name.to_owned()),
        token_hash: Set(sha256_hex(&secret)),
        abilities: Set(serde_json::to_string(&abilities).map_err(AppError::internal)?),
        expires_at: Set(expires_at),
        ..Default::default()
    }
    .insert(db)
    .await?;
    let plain_text = format!("{PREFIX}{}_{secret}", token.id);
    Ok(IssuedToken { token, plain_text })
}

/// Abilities stored on a token.
pub fn abilities(token: &entity::Model) -> HashSet<String> {
    serde_json::from_str::<Vec<String>>(&token.abilities).unwrap_or_default().into_iter().collect()
}

/// Resolves a bearer value to its token and active owner. Any failure is a 401.
pub async fn authenticate<C: ConnectionTrait>(db: &C, bearer: &str) -> AppResult<(entity::Model, user::Model)> {
    let (id, secret) = parse(bearer).ok_or(AppError::Unauthenticated)?;
    let token = entity::Entity::find_by_id(id).one(db).await?.ok_or(AppError::Unauthenticated)?;
    let valid = ct_eq(&token.token_hash, &sha256_hex(secret))
        && token.revoked_at.is_none()
        && token.expires_at.is_none_or(|at| at > Utc::now());
    if !valid {
        return Err(AppError::Unauthenticated);
    }
    let user = user::Entity::find_by_id(token.user_id)
        .one(db)
        .await?
        .filter(|u| u.is_active)
        .ok_or(AppError::Unauthenticated)?;
    touch(db, token.id).await?;
    Ok((token, user))
}

/// Revokes one token.
pub async fn revoke<C: ConnectionTrait>(db: &C, token_id: i64) -> AppResult<()> {
    entity::Entity::update_many()
        .col_expr(entity::Column::RevokedAt, Expr::value(Utc::now()))
        .filter(entity::Column::Id.eq(token_id))
        .filter(entity::Column::RevokedAt.is_null())
        .exec(db)
        .await?;
    Ok(())
}

/// `rcms_<digits>_<40 alphanumerics>`.
fn parse(bearer: &str) -> Option<(i64, &str)> {
    let (id, secret) = bearer.strip_prefix(PREFIX)?.split_once('_')?;
    let well_formed = !id.is_empty()
        && id.bytes().all(|b| b.is_ascii_digit())
        && secret.len() == SECRET_LEN
        && secret.bytes().all(|b| b.is_ascii_alphanumeric());
    well_formed.then(|| Some((id.parse().ok()?, secret))).flatten()
}

/// Records use at most once a minute.
async fn touch<C: ConnectionTrait>(db: &C, token_id: i64) -> AppResult<()> {
    let now = Utc::now();
    entity::Entity::update_many()
        .col_expr(entity::Column::LastUsedAt, Expr::value(now))
        .filter(entity::Column::Id.eq(token_id))
        .filter(
            Condition::any()
                .add(entity::Column::LastUsedAt.is_null())
                .add(entity::Column::LastUsedAt.lt(now - Duration::seconds(LAST_USED_EVERY_SECS))),
        )
        .exec(db)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_well_formed_tokens() {
        let secret = "a".repeat(40);
        assert_eq!(parse(&format!("rcms_12_{secret}")), Some((12, secret.as_str())));
        assert_eq!(parse(&format!("rcms_x_{secret}")), None);
        assert_eq!(parse(&format!("rcms__{secret}")), None);
        assert_eq!(parse("rcms_12_short"), None);
        assert_eq!(parse(&format!("rcms_12_{}", "!".repeat(40))), None);
        assert_eq!(parse(&format!("12_{secret}")), None);
        assert_eq!(parse(&format!("rcms_99999999999999999999_{secret}")), None, "overflowing id");
    }
}
