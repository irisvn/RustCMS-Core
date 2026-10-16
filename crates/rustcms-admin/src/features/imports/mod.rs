//! Imports of CSV files into a resource (ADR 0021): upload (checked and
//! stored), a preview that checks every row without writing, then a queued
//! job writing each row through the usual create or update path (field
//! rules, hooks, policy, scope, unique constraints): one transaction per row,
//! holding the RBAC lock and authorizing the user as they are then, committing
//! the row's receipt (a later run of the job skips the rows written).
//!
//! Needs [`ResourceMeta::importable`](crate::ResourceMeta::importable) and the
//! storage provider (the file is kept until the import expires).

pub mod entity;
pub mod receipt;

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use async_trait::async_trait;
use axum::Router;
use axum::extract::{Path, Request, State};
use axum::http::{Method, StatusCode};
use axum::routing::{get, post};
use chrono::Utc;
use rustcms_auth::{ADMIN_GUARD, AuthState};
use rustcms_core::audit::{self, AuditEntry};
use rustcms_core::models::user;
use rustcms_core::queue::{Job, JobCtx};
use rustcms_core::storage::{Storage, StorageKey};
use rustcms_core::{AppError, AppResult, AppState, Json, Principal, ValidationErrors};
use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseTransaction, DbBackend, EntityTrait,
    PaginatorTrait, QueryFilter, QuerySelect,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::Data;
use crate::context::{Ctx, ImportRow};
use crate::field::Operation;
use crate::http::{AdminCtx, AdminPanel, json_body, raw_body};
use crate::registry::DynResource;

/// Largest file.
pub const MAX_BYTES: usize = 5 * 1024 * 1024;
/// Most rows (header excluded).
pub const MAX_ROWS: usize = 10_000;
/// Most columns.
pub const MAX_COLUMNS: usize = 100;
/// Longest cell, in characters.
pub const MAX_CELL: usize = 10_000;
/// Imports a user may have uploaded or running at once.
const PENDING_PER_USER: u64 = 3;
/// How long an import (its file and report) is kept.
const KEEP: chrono::Duration = chrono::Duration::hours(24);
/// Rows written (and the user checked again) at a time.
const BATCH: usize = 100;
/// Most entries kept in a report (the counts are complete).
const MAX_REPORT: usize = 1_000;
/// Files read at once (uploads and previews): each holds its cells in memory.
static READING: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(4);
/// A running import with no progress for this long is abandoned (longer than
/// every run of its job with backoff).
const ABANDONED: chrono::Duration = chrono::Duration::hours(6);

/// A file is read by one request at a time per permit (beyond: 429).
fn reading() -> AppResult<tokio::sync::SemaphorePermit<'static>> {
    READING.try_acquire().map_err(|_| AppError::TooManyRequests { retry_after: 5 })
}

/// Every import route as `(method, path)`.
pub(crate) fn routes(admin_path: &str) -> Vec<(Method, String)> {
    let base = format!("{}/api", admin_path.trim_end_matches('/'));
    vec![
        (Method::POST, format!("{base}/resources/{{slug}}/imports")),
        (Method::GET, format!("{base}/imports/{{id}}")),
        (Method::POST, format!("{base}/imports/{{id}}/preview")),
        (Method::POST, format!("{base}/imports/{{id}}/run")),
    ]
}

pub(crate) fn router(router: Router<AppState>, admin_path: &str) -> Router<AppState> {
    let base = format!("{}/api", admin_path.trim_end_matches('/'));
    router
        .route(&format!("{base}/resources/{{slug}}/imports"), post(upload))
        .route(&format!("{base}/imports/{{id}}"), get(show))
        .route(&format!("{base}/imports/{{id}}/preview"), post(preview))
        .route(&format!("{base}/imports/{{id}}/run"), post(run))
}

/// The upload route of `slug`, whose body limit is [`MAX_BYTES`].
pub(crate) fn upload_path(admin_path: &str, slug: &str) -> String {
    format!("{}/api/resources/{slug}/imports", admin_path.trim_end_matches('/'))
}

/// One problem of a row (never its values).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RowIssue {
    /// Line in the file (the header is line 1).
    pub row: u32,
    /// Field (absent: the whole row).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    /// Message (English source).
    pub message: String,
    /// Its code and placeholder values, for the panel to translate.
    pub code: String,
    /// Placeholder values of the code.
    #[cfg_attr(feature = "ts", ts(as = "Option<BTreeMap<String, String>>", optional))]
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, String>,
}

impl RowIssue {
    /// Only known messages whose placeholders are numbers (limits) are kept:
    /// others (a hook's text, a value of the file quoted back) are `is invalid`.
    fn new(row: u32, field: Option<String>, message: &str) -> Self {
        let issue = rustcms_core::i18n::Issue::of(message);
        let safe = issue.code != rustcms_core::i18n::CUSTOM
            && issue.params.values().all(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()));
        if !safe {
            return Self::new(row, field, "is invalid");
        }
        Self { row, field, message: message.to_owned(), code: issue.code.to_owned(), params: issue.params }
    }
}

/// A cell of a file: its text, and whether it was quoted (exports write
/// `null` as an empty cell, the empty text as `""`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Cell {
    text: String,
    quoted: bool,
}

/// An import as its owner sees it.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ImportView {
    /// Id (UUID).
    pub id: String,
    /// Resource slug.
    pub resource: String,
    /// `uploaded`, `running`, `done` or `failed`.
    pub status: String,
    /// Column names of the file.
    pub header: Vec<String>,
    /// Rows of the file.
    pub rows: i32,
    /// Fields the user may set, with the import key marked.
    pub fields: Vec<crate::ImportField>,
    /// Column → field suggested (by name or label) or fixed (once it runs).
    pub mapping: BTreeMap<String, String>,
    /// Records created.
    pub created: i32,
    /// Records updated.
    pub updated: i32,
    /// Rows that failed.
    pub failed: i32,
    /// Problems of the rows that failed (at most 1,000).
    pub report: Vec<RowIssue>,
    /// Why it stopped: `revoked`, `account_changed`, `unfinished` or `failed`.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// What a preview found.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    /// Rows checked.
    pub rows: u32,
    /// Rows with no problem.
    pub valid: u32,
    /// Rows with an import key: created or updated, decided when it runs.
    pub keyed: u32,
    /// Problems found (at most 1,000).
    pub report: Vec<RowIssue>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MappingInput {
    mapping: BTreeMap<String, String>,
}

fn storage(app: &AppState) -> Option<Arc<dyn Storage>> {
    app.service::<Arc<dyn Storage>>().map(|s| s.as_ref().clone())
}

fn resource(ctx: &Ctx, slug: &str) -> AppResult<Arc<dyn DynResource>> {
    ctx.registry.get(slug).cloned().ok_or(AppError::NotFound)
}

fn file_error(message: impl Into<String>) -> AppError {
    ValidationErrors::field("file", message).into()
}

/// The header and rows of a CSV file (RFC 4180, `,`, CRLF or LF, every row
/// as wide as the header, blank lines skipped), within the limits.
fn parse(bytes: &[u8]) -> AppResult<(Vec<String>, Vec<Vec<Cell>>)> {
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let text = std::str::from_utf8(bytes).map_err(|_| file_error("is invalid"))?;
    let mut chars = text.chars().peekable();
    let mut header: Option<Vec<String>> = None;
    let mut rows: Vec<Vec<Cell>> = Vec::new();
    let mut record: Vec<Cell> = Vec::new();
    while chars.peek().is_some() || !record.is_empty() {
        let mut cell = String::new();
        let mut length = 0usize;
        let quoted = chars.peek() == Some(&'"');
        if quoted {
            chars.next();
            loop {
                match chars.next() {
                    None => return Err(file_error("is invalid")),
                    Some('"') if chars.peek() == Some(&'"') => {
                        chars.next();
                        cell.push('"');
                    }
                    Some('"') => break,
                    Some(c) => cell.push(c),
                }
                length += 1;
                if length > MAX_CELL {
                    return Err(file_error(format!("may not be longer than {MAX_CELL} characters")));
                }
            }
        } else {
            while let Some(&c) = chars.peek() {
                match c {
                    ',' | '\r' | '\n' => break,
                    '"' => return Err(file_error("is invalid")),
                    _ => {}
                }
                cell.push(c);
                chars.next();
                length += 1;
                if length > MAX_CELL {
                    return Err(file_error(format!("may not be longer than {MAX_CELL} characters")));
                }
            }
        }
        record.push(Cell { text: cell, quoted });
        let width = header.as_ref().map_or(MAX_COLUMNS, Vec::len);
        if record.len() > width {
            return Err(match header {
                None => file_error(format!("may not have more than {MAX_COLUMNS} items")),
                Some(_) => file_error("is invalid"),
            });
        }
        match chars.next() {
            Some(',') => continue,
            Some('\r') if chars.next() != Some('\n') => return Err(file_error("is invalid")),
            Some('\r' | '\n') | None => {}
            Some(_) => return Err(file_error("is invalid")),
        }
        let cells = std::mem::take(&mut record);
        // A blank line.
        if cells.len() == 1 && cells[0].text.is_empty() && !cells[0].quoted {
            continue;
        }
        match &header {
            None => {
                let names: Vec<String> = cells.into_iter().map(|c| c.text.trim().to_owned()).collect();
                let mut seen = HashSet::new();
                if names.iter().any(|h| h.is_empty() || !seen.insert(h.clone())) {
                    return Err(file_error("is invalid"));
                }
                header = Some(names);
            }
            Some(names) => {
                if cells.len() != names.len() {
                    return Err(file_error("is invalid"));
                }
                if rows.len() == MAX_ROWS {
                    return Err(file_error(format!("may not have more than {MAX_ROWS} items")));
                }
                rows.push(cells);
            }
        }
    }
    let header = header.ok_or_else(|| file_error("is required"))?;
    Ok((header, rows))
}

/// Column → field suggested for a header: a field's name, else its label (any case).
fn suggest(header: &[String], fields: &[crate::ImportField]) -> BTreeMap<String, String> {
    header
        .iter()
        .filter_map(|column| {
            let lower = column.to_lowercase();
            fields
                .iter()
                .find(|f| f.name == *column)
                .or_else(|| fields.iter().find(|f| f.name.to_lowercase() == lower || f.label.to_lowercase() == lower))
                .map(|f| (column.clone(), f.name.clone()))
        })
        .collect()
}

fn view(row: &entity::Model, fields: Vec<crate::ImportField>) -> ImportView {
    let header: Vec<String> = serde_json::from_value(row.header.clone()).unwrap_or_default();
    let mapping = match &row.mapping {
        Some(m) => serde_json::from_value(m.clone()).unwrap_or_default(),
        None => suggest(&header, &fields),
    };
    ImportView {
        id: row.id.to_string(),
        resource: row.resource.clone(),
        status: row.status.clone(),
        header,
        rows: row.rows,
        fields,
        mapping,
        created: row.created,
        updated: row.updated,
        failed: row.failed,
        report: row.report.clone().and_then(|r| serde_json::from_value(r).ok()).unwrap_or_default(),
        error: row.error.clone(),
    }
}

/// The user's imports waiting or running, counted in `db`.
async fn pending<C: ConnectionTrait>(db: &C, user_id: i64) -> AppResult<u64> {
    Ok(entity::Entity::find()
        .filter(entity::Column::UserId.eq(user_id))
        .filter(entity::Column::Status.is_in(["uploaded", "running"]))
        .filter(entity::Column::ExpiresAt.gt(Utc::now()))
        .count(db)
        .await?)
}

/// `POST {resource}/imports` (the CSV file as the body): checked, stored.
/// With an `Idempotency-Key`, a retry of the same file answers with the same import.
async fn upload(
    State(app): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Path(slug): Path<String>,
    request: Request,
) -> AppResult<(StatusCode, Json<ImportView>)> {
    let resource = resource(&ctx, &slug)?;
    // Admitted before the body is read.
    resource.admit_import(&ctx).await?;
    let storage = storage(&app).ok_or(AppError::NotFound)?;
    let user_id: i64 = ctx.principal.id.parse().map_err(|_| AppError::Forbidden)?;
    let auth_version = ctx.principal.auth_version.ok_or(AppError::Forbidden)?;
    let idempotency = crate::http::idempotency_key(request.headers())?;
    // Refused before the file is read when the user has too many already
    // (counted again below, under the lock), and read by few requests at once.
    if idempotency.is_none() && pending(&ctx.db, user_id).await? >= PENDING_PER_USER {
        return Err(AppError::TooManyRequests { retry_after: 60 });
    }
    let permit = reading()?;
    let body = raw_body(request, &app).await?;
    if body.len() > MAX_BYTES {
        return Err(AppError::PayloadTooLarge);
    }
    let (header, rows) = parse(&body)?;
    drop(permit);
    use sha2::Digest;
    let sha256 = hex::encode(sha2::Sha256::digest(&body));
    let claim = idempotency
        .as_deref()
        .map(|key| crate::idempotency::Claim::new(&ctx.principal, &slug, "import", key, &json!({ "sha256": sha256 })));
    let key = StorageKey::generate("csv")?;
    let staged = Staged(storage.staging_dir().join(format!("import-{}.csv", uuid::Uuid::now_v7())));
    tokio::fs::write(&staged.0, &body).await.map_err(AppError::internal)?;
    let now = Utc::now();
    let txn = rustcms_core::db::begin_write(&ctx.db).await?;
    // One user's imports are counted and added one at a time.
    rustcms_core::db::xact_lock(&txn, &rustcms_core::db::lock_slot("idem", &format!("imports:{user_id}"))).await?;
    if let Some(claim) = &claim
        && !crate::idempotency::acquire(&txn, claim).await?
    {
        txn.rollback().await?;
        let receipt = crate::idempotency::receipt(&ctx.db, claim).await?;
        let id = receipt["id"].as_str().and_then(|id| uuid::Uuid::parse_str(id).ok());
        let (row, _) =
            owned(&ctx, &id.ok_or_else(|| AppError::internal("import receipt without an id"))?.to_string()).await?;
        return Ok((StatusCode::CREATED, Json(view(&row, resource.import_fields(&ctx)))));
    }
    if pending(&txn, user_id).await? >= PENDING_PER_USER {
        txn.rollback().await?;
        return Err(AppError::TooManyRequests { retry_after: 60 });
    }
    // The row first: a crash after it leaves the object to pruning.
    let row = entity::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        user_id: Set(user_id),
        auth_version: Set(auth_version),
        resource: Set(slug.clone()),
        disk: Set(storage.disk().to_owned()),
        storage_key: Set(key.as_str().to_owned()),
        sha256: Set(sha256),
        header: Set(json!(header)),
        rows: Set(i32::try_from(rows.len()).unwrap_or(i32::MAX)),
        mapping: Set(None),
        status: Set("uploaded".into()),
        created: Set(0),
        updated: Set(0),
        failed: Set(0),
        report: Set(None),
        error: Set(None),
        created_at: Set(now),
        started_at: Set(None),
        expires_at: Set(now + KEEP),
    }
    .insert(&txn)
    .await?;
    if let Some(claim) = &claim {
        crate::idempotency::complete(&txn, claim, &json!({"id": row.id.to_string()})).await?;
    }
    txn.commit().await?;
    if let Err(err) = storage.put_file(&key, &staged.0, "text/csv").await {
        // As if never sent: a retry with the same key uploads again.
        entity::Entity::delete_by_id(row.id).exec(&ctx.db).await?;
        if let Some(claim) = &claim {
            crate::idempotency::release(&ctx.db, claim).await?;
        }
        return Err(err.into());
    }
    Ok((StatusCode::CREATED, Json(view(&row, resource.import_fields(&ctx)))))
}

/// A temporary file removed when dropped (also when the request is cancelled).
struct Staged(std::path::PathBuf);

impl Drop for Staged {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// The user's import `id` (someone else's or none: 404), while they may import.
async fn owned(ctx: &Ctx, id: &str) -> AppResult<(entity::Model, Arc<dyn DynResource>)> {
    let id = uuid::Uuid::parse_str(id).map_err(|_| AppError::NotFound)?;
    let row = entity::Entity::find_by_id(id)
        .filter(entity::Column::UserId.eq(ctx.principal.id.parse::<i64>().unwrap_or(-1)))
        .filter(entity::Column::ExpiresAt.gt(Utc::now()))
        .one(&ctx.db)
        .await?
        .ok_or(AppError::NotFound)?;
    let resource = resource(ctx, &row.resource)?;
    resource.admit_import(ctx).await?;
    Ok((row, resource))
}

/// `GET imports/{id}`: state, counts and the report.
async fn show(AdminCtx(ctx): AdminCtx, Path(id): Path<String>) -> AppResult<Json<ImportView>> {
    let (row, resource) = owned(&ctx, &id).await?;
    Ok(Json(view(&row, resource.import_fields(&ctx))))
}

/// `mapping` checked against the file and the fields the user may set.
fn check_mapping(
    header: &[String],
    fields: &[crate::ImportField],
    mapping: &BTreeMap<String, String>,
) -> AppResult<()> {
    let mut errors = ValidationErrors::new();
    let mut targets = HashSet::new();
    for (column, field) in mapping {
        if !header.contains(column) {
            errors.add(format!("mapping.{column}"), "is not allowed");
        } else if !fields.iter().any(|f| f.name == *field) || !targets.insert(field.as_str()) {
            errors.add(format!("mapping.{column}"), "is invalid");
        }
    }
    if mapping.is_empty() {
        errors.add("mapping", "is required");
    }
    errors.into_result()?;
    Ok(())
}

async fn file_rows(app: &AppState, row: &entity::Model) -> AppResult<(Vec<String>, Vec<Vec<Cell>>)> {
    let storage = storage(app).ok_or_else(|| AppError::internal("storage not registered"))?;
    if row.disk != storage.disk() {
        return Err(AppError::internal("import stored on another disk"));
    }
    let (body, _) = storage.open(&StorageKey::parse(&row.storage_key)?).await?;
    let bytes = axum::body::to_bytes(body, MAX_BYTES + 1).await.map_err(AppError::internal)?;
    parse(&bytes)
}

/// A row as input: mapped columns only, cells read by field type.
fn row_data(resource: &dyn DynResource, header: &[String], mapping: &BTreeMap<String, String>, cells: &[Cell]) -> Data {
    header
        .iter()
        .zip(cells)
        .filter_map(|(column, cell)| {
            let field = mapping.get(column)?;
            Some((field.clone(), resource.import_value(field, &cell.text, cell.quoted)))
        })
        .collect()
}

/// The problems of an error, by field (no values).
fn issues(row: u32, err: &AppError) -> Vec<RowIssue> {
    match err {
        AppError::Validation(errors) => {
            let mut out: Vec<RowIssue> = errors
                .errors
                .iter()
                .flat_map(|(field, messages)| messages.iter().map(|m| RowIssue::new(row, Some(field.clone()), m)))
                .collect();
            out.extend(errors.form_errors.iter().map(|m| RowIssue::new(row, None, m)));
            out
        }
        AppError::Forbidden => vec![RowIssue::new(row, None, "is not allowed")],
        AppError::NotFound => vec![RowIssue::new(row, None, "was not found")],
        _ => vec![RowIssue::new(row, None, "could not be imported")],
    }
}

/// `POST imports/{id}/preview` `{mapping}`: every row checked as it would be
/// written, without hooks or the database (a row with an import key is
/// checked as an update: create or update is decided when it runs).
async fn preview(
    State(app): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Path(id): Path<String>,
    request: Request,
) -> AppResult<Json<ImportPreview>> {
    let (row, resource) = owned(&ctx, &id).await?;
    let MappingInput { mapping } = json_body(request, &app).await?;
    let fields = resource.import_fields(&ctx);
    let _permit = reading()?;
    let (header, rows) = file_rows(&app, &row).await?;
    check_mapping(&header, &fields, &mapping)?;
    let key = resource.import_key().filter(|k| mapping.values().any(|f| f == k));
    let mut out = ImportPreview { rows: 0, valid: 0, keyed: 0, report: Vec::new() };
    for (index, cells) in rows.iter().enumerate() {
        let number = u32::try_from(index + 2).unwrap_or(u32::MAX);
        let mut data = row_data(resource.as_ref(), &header, &mapping, cells);
        let keyed = key.is_some_and(|k| data.get(k).is_some_and(|v| !v.is_null() && v != ""));
        let op = if keyed {
            if let Some(k) = key {
                data.remove(k);
            }
            out.keyed += 1;
            Operation::Update
        } else {
            Operation::Create
        };
        out.rows += 1;
        match resource.check_import_row(&ctx, op, data) {
            Ok(_) => out.valid += 1,
            Err(err) => {
                for issue in issues(number, &err) {
                    if out.report.len() < MAX_REPORT {
                        out.report.push(issue);
                    }
                }
            }
        }
    }
    Ok(Json(out))
}

/// `POST imports/{id}/run` `{mapping}`: fixes the mapping and queues the import (once).
async fn run(
    State(app): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Path(id): Path<String>,
    request: Request,
) -> AppResult<Json<ImportView>> {
    let (row, resource) = owned(&ctx, &id).await?;
    let MappingInput { mapping } = json_body(request, &app).await?;
    let fields = resource.import_fields(&ctx);
    let header: Vec<String> = serde_json::from_value(row.header.clone()).map_err(AppError::internal)?;
    check_mapping(&header, &fields, &mapping)?;
    let txn = rustcms_core::db::begin_write(&ctx.db).await?;
    // From `uploaded` once: a second run (another tab, a retry) changes nothing.
    let started = entity::Entity::update_many()
        .col_expr(entity::Column::Status, Expr::value("running"))
        .col_expr(entity::Column::Mapping, Expr::value(json!(mapping)))
        // Kept a full day from now: the job never writes after that.
        .col_expr(entity::Column::StartedAt, Expr::value(Utc::now()))
        .col_expr(entity::Column::ExpiresAt, Expr::value(Utc::now() + KEEP))
        .filter(entity::Column::Id.eq(row.id))
        .filter(entity::Column::Status.eq("uploaded"))
        .exec(&txn)
        .await?;
    if started.rows_affected != 1 {
        txn.rollback().await?;
        return Err(AppError::Conflict {
            code: "import_started",
            reason: "started",
            message: "This import has already started.",
        });
    }
    ctx.dispatch(&txn, &ImportJob { id: row.id }).await?;
    txn.commit().await?;
    let row = entity::Entity::find_by_id(row.id).one(&ctx.db).await?.ok_or(AppError::NotFound)?;
    Ok(Json(view(&row, fields)))
}

/// Writes an import's rows (`admin.import`).
#[derive(Debug, Serialize, Deserialize)]
pub struct ImportJob {
    /// The import.
    pub id: uuid::Uuid,
}

/// Why an import stops (a code its owner's panel translates; never retried).
struct Stopped(&'static str);

#[async_trait]
impl Job for ImportJob {
    const KIND: &'static str = "admin.import";

    fn max_attempts() -> u32 {
        3
    }

    fn timeout() -> std::time::Duration {
        std::time::Duration::from_secs(1_800)
    }

    async fn handle(self, job: JobCtx) -> anyhow::Result<()> {
        let db = job.state.db().clone();
        let Some(row) = entity::Entity::find_by_id(self.id).one(&db).await? else { return Ok(()) };
        if row.status != "running" {
            return Ok(());
        }
        // Expired (a run delayed past it): never written after that.
        if row.expires_at <= Utc::now() {
            stop(&db, row.id, "unfinished").await?;
            return Ok(());
        }
        match write(&job, &row).await {
            Ok(()) => Ok(()),
            Err(Ok(Stopped(message))) => {
                stop(&db, row.id, message).await?;
                Ok(())
            }
            // Retried by the queue: rows already written are answered by their receipts.
            Err(Err(err)) => {
                if job.attempt >= Self::max_attempts() {
                    entity::Entity::update_many()
                        .col_expr(entity::Column::Status, Expr::value("failed"))
                        .col_expr(entity::Column::Error, Expr::value("failed"))
                        .filter(entity::Column::Id.eq(row.id))
                        .filter(entity::Column::Status.eq("running"))
                        .exec(&db)
                        .await?;
                }
                Err(err.into())
            }
        }
    }
}

/// Marks a running import failed, saying why.
async fn stop(db: &sea_orm::DatabaseConnection, id: uuid::Uuid, code: &str) -> AppResult<()> {
    entity::Entity::update_many()
        .col_expr(entity::Column::Status, Expr::value("failed"))
        .col_expr(entity::Column::Error, Expr::value(code))
        .filter(entity::Column::Id.eq(id))
        .filter(entity::Column::Status.eq("running"))
        .exec(db)
        .await?;
    Ok(())
}

/// The row was written by an earlier run (its receipt is committed).
const ROW_WRITTEN: AppError =
    AppError::Conflict { code: "import_row_written", reason: "written", message: "This row was already imported." };
/// The import stopped meanwhile (failed or expired): nothing more is written.
const IMPORT_STOPPED: AppError =
    AppError::Conflict { code: "import_stopped", reason: "stopped", message: "This import has stopped." };

/// Records, in the transaction writing it, that `row` was written: the import
/// must still be running (held so until commit, so pruning waits), and a row
/// is recorded once ([`ROW_WRITTEN`] for a second time: the write rolls back).
pub(crate) async fn record_row(txn: &DatabaseTransaction, row: &ImportRow, outcome: &str, key: &str) -> AppResult<()> {
    let mut find = entity::Entity::find_by_id(row.import);
    if txn.get_database_backend() != DbBackend::Sqlite {
        find = find.lock_shared();
    }
    let import = find.one(txn).await?;
    if !import.is_some_and(|i| i.status == "running" && i.expires_at > Utc::now()) {
        return Err(IMPORT_STOPPED);
    }
    let receipt = receipt::ActiveModel {
        import_id: Set(row.import),
        row: Set(row.row),
        outcome: Set(outcome.to_owned()),
        record_key: Set(key.to_owned()),
    };
    match receipt::Entity::insert(receipt).exec_without_returning(txn).await {
        Ok(_) => Ok(()),
        Err(err) if matches!(err.sql_err(), Some(sea_orm::SqlErr::UniqueConstraintViolation(_))) => Err(ROW_WRITTEN),
        Err(err) => Err(err.into()),
    }
}

/// The user of `row` as they are now: active, same credential version, with
/// their current permissions.
async fn principal(app: &AppState, row: &entity::Model) -> Result<Principal, Result<Stopped, AppError>> {
    let db = app.db();
    let user = user::Entity::find_by_id(row.user_id).one(db).await.map_err(|e| Err(e.into()))?;
    let Some(user) = user.filter(|u| u.is_active && u.auth_version == row.auth_version) else {
        return Err(Ok(Stopped("account_changed")));
    };
    let auth = app.service::<AuthState>().ok_or_else(|| Err(AppError::internal("auth not registered")))?;
    auth.resolver.principal(db, ADMIN_GUARD, &user, None).await.map_err(Err)
}

/// Whether the user of `row` may still import (else why not).
async fn still_allowed(
    app: &AppState,
    panel: &AdminPanel,
    row: &entity::Model,
) -> Result<(), Result<Stopped, AppError>> {
    let ctx = Ctx::new(app.db().clone(), principal(app, row).await?, panel.registry.clone());
    let resource = resource(&ctx, &row.resource).map_err(Err)?;
    if resource.admit_import(&ctx).await.is_err() {
        return Err(Ok(Stopped("revoked")));
    }
    Ok(())
}

/// Receipts of the rows of import `id` written so far: line → created.
async fn written(db: &sea_orm::DatabaseConnection, id: uuid::Uuid) -> AppResult<BTreeMap<i32, bool>> {
    let receipts = receipt::Entity::find().filter(receipt::Column::ImportId.eq(id)).all(db).await?;
    Ok(receipts.into_iter().map(|r| (r.row, r.outcome == "created")).collect())
}

async fn write(job: &JobCtx, row: &entity::Model) -> Result<(), Result<Stopped, AppError>> {
    let app = &job.state;
    let panel = app.service::<AdminPanel>().ok_or_else(|| Err(AppError::internal("AdminProvider not registered")))?;
    let mapping: BTreeMap<String, String> = row
        .mapping
        .clone()
        .and_then(|m| serde_json::from_value(m).ok())
        .ok_or_else(|| Err(AppError::internal("import without a mapping")))?;
    let (header, rows) = file_rows(app, row).await.map_err(Err)?;
    let ctx_for = |principal: Principal| {
        Ctx::new(app.db().clone(), principal, panel.registry.clone()).with_audit(job.audit_context())
    };
    // Rows written by an earlier run are counted, never written again.
    let mut done = written(app.db(), row.id).await.map_err(Err)?;
    let mut counts = (0i32, 0i32, 0i32);
    let mut report: Vec<RowIssue> = Vec::new();
    for (batch, chunk) in rows.chunks(BATCH).enumerate() {
        // The user again at each batch (each row's write checks again, locked).
        let ctx = ctx_for(principal(app, row).await?);
        let resource = resource(&ctx, &row.resource).map_err(Err)?;
        if resource.admit_import(&ctx).await.is_err() {
            return Err(Ok(Stopped("revoked")));
        }
        let key = resource.import_key().filter(|k| mapping.values().any(|f| f == k)).map(str::to_owned);
        for (offset, cells) in chunk.iter().enumerate() {
            let number = i32::try_from(batch * BATCH + offset + 2).unwrap_or(i32::MAX);
            if let Some(created) = done.get(&number) {
                if *created {
                    counts.0 += 1
                } else {
                    counts.1 += 1
                }
                continue;
            }
            let ctx = ctx.clone().with_import_row(ImportRow { import: row.id, row: number });
            let mut data = row_data(resource.as_ref(), &header, &mapping, cells);
            let lookup = key.as_ref().and_then(|k| data.get(k).filter(|v| !v.is_null() && *v != "").cloned());
            let outcome = match lookup {
                None => resource.create(&ctx, data).await.map(|_| true),
                Some(value) => match resource.import_matches(&ctx, &value).await.map_err(Err)?.as_slice() {
                    [] => resource.create(&ctx, data).await.map(|_| true),
                    [found] => {
                        let found = found.clone();
                        if let Some(k) = &key {
                            data.remove(k);
                        }
                        resource.update(&ctx, &found, data).await.map(|_| false)
                    }
                    _ => {
                        let mut errors = ValidationErrors::new();
                        errors.add_form("matches several records");
                        Err(errors.into())
                    }
                },
            };
            let line = u32::try_from(number).unwrap_or(u32::MAX);
            match outcome {
                Ok(true) => counts.0 += 1,
                Ok(false) => counts.1 += 1,
                // Written meanwhile by another run of the job: counted as it recorded.
                Err(AppError::Conflict { code: "import_row_written", .. }) => {
                    done = written(app.db(), row.id).await.map_err(Err)?;
                    match done.get(&number) {
                        Some(true) => counts.0 += 1,
                        _ => counts.1 += 1,
                    }
                }
                Err(AppError::Conflict { code: "import_stopped", .. }) => return Err(Ok(Stopped("unfinished"))),
                // Busy or unavailable: the whole job is retried (written rows are skipped).
                Err(err @ (AppError::Internal(_) | AppError::TooManyRequests { .. })) => return Err(Err(err)),
                Err(err) => {
                    // Refused: the user's access ended meanwhile (stop), or this row's record.
                    if matches!(err, AppError::Forbidden) {
                        still_allowed(app, &panel, row).await?;
                    }
                    counts.2 += 1;
                    for issue in issues(line, &err) {
                        if report.len() < MAX_REPORT {
                            report.push(issue);
                        }
                    }
                }
            }
        }
        // Progress after each batch (while it runs).
        entity::Entity::update_many()
            .col_expr(entity::Column::Created, Expr::value(counts.0))
            .col_expr(entity::Column::Updated, Expr::value(counts.1))
            .col_expr(entity::Column::Failed, Expr::value(counts.2))
            .col_expr(entity::Column::Report, Expr::value(json!(report)))
            .filter(entity::Column::Id.eq(row.id))
            .filter(entity::Column::Status.eq("running"))
            .exec(app.db())
            .await
            .map_err(|e| Err(e.into()))?;
    }
    // Done, recorded once (the status changes in the same transaction).
    let txn = rustcms_core::db::begin_write(app.db()).await.map_err(|e| Err(e.into()))?;
    let finished = entity::Entity::update_many()
        .col_expr(entity::Column::Status, Expr::value("done"))
        .filter(entity::Column::Id.eq(row.id))
        .filter(entity::Column::Status.eq("running"))
        .filter(entity::Column::ExpiresAt.gt(Utc::now()))
        .exec(&txn)
        .await
        .map_err(|e| Err(e.into()))?;
    if finished.rows_affected == 1 {
        let meta = json!({"import": row.id.to_string(), "rows": rows.len(), "created": counts.0, "updated": counts.1, "failed": counts.2});
        audit::record(&txn, &job.audit_context(), AuditEntry::new("imported", &row.resource).meta(meta))
            .await
            .map_err(Err)?;
    }
    txn.commit().await.map_err(|e| Err(e.into()))?;
    Ok(())
}

/// Deletes expired imports (rows, receipts and files), and fails imports
/// abandoned while running (a row being written holds its import, so this
/// waits for it; later rows see the import failed and stop). Returns how
/// many were deleted.
pub async fn prune(db: &sea_orm::DatabaseConnection, storage: &dyn Storage) -> AppResult<u64> {
    let now = Utc::now();
    entity::Entity::update_many()
        .col_expr(entity::Column::Status, Expr::value("failed"))
        .col_expr(entity::Column::Error, Expr::value("unfinished"))
        .filter(entity::Column::Status.eq("running"))
        .filter(entity::Column::StartedAt.lt(now - ABANDONED))
        .exec(db)
        .await?;
    let expired = entity::Entity::find().filter(entity::Column::ExpiresAt.lte(now)).limit(500).all(db).await?;
    let mut deleted = 0;
    for row in expired {
        if row.disk == storage.disk() {
            storage.delete(&StorageKey::parse(&row.storage_key)?).await?;
        }
        receipt::Entity::delete_many().filter(receipt::Column::ImportId.eq(row.id)).exec(db).await?;
        entity::Entity::delete_by_id(row.id).exec(db).await?;
        deleted += 1;
    }
    storage.sweep(std::time::Duration::from_secs(3_600)).await?;
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cells(rows: &[Vec<Cell>]) -> Vec<Vec<(&str, bool)>> {
        rows.iter().map(|r| r.iter().map(|c| (c.text.as_str(), c.quoted)).collect()).collect()
    }

    #[test]
    fn files_keep_quoting_and_limits() {
        let (header, rows) =
            parse(b"\xef\xbb\xbf a ,b\r\n,\"\"\r\n\r\n\"x,\"\"y\"\"\",\"two\nlines\"\nlast,\n").unwrap();
        assert_eq!(header, ["a", "b"]);
        assert_eq!(
            cells(&rows),
            [
                vec![("", false), ("", true)],
                vec![("x,\"y\"", true), ("two\nlines", true)],
                vec![("last", false), ("", false)]
            ]
        );
        for bad in [
            &b""[..],
            b"\r\n",
            b"a,a\r\n",
            b"a,\r\n",
            b"a,b\r\n1\r\n",
            b"a\r\n1\"2\r\n",
            b"a\r\n\"1\"2\r\n",
            b"a\r\n\"1",
            b"a\rb",
        ] {
            assert!(parse(bad).is_err(), "{:?}", String::from_utf8_lossy(bad));
        }
        let long = format!("{}\r\n", "h".repeat(MAX_CELL + 1));
        assert!(parse(long.as_bytes()).is_err(), "header cells are bounded too");
    }

    #[test]
    fn reports_keep_only_safe_messages() {
        let kept = RowIssue::new(2, Some("title".into()), "may not be longer than 200 characters");
        assert_eq!((kept.code.as_str(), kept.params.get("max").map(String::as_str)), ("max_length", Some("200")));
        for unsafe_message in ["unknown operator `<script>`", "cannot sort by `secret value`", "Title Kept is banned"] {
            let issue = RowIssue::new(2, None, unsafe_message);
            assert_eq!((issue.message.as_str(), issue.code.as_str()), ("is invalid", "invalid"), "{unsafe_message}");
            assert!(issue.params.is_empty());
        }
    }
}
