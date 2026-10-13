//! Idempotency keys for resource creates and actions (`Idempotency-Key`).
//!
//! The key is claimed inside the write's own transaction, before hooks run,
//! and its result is stored before commit: the claim and the write commit
//! together or not at all. A retry with the same key and request finds the
//! committed receipt and replays the result, re-rendered for the user now.

use chrono::{Duration, Utc};
use rustcms_core::{AppError, AppResult, Principal, ValidationErrors};
use sea_orm::sea_query::{Expr, OnConflict};
use sea_orm::{ActiveValue::Set, ColumnTrait, ConnectionTrait, DbErr, EntityTrait, PaginatorTrait, QueryFilter};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Request header carrying the client's key.
pub const HEADER: &str = "idempotency-key";
/// How long a receipt is kept (a retry after that is a new request).
pub const TTL_HOURS: i64 = 24;
/// Most live keys per user: further keyed requests answer 429.
pub const MAX_LIVE_KEYS: u64 = 1_000;
const MAX_KEY_LEN: usize = 128;
const VERSION: &str = "v1";

pub(crate) mod entity {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "idempotency_keys")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub owner: String,
        pub request_hash: String,
        pub result: Option<String>,
        pub created_at: DateTimeUtc,
        pub expires_at: DateTimeUtc,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// A validated `Idempotency-Key` value: 1 to 128 of `A-Z a-z 0-9 _ -`.
pub fn parse(value: &str) -> AppResult<String> {
    let valid = !value.is_empty()
        && value.len() <= MAX_KEY_LEN
        && value.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    if valid {
        Ok(value.to_owned())
    } else {
        Err(ValidationErrors::field("idempotency_key", "must be 1 to 128 letters, digits, `_` or `-`").into())
    }
}

/// One keyed request: who, what, and a hash of the request itself.
#[derive(Debug, Clone)]
pub(crate) struct Claim {
    id: String,
    owner: String,
    request_hash: String,
}

impl Claim {
    /// `operation` names the endpoint (`create`, `action:publish`); `request`
    /// is everything that decides the write.
    pub(crate) fn new(principal: &Principal, slug: &str, operation: &str, key: &str, request: &Value) -> Self {
        let owner = format!("{}:{}", principal.guard, principal.id);
        Self {
            id: digest(&[VERSION, &principal.guard, &principal.id, slug, operation, key]),
            owner,
            request_hash: digest(&[VERSION, &canonical(request)]),
        }
    }
}

/// SHA-256 of length-prefixed parts (no two tuples share an encoding).
fn digest(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part.as_bytes());
    }
    hex::encode(hasher.finalize())
}

/// JSON with object keys sorted at every level (stable whatever the input order).
fn canonical(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(&String, &Value)> = map.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            let inner: Vec<String> =
                entries.into_iter().map(|(k, v)| format!("{}:{}", Value::String(k.clone()), canonical(v))).collect();
            format!("{{{}}}", inner.join(","))
        }
        Value::Array(items) => format!("[{}]", items.iter().map(canonical).collect::<Vec<_>>().join(",")),
        other => other.to_string(),
    }
}

/// Lock waits and busy databases: nothing was written, the request may be retried.
pub(crate) fn conflict(err: DbErr) -> AppError {
    err.into()
}

/// Claims `claim` in the write transaction `txn`. `true`: acquired (run the
/// write, then [`complete`]). `false`: a committed receipt exists (PostgreSQL
/// waits for a concurrent claimer to commit or roll back first).
pub(crate) async fn acquire<C: ConnectionTrait>(txn: &C, claim: &Claim) -> AppResult<bool> {
    // An existing claim is resolved first (a retry is never refused by the
    // quota); an expired one is replaced as if it never existed.
    if let Some(existing) = entity::Entity::find_by_id(claim.id.clone()).one(txn).await? {
        if existing.expires_at > Utc::now() {
            return Ok(false);
        }
        entity::Entity::delete_by_id(claim.id.clone()).exec(txn).await?;
    }
    // Quota checks of one user run one at a time (SQLite: the write lock).
    if txn.get_database_backend() == sea_orm::DbBackend::Postgres {
        // A number derived from the user (no input reaches the SQL text).
        let slot = i64::from_str_radix(&digest(&["idempotency-quota", &claim.owner])[..15], 16).unwrap_or_default();
        txn.execute_unprepared(&format!("SELECT pg_advisory_xact_lock({slot})")).await?;
    }
    let live = entity::Entity::find()
        .filter(entity::Column::Owner.eq(&claim.owner))
        .filter(entity::Column::ExpiresAt.gt(Utc::now()))
        .count(txn)
        .await
        .map_err(conflict)?;
    if live >= MAX_LIVE_KEYS {
        return Err(AppError::TooManyRequests { retry_after: 3_600 });
    }
    let now = Utc::now();
    let row = entity::ActiveModel {
        id: Set(claim.id.clone()),
        owner: Set(claim.owner.clone()),
        request_hash: Set(claim.request_hash.clone()),
        result: Set(None),
        created_at: Set(now),
        expires_at: Set(now + Duration::hours(TTL_HOURS)),
    };
    let inserted = entity::Entity::insert(row)
        .on_conflict(OnConflict::column(entity::Column::Id).do_nothing().to_owned())
        .exec_without_returning(txn)
        .await
        .map_err(conflict)?;
    Ok(inserted == 1)
}

/// Stores the result of an acquired claim, in the same transaction as the write.
pub(crate) async fn complete<C: ConnectionTrait>(txn: &C, claim: &Claim, result: &Value) -> AppResult<()> {
    entity::Entity::update_many()
        .col_expr(entity::Column::Result, Expr::value(result.to_string()))
        .filter(entity::Column::Id.eq(&claim.id))
        .exec(txn)
        .await?;
    Ok(())
}

/// The committed result for `claim`. The same key with another request is a 422.
pub(crate) async fn receipt<C: ConnectionTrait>(db: &C, claim: &Claim) -> AppResult<Value> {
    let row = entity::Entity::find_by_id(claim.id.clone()).one(db).await?.ok_or(AppError::Conflict {
        code: "write_conflict",
        message: "The earlier request with this key is no longer known. Reload and check before trying again.",
    })?;
    if row.request_hash != claim.request_hash {
        return Err(ValidationErrors::field(
            "idempotency_key",
            "was already used for a different request; send a new key for a new change",
        )
        .into());
    }
    let result = row.result.ok_or_else(|| AppError::internal("committed idempotency receipt without a result"))?;
    serde_json::from_str(&result).map_err(AppError::internal)
}

/// Expired receipts deleted per statement (short write transactions).
const PRUNE_BATCH: u64 = 500;

/// Deletes expired receipts in batches; returns how many.
pub async fn prune<C: ConnectionTrait>(db: &C) -> AppResult<u64> {
    use sea_orm::sea_query::Query;
    let mut total = 0;
    loop {
        let expired = Query::select()
            .column(entity::Column::Id)
            .from(entity::Entity)
            .and_where(entity::Column::ExpiresAt.lte(Utc::now()))
            .limit(PRUNE_BATCH)
            .to_owned();
        let deleted =
            entity::Entity::delete_many().filter(entity::Column::Id.in_subquery(expired)).exec(db).await?.rows_affected;
        total += deleted;
        if deleted < PRUNE_BATCH {
            return Ok(total);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keys_are_validated() {
        assert!(parse("0199d3a4-1b2c_7d3e").is_ok());
        for bad in ["", "has space", "ü", &"k".repeat(129)] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn requests_hash_canonically() {
        let principal = Principal { id: "7".into(), guard: "admin".into(), ..Default::default() };
        let a = Claim::new(&principal, "posts", "create", "k", &json!({"b": 1, "a": {"y": [1, 2], "x": null}}));
        let b = Claim::new(&principal, "posts", "create", "k", &json!({"a": {"x": null, "y": [1, 2]}, "b": 1}));
        assert_eq!((a.id.clone(), a.request_hash.clone()), (b.id.clone(), b.request_hash.clone()));
        let other = Claim::new(&principal, "posts", "create", "k", &json!({"b": 1, "a": {"y": [2, 1], "x": null}}));
        assert_ne!(a.request_hash, other.request_hash);
        let missing = Claim::new(&principal, "posts", "create", "k", &json!({"b": 1, "a": {"y": [1, 2]}}));
        assert_ne!(a.request_hash, missing.request_hash, "null and absent differ");
        // Who, where and which key all separate claims.
        let someone = Principal { id: "8".into(), ..principal.clone() };
        assert_ne!(a.id, Claim::new(&someone, "posts", "create", "k", &json!({})).id);
        assert_ne!(a.id, Claim::new(&principal, "pages", "create", "k", &json!({})).id);
        assert_ne!(a.id, Claim::new(&principal, "posts", "action:publish", "k", &json!({})).id);
        // Length prefixes: moving a boundary changes the id.
        assert_ne!(digest(&["ab", "c"]), digest(&["a", "bc"]));
    }
}
