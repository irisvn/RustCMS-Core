//! Reading the audit log (ADR 0014): the whole log for auditors
//! (`audit.view_all`), and a record's history for those who may view the
//! record (`audit.view`). Filters run in SQL, so a page never hides entries
//! it skipped: no count, gap or cursor tells what a user may not see.

use std::collections::HashMap;

use axum::Router;
use axum::extract::{Path, RawQuery};
use axum::http::Method;
use axum::routing::get;
use chrono::{DateTime, Utc};
use rustcms_core::models::{audit_log, user};
use rustcms_core::{AppError, AppResult, AppState, Json, ValidationErrors};
use sea_orm::sea_query::Condition;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use serde::Serialize;
use serde_json::{Map, Value};

use crate::context::Ctx;
use crate::http::AdminCtx;

/// See every entry.
pub const VIEW_ALL: &str = "audit.view_all";
/// See sign-in, sign-out and credential entries (with addresses) too.
pub const VIEW_AUTH: &str = "audit.view_auth";
/// See the history of records one may view.
pub const VIEW: &str = "audit.view";

/// Event families about credentials (personal data: emails typed, addresses).
const AUTH_FAMILIES: [&str; 3] = ["auth.%", "jwt.%", "token.%"];
/// Events a record's history shows (what the engine records about records).
const RECORD_EVENTS: [&str; 9] =
    ["created", "updated", "trashed", "deleted", "restored", "force_deleted", "attached", "detached", "pivot_updated"];
/// Events about a record's belongs-to-many links (meta: relation, targets, pivot).
const LINK_EVENTS: [&str; 3] = ["attached", "detached", "pivot_updated"];
const MAX_PER_PAGE: u64 = 100;
const DEFAULT_PER_PAGE: u64 = 50;

/// Every audit route as `(method, path)`.
pub(crate) fn routes(admin_path: &str) -> Vec<(Method, String)> {
    let base = format!("{}/api", admin_path.trim_end_matches('/'));
    vec![
        (Method::GET, format!("{base}/audit")),
        (Method::GET, format!("{base}/resources/{{slug}}/records/{{key}}/history")),
    ]
}

pub(crate) fn router(router: Router<AppState>, admin_path: &str) -> Router<AppState> {
    let base = format!("{}/api", admin_path.trim_end_matches('/'));
    router
        .route(&format!("{base}/audit"), get(log))
        .route(&format!("{base}/resources/{{slug}}/records/{{key}}/history"), get(history))
}

/// Who did it.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct AuditActorView {
    /// `user`, `system` or `guest`.
    pub kind: String,
    /// User id (string).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Name, for viewers who may list users.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// How they were signed in: `session`, `token`, `jwt` or `console`.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential: Option<String>,
}

/// One entry as a viewer may see it.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct AuditEntryView {
    /// Entry id.
    pub id: String,
    /// When (RFC 3339).
    pub at: String,
    /// What happened.
    pub event: String,
    /// What it is about (resource slug, `user`, `role`, `settings`, …).
    pub subject_type: String,
    /// Which one.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject_key: Option<String>,
    /// Who.
    pub actor: AuditActorView,
    /// Client address (`audit.view_auth`).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip: Option<String>,
    /// Client user agent (`audit.view_auth`).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    /// Request id.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// `{field: [old, new]}`.
    #[cfg_attr(feature = "ts", ts(optional, type = "Record<string, JsonValue>"))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changes: Option<Map<String, Value>>,
    /// Facts of the event.
    #[cfg_attr(feature = "ts", ts(optional, type = "Record<string, JsonValue>"))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<Map<String, Value>>,
}

/// A page of entries, newest first.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct AuditPage {
    /// Entries.
    pub data: Vec<AuditEntryView>,
    /// Continuation.
    pub meta: AuditPageMeta,
}

/// Where the next page starts.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct AuditPageMeta {
    /// Pass as `cursor` for older entries; absent on the last page.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// Filters of `GET audit`, parsed after admission.
#[derive(Default)]
struct Filters {
    event: Option<String>,
    subject_type: Option<String>,
    subject_key: Option<String>,
    actor: Option<i64>,
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
    cursor: Option<(DateTime<Utc>, uuid::Uuid)>,
    per_page: u64,
}

fn parse(query: Option<&str>, allowed: &[&str]) -> AppResult<Filters> {
    let mut filters = Filters { per_page: DEFAULT_PER_PAGE, ..Default::default() };
    let mut errors = ValidationErrors::new();
    let mut seen = std::collections::HashSet::new();
    for (name, value) in form_urlencoded::parse(query.unwrap_or_default().as_bytes()) {
        let name = name.into_owned();
        if !allowed.contains(&name.as_str()) {
            errors.add(name.as_str(), "is not allowed");
            continue;
        }
        if !seen.insert(name.clone()) {
            errors.add(name.as_str(), "may only be given once");
            continue;
        }
        let text = |max: usize, errors: &mut ValidationErrors| {
            if value.is_empty() || value.chars().count() > max {
                errors.add(name.as_str(), format!("may not be longer than {max} characters"));
                None
            } else {
                Some(value.to_string())
            }
        };
        let time = |errors: &mut ValidationErrors| match DateTime::parse_from_rfc3339(&value) {
            Ok(t) => Some(t.with_timezone(&Utc)),
            Err(_) => {
                errors.add(name.as_str(), "must be an RFC 3339 timestamp");
                None
            }
        };
        match name.as_str() {
            "event" => filters.event = text(64, &mut errors),
            "subject_type" => filters.subject_type = text(64, &mut errors),
            "subject_key" => filters.subject_key = text(1_000, &mut errors),
            "actor" => match value.parse::<i64>() {
                Ok(id) => filters.actor = Some(id),
                Err(_) => errors.add(name.as_str(), "must be an integer string"),
            },
            "from" => filters.from = time(&mut errors),
            "to" => filters.to = time(&mut errors),
            "per_page" => match value.parse::<u64>() {
                Ok(n) if (1..=MAX_PER_PAGE).contains(&n) => filters.per_page = n,
                _ => errors.add(name.as_str(), format!("must be a number from 1 to {MAX_PER_PAGE}")),
            },
            "cursor" => match decode_cursor(&value) {
                Some(cursor) => filters.cursor = Some(cursor),
                None => errors.add(name.as_str(), "is invalid"),
            },
            _ => {}
        }
    }
    if filters.subject_key.is_some() && filters.subject_type.is_none() {
        errors.add("subject_key", "is not allowed");
    }
    errors.into_result()?;
    Ok(filters)
}

fn encode_cursor(entry: &audit_log::Model) -> String {
    // The exact stored instant (SQLite keeps nanoseconds of older rows).
    hex::encode(format!("{}|{}", entry.at.timestamp_nanos_opt().unwrap_or_default(), entry.id))
}

fn decode_cursor(raw: &str) -> Option<(DateTime<Utc>, uuid::Uuid)> {
    if raw.len() > 128 {
        return None;
    }
    let text = String::from_utf8(hex::decode(raw).ok()?).ok()?;
    let (nanos, id) = text.split_once('|')?;
    Some((DateTime::from_timestamp_nanos(nanos.parse().ok()?), id.parse().ok()?))
}

/// A page of `select`, newest first, after `cursor`.
async fn page(
    ctx: &Ctx,
    mut select: sea_orm::Select<audit_log::Entity>,
    cursor: Option<(DateTime<Utc>, uuid::Uuid)>,
    per_page: u64,
) -> AppResult<(Vec<audit_log::Model>, Option<String>)> {
    if let Some((at, id)) = cursor {
        select = select.filter(
            Condition::any()
                .add(audit_log::Column::At.lt(at))
                .add(Condition::all().add(audit_log::Column::At.eq(at)).add(audit_log::Column::Id.lt(id))),
        );
    }
    let mut rows = select
        .order_by_desc(audit_log::Column::At)
        .order_by_desc(audit_log::Column::Id)
        .limit(per_page + 1)
        .all(&ctx.db)
        .await?;
    let next = (rows.len() as u64 > per_page).then(|| {
        rows.truncate(per_page as usize);
        rows.last().map(encode_cursor)
    });
    Ok((rows, next.flatten()))
}

/// Names of `ids` for viewers who may list users.
async fn names(ctx: &Ctx, rows: &[audit_log::Model]) -> AppResult<HashMap<i64, String>> {
    if !ctx.can("users.view_any") {
        return Ok(HashMap::new());
    }
    let mut ids: Vec<i64> = rows.iter().filter_map(|r| r.actor_user_id).collect();
    ids.sort_unstable();
    ids.dedup();
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let users = user::Entity::find().filter(user::Column::Id.is_in(ids)).all(&ctx.db).await?;
    Ok(users.into_iter().map(|u| (u.id, u.name)).collect())
}

/// `session`, `token`, `jwt` or `console`: never the token or sign-in id.
fn credential_kind(label: Option<&str>) -> Option<String> {
    label.map(|l| l.split(':').next().unwrap_or(l).to_owned())
}

fn view(row: audit_log::Model, names: &HashMap<i64, String>, addresses: bool) -> AuditEntryView {
    let object = |value: Option<Value>| match value {
        Some(Value::Object(map)) => Some(map),
        _ => None,
    };
    AuditEntryView {
        id: row.id.to_string(),
        at: row.at.to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
        event: row.event,
        subject_type: row.subject_type,
        subject_key: row.subject_key,
        actor: AuditActorView {
            kind: row.actor_kind,
            id: row.actor_user_id.map(|id| id.to_string()),
            name: row.actor_user_id.and_then(|id| names.get(&id).cloned()),
            credential: credential_kind(row.credential.as_deref()),
        },
        ip: row.ip.filter(|_| addresses),
        user_agent: row.user_agent.filter(|_| addresses),
        request_id: row.request_id,
        changes: object(row.changes),
        // Credential identifiers (a JWT sign-in) are shown by kind only, as on the actor.
        meta: object(row.meta).map(|mut meta| {
            meta.remove("family");
            meta
        }),
    }
}

/// `GET audit`: the whole log, for auditors.
async fn log(AdminCtx(ctx): AdminCtx, RawQuery(query): RawQuery) -> AppResult<Json<AuditPage>> {
    // Admission first: a caller without access learns nothing from 422s.
    if !ctx.can(VIEW_ALL) {
        return Err(AppError::Forbidden);
    }
    let allowed = ["event", "subject_type", "subject_key", "actor", "from", "to", "cursor", "per_page"];
    let filters = parse(query.as_deref(), &allowed)?;
    let auth = ctx.can(VIEW_AUTH);
    let mut select = audit_log::Entity::find();
    if !auth {
        for family in AUTH_FAMILIES {
            select = select.filter(audit_log::Column::Event.not_like(family));
        }
    }
    if let Some(event) = &filters.event {
        select = select.filter(audit_log::Column::Event.eq(event.as_str()));
    }
    if let Some(subject) = &filters.subject_type {
        select = select.filter(audit_log::Column::SubjectType.eq(subject.as_str()));
    }
    if let Some(key) = &filters.subject_key {
        select = select.filter(audit_log::Column::SubjectKey.eq(rustcms_core::audit::subject_key(key)));
    }
    if let Some(actor) = filters.actor {
        select = select.filter(audit_log::Column::ActorUserId.eq(actor));
    }
    if let Some(from) = filters.from {
        select = select.filter(audit_log::Column::At.gte(from));
    }
    if let Some(to) = filters.to {
        select = select.filter(audit_log::Column::At.lt(to));
    }
    let (rows, next_cursor) = page(&ctx, select, filters.cursor, filters.per_page).await?;
    let names = names(&ctx, &rows).await?;
    let data = rows.into_iter().map(|row| view(row, &names, auth)).collect();
    Ok(Json(AuditPage { data, meta: AuditPageMeta { next_cursor } }))
}

/// `GET resources/{slug}/records/{key}/history`: what the engine recorded
/// about a record the user may view, showing only fields they may read.
async fn history(
    AdminCtx(ctx): AdminCtx,
    Path((slug, key)): Path<(String, String)>,
    RawQuery(query): RawQuery,
) -> AppResult<Json<AuditPage>> {
    if !ctx.can(VIEW) {
        return Err(AppError::Forbidden);
    }
    let resource = ctx.registry.get(&slug).ok_or(AppError::NotFound)?.clone();
    if !resource.meta().audit {
        return Err(AppError::NotFound);
    }
    // As the record page: in scope and allowed by the policy (404 / 403).
    let envelope = resource.find(&ctx, &key).await?;
    let filters = parse(query.as_deref(), &["cursor", "per_page"])?;
    let select = audit_log::Entity::find()
        .filter(audit_log::Column::SubjectType.eq(slug.as_str()))
        .filter(audit_log::Column::SubjectKey.eq(rustcms_core::audit::subject_key(&envelope.key)))
        .filter(
            Condition::any()
                .add(audit_log::Column::Event.is_in(RECORD_EVENTS))
                .add(audit_log::Column::Event.like("action:%")),
        );
    let (rows, next_cursor) = page(&ctx, select, filters.cursor, filters.per_page).await?;
    let names = names(&ctx, &rows).await?;
    let readable = |field: &str| resource.field_access(&ctx, field).0;
    // Links: targets shown to those who may list the target, pivot columns
    // to those who may read them now (whatever was recorded).
    let mut links = HashMap::new();
    for relation in resource.relations().iter().filter(|r| r.is_many()) {
        let targets = match ctx.registry.get(relation.target) {
            Some(target) => target.allows(&ctx, crate::policy::ability::VIEW_ANY).await,
            None => false,
        };
        let pivot: Vec<String> =
            relation.pivot_fields.iter().filter(|f| f.can_read(&ctx)).map(|f| f.name.clone()).collect();
        links.insert(relation.name.to_owned(), (targets, pivot));
    }
    let data = rows
        .into_iter()
        .map(|row| {
            let mut entry = view(row, &names, false);
            entry.request_id = None;
            entry.changes = entry.changes.map(|changes| readable_changes(changes, &readable));
            entry.meta = if LINK_EVENTS.contains(&entry.event.as_str()) {
                entry.meta.and_then(|meta| link_meta(meta, &links))
            } else {
                // Only an action's result: relation parents and anything else stay out.
                entry.meta.and_then(|meta| {
                    let kept: Map<String, Value> = meta.into_iter().filter(|(k, _)| k == "result").collect();
                    (!kept.is_empty()).then_some(kept)
                })
            };
            entry
        })
        .collect();
    Ok(Json(AuditPage { data, meta: AuditPageMeta { next_cursor } }))
}

/// A link entry's meta for a reader: the relation and how many targets;
/// the targets themselves and the pivot columns only as `links` allows (by
/// relation: may list the target, readable pivot columns). A relation since
/// removed shows its name and count only.
fn link_meta(mut meta: Map<String, Value>, links: &HashMap<String, (bool, Vec<String>)>) -> Option<Map<String, Value>> {
    let relation = meta.get("relation").and_then(Value::as_str)?.to_owned();
    let targets = meta.remove("targets").and_then(|t| t.as_array().cloned()).unwrap_or_default();
    let (show_targets, pivot_fields) = links.get(&relation).cloned().unwrap_or_default();
    let mut out = Map::new();
    out.insert("relation".into(), Value::String(relation));
    out.insert("count".into(), Value::from(targets.len()));
    if show_targets {
        out.insert("targets".into(), Value::Array(targets));
    }
    if let Some(Value::Object(pivot)) = meta.remove("pivot") {
        let kept: Map<String, Value> = pivot.into_iter().filter(|(k, _)| pivot_fields.contains(k)).collect();
        if !kept.is_empty() {
            out.insert("pivot".into(), Value::Object(kept));
        }
    }
    Some(out)
}

/// `changes` restricted to the fields `readable` allows (an overflow keeps
/// only their names).
fn readable_changes(changes: Map<String, Value>, readable: &dyn Fn(&str) -> bool) -> Map<String, Value> {
    if changes.get("overflow") == Some(&Value::Bool(true)) {
        let fields: Vec<Value> = changes
            .get("fields")
            .and_then(Value::as_array)
            .map(|f| f.iter().filter(|n| n.as_str().is_some_and(readable)).cloned().collect())
            .unwrap_or_default();
        return Map::from_iter([
            ("overflow".to_owned(), Value::Bool(true)),
            ("fields".to_owned(), Value::Array(fields)),
        ]);
    }
    changes.into_iter().filter(|(field, _)| readable(field)).collect()
}

/// The audit permissions (group `audit`).
pub(crate) fn permissions() -> [&'static str; 3] {
    [VIEW_ALL, VIEW_AUTH, VIEW]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursors_round_trip_and_reject_garbage() {
        let row = audit_log::Model {
            id: uuid::Uuid::now_v7(),
            at: Utc::now(),
            event: "x".into(),
            subject_type: "y".into(),
            subject_key: None,
            actor_kind: "system".into(),
            actor_user_id: None,
            actor_guard: None,
            credential: None,
            ip: None,
            user_agent: None,
            request_id: None,
            changes: None,
            meta: None,
        };
        let (at, id) = decode_cursor(&encode_cursor(&row)).unwrap();
        assert_eq!((at, id), (row.at, row.id));
        for bad in ["", "!!", "7878", &"a".repeat(200)] {
            assert!(decode_cursor(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn history_shows_readable_fields_only() {
        let readable = |f: &str| f != "secret";
        let changes = serde_json::json!({"title": [1, 2], "secret": [3, 4]});
        let kept = readable_changes(changes.as_object().unwrap().clone(), &readable);
        assert_eq!(Value::Object(kept), serde_json::json!({"title": [1, 2]}));
        let overflow = serde_json::json!({"overflow": true, "fields": ["title", "secret"]});
        let kept = readable_changes(overflow.as_object().unwrap().clone(), &readable);
        assert_eq!(Value::Object(kept), serde_json::json!({"overflow": true, "fields": ["title"]}));
        assert_eq!(credential_kind(Some("token:42")).as_deref(), Some("token"));
        assert_eq!(credential_kind(Some("jwt:abc")).as_deref(), Some("jwt"));
    }
}
