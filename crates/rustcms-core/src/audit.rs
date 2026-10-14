//! The audit log: who changed what, recorded in the transaction of the
//! change (ADR 0014).
//!
//! - [`record`] takes the write's own [`DatabaseTransaction`]: an entry commits
//!   with the change, and disappears with it on rollback.
//! - [`record_detached`] is for events that are not data changes (sign-ins):
//!   best effort, a failure is logged and never fails the request.
//! - Values are bounded ([`bounded`]): an entry never makes a valid write fail.

use std::collections::BTreeMap;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use chrono::{DateTime, Utc};
use sea_orm::{
    ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseConnection, DatabaseTransaction, EntityTrait, QueryFilter,
    QueryOrder, QuerySelect,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::models::audit_log::{self, Entity as AuditLog};
use crate::principal::Credential;
use crate::{AppResult, Principal};

/// What replaces a value that must not be stored.
pub const REDACTED: &str = "[redacted]";

/// Longest text kept whole; longer text is summarized.
pub const MAX_TEXT: usize = 1_000;
/// Deepest nesting kept; deeper values are summarized.
pub const MAX_DEPTH: usize = 8;
/// Most items kept per list or object.
pub const MAX_ITEMS: usize = 100;
/// Largest `changes` or `meta` of one entry, in bytes of JSON.
pub const MAX_BYTES: usize = 64 * 1024;

/// Longest retention accepted (100 years).
pub const MAX_RETENTION_DAYS: u32 = 36_500;

/// `[audit]` settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AuditConfig {
    /// Entries older than this are deleted (hourly, and by `audit:prune`).
    pub retention_days: u32,
    /// Failed sign-ins recorded one by one per minute (per instance); beyond,
    /// one summary entry per minute counts them.
    pub sign_in_failures_per_minute: u32,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self { retention_days: 365, sign_in_failures_per_minute: 600 }
    }
}

impl AuditConfig {
    /// The `[audit]` section, validated.
    pub fn of(config: &crate::Config) -> anyhow::Result<Self> {
        config.section::<Self>("audit")?.validated()
    }

    fn validated(self) -> anyhow::Result<Self> {
        anyhow::ensure!(
            (1..=MAX_RETENTION_DAYS).contains(&self.retention_days),
            "audit.retention_days must be between 1 and {MAX_RETENTION_DAYS}"
        );
        Ok(self)
    }
}

/// Facts about the request behind an entry (empty for console commands).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequestMeta {
    /// Client address (after the trusted-proxy rules).
    pub ip: Option<String>,
    /// `User-Agent`, cut to 255 characters.
    pub user_agent: Option<String>,
    /// `x-request-id`.
    pub request_id: Option<String>,
}

impl RequestMeta {
    /// The metadata of a request.
    pub fn from_parts(parts: &Parts) -> Self {
        let ip = parts.extensions.get::<crate::ClientIp>().map(|c| c.0.to_string());
        let user_agent =
            parts.headers.get(axum::http::header::USER_AGENT).and_then(|v| v.to_str().ok()).map(|ua| cut(ua, 255));
        let request_id = parts.headers.get(crate::http::REQUEST_ID).and_then(|v| v.to_str().ok()).map(|id| cut(id, 64));
        Self { ip, user_agent, request_id }
    }
}

impl<S: Send + Sync> FromRequestParts<S> for RequestMeta {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        Ok(Self::from_parts(parts))
    }
}

/// Kind of actor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorKind {
    /// A signed-in user.
    User,
    /// A console command or the application itself.
    System,
    /// Nobody signed in (failed sign-ins).
    Guest,
}

impl ActorKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::System => "system",
            Self::Guest => "guest",
        }
    }
}

/// Who acts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditActor {
    /// User, system or guest.
    pub kind: ActorKind,
    /// The user, if any.
    pub user_id: Option<i64>,
    /// The guard the user acts in.
    pub guard: Option<String>,
    /// `session`, `token:<id>`, `jwt:<family>` or `console:<command>`.
    pub credential: Option<String>,
}

impl AuditActor {
    /// The user behind a principal, with the credential of the request.
    pub fn user(principal: &Principal) -> Self {
        Self {
            kind: ActorKind::User,
            user_id: principal.id.parse().ok(),
            guard: Some(principal.guard.clone()).filter(|g| !g.is_empty()),
            credential: principal.credential.as_ref().map(Credential::label),
        }
    }

    /// A user who just proved their credentials (sign-in, token issuance).
    pub fn signed_in(user_id: i64, guard: &str, credential: Option<Credential>) -> Self {
        Self {
            kind: ActorKind::User,
            user_id: Some(user_id),
            guard: Some(guard.to_owned()),
            credential: credential.as_ref().map(Credential::label),
        }
    }

    /// A console command (`console:<command>`), or the application (`None`).
    pub fn system(command: Option<&str>) -> Self {
        Self {
            kind: ActorKind::System,
            user_id: None,
            guard: None,
            credential: command.map(|c| format!("console:{}", cut(c, 80))),
        }
    }

    /// Nobody signed in.
    pub fn guest() -> Self {
        Self { kind: ActorKind::Guest, user_id: None, guard: None, credential: None }
    }
}

/// The actor and request of the entries of one operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditContext {
    /// Who acts.
    pub actor: AuditActor,
    /// From where.
    pub request: RequestMeta,
}

impl AuditContext {
    /// An actor in a request.
    pub fn new(actor: AuditActor, request: RequestMeta) -> Self {
        Self { actor, request }
    }

    /// The application, or a console command, outside any request.
    pub fn system(command: Option<&str>) -> Self {
        Self { actor: AuditActor::system(command), request: RequestMeta::default() }
    }
}

/// One event to record.
#[derive(Debug, Clone, PartialEq)]
pub struct AuditEntry {
    event: String,
    subject_type: String,
    subject_key: Option<String>,
    changes: Option<Value>,
    meta: Option<Value>,
}

impl AuditEntry {
    /// `event` (`created`, `user.roles_changed`, …) on a subject type (a
    /// resource slug, `user`, `role`, …).
    pub fn new(event: impl Into<String>, subject_type: impl Into<String>) -> Self {
        Self { event: event.into(), subject_type: subject_type.into(), subject_key: None, changes: None, meta: None }
    }

    /// The subject's key.
    pub fn key(mut self, key: impl ToString) -> Self {
        self.subject_key = Some(key.to_string());
        self
    }

    /// What changed: `{field: [old, new]}` (see [`diff`]); bounded when recorded.
    pub fn changes(mut self, changes: Value) -> Self {
        self.changes = Some(changes);
        self
    }

    /// Facts specific to the event (a typed struct: never request input as is).
    pub fn meta(mut self, meta: impl Serialize) -> Self {
        self.meta = serde_json::to_value(meta).ok();
        self
    }

    /// The event name.
    pub fn event(&self) -> &str {
        &self.event
    }

    fn into_model(self, ctx: &AuditContext, at: DateTime<Utc>) -> audit_log::ActiveModel {
        // One budget for the whole entry: meta takes at most a quarter.
        let meta = self.meta.map(|m| bounded_to(m, MAX_BYTES / 4));
        let room = MAX_BYTES - meta.as_ref().map_or(0, size);
        let changes = self.changes.map(|c| bounded_to(c, room));
        audit_log::ActiveModel {
            id: Set(uuid::Uuid::now_v7()),
            at: Set(at),
            event: Set(identifier(&self.event, 64)),
            subject_type: Set(identifier(&self.subject_type, 64)),
            subject_key: Set(self.subject_key.map(|k| identifier(&k, 255))),
            actor_kind: Set(ctx.actor.kind.as_str().to_owned()),
            actor_user_id: Set(ctx.actor.user_id),
            actor_guard: Set(ctx.actor.guard.as_deref().map(|g| cut(g, 32))),
            credential: Set(ctx.actor.credential.as_deref().map(|c| cut(c, 96))),
            ip: Set(ctx.request.ip.as_deref().map(|i| cut(i, 45))),
            user_agent: Set(ctx.request.user_agent.as_deref().map(|u| cut(u, 255))),
            request_id: Set(ctx.request.request_id.as_deref().map(|r| cut(r, 64))),
            changes: Set(changes),
            meta: Set(meta),
        }
    }
}

/// Records `entry` in the transaction of the change it describes.
pub async fn record(txn: &DatabaseTransaction, ctx: &AuditContext, entry: AuditEntry) -> AppResult<()> {
    record_all(txn, ctx, vec![entry]).await
}

/// Records `entries` in the transaction of the change, in batches.
pub async fn record_all(txn: &DatabaseTransaction, ctx: &AuditContext, entries: Vec<AuditEntry>) -> AppResult<()> {
    let at = Utc::now();
    let mut rows = entries.into_iter().map(|e| e.into_model(ctx, at)).peekable();
    while rows.peek().is_some() {
        let batch: Vec<_> = rows.by_ref().take(50).collect();
        AuditLog::insert_many(batch).exec_without_returning(txn).await?;
    }
    Ok(())
}

/// Records an event that is not a data change (a sign-in, a sign-out): best
/// effort, on its own; a failure is logged and never fails the caller.
pub async fn record_detached<C: ConnectionTrait>(db: &C, ctx: &AuditContext, entry: AuditEntry) {
    let event = entry.event.clone();
    if let Err(err) = AuditLog::insert(entry.into_model(ctx, Utc::now())).exec_without_returning(db).await {
        tracing::warn!(%err, event, "audit entry not recorded");
    }
}

/// `{field: [old, new]}` for every field whose value differs between `before`
/// and `after` (missing = `null`); fields `redact` names keep `"[redacted]"`.
pub fn diff(
    before: Option<&Map<String, Value>>,
    after: Option<&Map<String, Value>>,
    redact: &dyn Fn(&str) -> bool,
) -> Value {
    let empty = Map::new();
    let (before, after) = (before.unwrap_or(&empty), after.unwrap_or(&empty));
    let mut names: Vec<&String> = before.keys().chain(after.keys()).collect();
    names.sort();
    names.dedup();
    let mut out = Map::new();
    for name in names {
        let (old, new) = (before.get(name).unwrap_or(&Value::Null), after.get(name).unwrap_or(&Value::Null));
        if old == new {
            continue;
        }
        let pair = if redact(name) {
            let shown = |v: &Value| if v.is_null() { Value::Null } else { json!(REDACTED) };
            json!([shown(old), shown(new)])
        } else {
            json!([old, new])
        };
        out.insert(name.clone(), pair);
    }
    Value::Object(out)
}

/// `value` within the limits: long text summarized (`{truncated, length,
/// sha256}`), deep values and long lists cut, and an object over
/// [`MAX_BYTES`] replaced by `{overflow: true, fields: [...]}`.
pub fn bounded(value: Value) -> Value {
    bounded_to(value, MAX_BYTES)
}

/// [`bounded`] within `limit` bytes.
fn bounded_to(value: Value, limit: usize) -> Value {
    let value = shrink(value, 0);
    if size(&value) <= limit {
        return value;
    }
    let fields: Vec<Value> = match &value {
        Value::Object(map) => map.keys().take(MAX_ITEMS).map(|k| json!(cut(k, 64))).collect(),
        _ => Vec::new(),
    };
    json!({ "overflow": true, "fields": fields })
}

fn shrink(value: Value, depth: usize) -> Value {
    match value {
        Value::String(text) if text.chars().count() > MAX_TEXT => summary(&text),
        Value::Array(items) if depth >= MAX_DEPTH => json!({ "nested": true, "items": items.len() }),
        Value::Object(map) if depth >= MAX_DEPTH => json!({ "nested": true, "items": map.len() }),
        Value::Array(items) => {
            let total = items.len();
            let mut kept: Vec<Value> = items.into_iter().take(MAX_ITEMS).map(|v| shrink(v, depth + 1)).collect();
            if total > MAX_ITEMS {
                kept.push(json!({ "more": total - MAX_ITEMS }));
            }
            Value::Array(kept)
        }
        Value::Object(map) => {
            let total = map.len();
            let mut kept: Map<String, Value> =
                map.into_iter().take(MAX_ITEMS).map(|(k, v)| (cut(&k, 255), shrink(v, depth + 1))).collect();
            if total > MAX_ITEMS {
                kept.insert("…".into(), json!({ "more": total - MAX_ITEMS }));
            }
            Value::Object(kept)
        }
        other => other,
    }
}

fn size(value: &Value) -> usize {
    serde_json::to_vec(value).map_or(usize::MAX, |b| b.len())
}

/// `text` if it has at most `max` characters; otherwise its start and a
/// digest of the whole, so two long identifiers never record alike.
fn identifier(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let digest = hex::encode(Sha256::digest(text.as_bytes()));
    let hex = (max / 2).min(digest.len());
    format!("{}~{}", cut(text, max - hex - 1), &digest[..hex])
}

/// A long text as its length and digest.
pub fn summary(text: &str) -> Value {
    json!({ "truncated": true, "length": text.chars().count(), "sha256": hex::encode(Sha256::digest(text.as_bytes())) })
}

/// At most `max` characters of `text` (never splits a character).
fn cut(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// Deletes entries older than `cutoff`, oldest first, in batches; stops
/// after `budget`. Safe to run on several instances at once.
pub async fn prune(db: &DatabaseConnection, cutoff: DateTime<Utc>, budget: std::time::Duration) -> AppResult<u64> {
    let started = std::time::Instant::now();
    let mut deleted = 0;
    loop {
        let ids: Vec<uuid::Uuid> = AuditLog::find()
            .select_only()
            .column(audit_log::Column::Id)
            .filter(audit_log::Column::At.lt(cutoff))
            .order_by_asc(audit_log::Column::At)
            .order_by_asc(audit_log::Column::Id)
            .limit(1_000)
            .into_tuple()
            .all(db)
            .await?;
        if ids.is_empty() {
            break;
        }
        let batch = ids.len();
        deleted += AuditLog::delete_many().filter(audit_log::Column::Id.is_in(ids)).exec(db).await?.rows_affected;
        if batch < 1_000 || started.elapsed() >= budget {
            break;
        }
    }
    Ok(deleted)
}

/// Field names and values as an ordered map (for tests and callers that build
/// changes by hand).
pub fn changes_of(pairs: &[(&str, Value, Value)]) -> Value {
    let map: BTreeMap<String, Value> =
        pairs.iter().map(|(name, old, new)| ((*name).to_owned(), json!([old, new]))).collect();
    json!(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diffs_only_what_changed() {
        let before = json!({"title": "A", "body": "x", "secret": "s1"});
        let after = json!({"title": "B", "body": "x", "secret": "s2", "tags": ["a"]});
        let changes = diff(before.as_object(), after.as_object(), &|f| f == "secret");
        assert_eq!(changes, json!({"title": ["A", "B"], "secret": [REDACTED, REDACTED], "tags": [null, ["a"]]}));
        assert_eq!(diff(None, after.as_object(), &|_| false)["title"], json!([null, "B"]));
        assert_eq!(diff(before.as_object(), before.as_object(), &|_| false), json!({}));
    }

    #[test]
    fn bounds_values() {
        let long = "é".repeat(MAX_TEXT + 1);
        let bounded_long = bounded(json!({"body": [null, long]}));
        assert_eq!(bounded_long["body"][1]["truncated"], json!(true));
        assert_eq!(bounded_long["body"][1]["length"], json!(MAX_TEXT + 1));

        let mut deep = json!("leaf");
        for _ in 0..20 {
            deep = json!([deep]);
        }
        assert!(serde_json::to_string(&bounded(deep)).unwrap().contains("nested"));

        let many: Vec<Value> = (0..500).map(|i| json!(i)).collect();
        let cut_list = bounded(json!(many));
        assert_eq!(cut_list.as_array().unwrap().len(), MAX_ITEMS + 1);

        let huge: Map<String, Value> = (0..90).map(|i| (format!("f{i}"), json!("x".repeat(MAX_TEXT)))).collect();
        let over = bounded(Value::Object(huge));
        assert_eq!(over["overflow"], json!(true));
        assert_eq!(over["fields"].as_array().unwrap().len(), 90);
    }

    #[test]
    fn long_identifiers_stay_distinct() {
        let (a, b) = ("k".repeat(300) + "a", "k".repeat(300) + "b");
        let (ia, ib) = (identifier(&a, 255), identifier(&b, 255));
        assert_ne!(ia, ib);
        assert_eq!((ia.chars().count(), identifier("short", 255)), (255, "short".to_owned()));
        assert_eq!(identifier(&"é".repeat(100), 64).chars().count(), 64);
    }

    #[test]
    fn one_budget_per_entry() {
        let big = |n: usize| -> Value {
            let map: Map<String, Value> = (0..n).map(|i| (format!("f{i}"), json!("x".repeat(MAX_TEXT)))).collect();
            Value::Object(map)
        };
        let ctx = AuditContext::new(
            AuditActor::guest(),
            RequestMeta { user_agent: Some("u".repeat(400)), request_id: Some("r".repeat(100)), ip: None },
        );
        let model = AuditEntry::new("e", "s").changes(big(60)).meta(big(60)).into_model(&ctx, Utc::now());
        let total = |v: &sea_orm::ActiveValue<Option<Value>>| v.clone().unwrap().as_ref().map_or(0, size);
        assert!(total(&model.changes) + total(&model.meta) <= MAX_BYTES);
        assert_eq!(model.meta.clone().unwrap().unwrap()["overflow"], json!(true));
        assert_eq!(model.user_agent.unwrap().map(|u| u.len()), Some(255));
        assert_eq!(model.request_id.unwrap().map(|r| r.len()), Some(64));
    }

    #[test]
    fn retention_is_bounded() {
        let config = |retention_days: u32| AuditConfig { retention_days, ..Default::default() }.validated();
        assert!(config(0).is_err());
        assert!(config(u32::MAX).is_err());
        assert_eq!(config(MAX_RETENTION_DAYS).unwrap().retention_days, MAX_RETENTION_DAYS);
    }

    #[test]
    fn cuts_on_characters() {
        assert_eq!(cut("ééé", 2), "éé");
        assert_eq!(AuditActor::system(Some("token:create")).credential.as_deref(), Some("console:token:create"));
    }
}
