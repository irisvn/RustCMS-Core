//! Idempotency keys for resource creates and actions (`Idempotency-Key`).
//!
//! The key is claimed inside the write's own transaction, before hooks run,
//! and its result is stored before commit: the claim and the write commit
//! together or not at all. A retry with the same key and request finds the
//! committed receipt and replays the result, re-rendered for the user now.

use chrono::{Duration, Utc};
use rustcms_core::{AppError, AppResult, Principal, ValidationErrors};
use sea_orm::sea_query::{Expr, OnConflict};
use sea_orm::{
    ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseTransaction, DbBackend, DbErr, EntityTrait, PaginatorTrait,
    QueryFilter,
};
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

/// Longest wait for a concurrent claim of the same owner.
const CLAIM_WAIT: std::time::Duration = std::time::Duration::from_secs(5);
/// Time kept before the request deadline to answer.
const CLAIM_MARGIN: std::time::Duration = std::time::Duration::from_secs(1);

/// Bounds how long the claim in `txn` waits for a concurrent claimer, within
/// the request's deadline (then 409 `write_conflict`). Called before
/// [`acquire`], which restores the default wait on MySQL.
pub(crate) async fn bound_claim_wait(txn: &DatabaseTransaction) -> AppResult<()> {
    let wait = rustcms_core::http::request_deadline()
        .map_or(CLAIM_WAIT, |deadline| {
            deadline.saturating_duration_since(tokio::time::Instant::now()).saturating_sub(CLAIM_MARGIN)
        })
        .clamp(std::time::Duration::from_millis(100), CLAIM_WAIT);
    match txn.get_database_backend() {
        DbBackend::Postgres => {
            txn.execute_unprepared(&format!("SET LOCAL lock_timeout = '{}ms'", wait.as_millis())).await?;
        }
        // Whole seconds only: rounded down, at least one.
        DbBackend::MySql => rustcms_core::db::set_lock_wait(txn, wait.as_secs()).await?,
        _ => {}
    }
    Ok(())
}

/// Claims `claim` in the write transaction `txn`. `true`: acquired (run the
/// write, then [`complete`]). `false`: a committed receipt exists (a
/// concurrent claimer of the same owner is waited for first).
///
/// The owner's lock is the transaction's first lock: the receipt is read,
/// an expired one replaced and the quota counted with no other claim of the
/// owner in between (so a racing retry replays instead of hitting the quota,
/// and two requests never both replace an expired receipt).
pub(crate) async fn acquire(txn: &DatabaseTransaction, claim: &Claim) -> AppResult<bool> {
    let acquired = claim_in(txn, claim).await;
    // The claim wait (see `bound_claim_wait`) ends here on MySQL, whatever happened.
    rustcms_core::db::set_lock_wait(txn, rustcms_core::db::MYSQL_LOCK_WAIT_SECONDS).await.map_err(conflict)?;
    acquired
}

async fn claim_in(txn: &DatabaseTransaction, claim: &Claim) -> AppResult<bool> {
    rustcms_core::db::xact_lock(txn, &rustcms_core::db::lock_slot("idem", &claim.owner)).await.map_err(conflict)?;
    // An existing claim is resolved first (a retry is never refused by the
    // quota); an expired one is replaced as if it never existed.
    if let Some(existing) = entity::Entity::find_by_id(claim.id.clone()).one(txn).await? {
        if existing.expires_at > Utc::now() {
            return Ok(false);
        }
        entity::Entity::delete_many()
            .filter(entity::Column::Id.eq(&claim.id))
            .filter(entity::Column::ExpiresAt.lte(Utc::now()))
            .exec(txn)
            .await?;
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
    if txn.get_database_backend() == sea_orm::DbBackend::MySql {
        // No `ON CONFLICT DO NOTHING` on MySQL, and its upsert reports a row either
        // way. A duplicate key waits for a concurrent claimer like the upsert does,
        // and does not abort the transaction.
        return match entity::Entity::insert(row).exec_without_returning(txn).await {
            Ok(_) => Ok(true),
            Err(err) if matches!(err.sql_err(), Some(sea_orm::SqlErr::UniqueConstraintViolation(_))) => Ok(false),
            Err(err) => Err(conflict(err)),
        };
    }
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

/// The committed result for `claim`, if any (outside a claim: e.g. before
/// storing an upload). The same key with another request is a 422.
pub(crate) async fn existing<C: ConnectionTrait>(db: &C, claim: &Claim) -> AppResult<Option<Value>> {
    match entity::Entity::find_by_id(claim.id.clone()).one(db).await? {
        Some(row) if row.expires_at > Utc::now() && row.result.is_some() => receipt(db, claim).await.map(Some),
        _ => Ok(None),
    }
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
        if db.get_database_backend() == sea_orm::DbBackend::MySql {
            // MySQL refuses `LIMIT` in an `IN` subquery on the deleted table.
            let statement = Query::delete()
                .from_table(entity::Entity)
                .and_where(entity::Column::ExpiresAt.lte(Utc::now()))
                .order_by(entity::Column::ExpiresAt, sea_orm::sea_query::Order::Asc)
                .limit(PRUNE_BATCH)
                .to_owned();
            let deleted = db.execute(&statement).await?.rows_affected();
            total += deleted;
            if deleted < PRUNE_BATCH {
                return Ok(total);
            }
            continue;
        }
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
