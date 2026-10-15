//! Imports of CSV files into a resource (ADR 0021): upload (checked and
//! stored), a preview that checks every row without writing, then a queued
//! job writing each row through the usual create or update path (field
//! rules, hooks, policy, scope, unique constraints), one transaction and one
//! idempotency receipt per row, the user reloaded at each batch.
//!
//! Needs [`ResourceMeta::importable`](crate::ResourceMeta::importable) and the
//! storage provider (the file is kept until the import expires).

pub mod entity;

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
use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::Data;
use crate::context::Ctx;
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
    fn new(row: u32, field: Option<String>, message: &str) -> Self {
        let issue = rustcms_core::i18n::Issue::of(message);
        Self { row, field, message: message.to_owned(), code: issue.code.to_owned(), params: issue.params }
    }
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

/// The header and rows of a CSV file, within the limits.
fn parse(bytes: &[u8]) -> AppResult<(Vec<String>, Vec<Vec<String>>)> {
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    if std::str::from_utf8(bytes).is_err() {
        return Err(file_error("is invalid"));
    }
    let mut reader = csv::ReaderBuilder::new().has_headers(true).flexible(false).from_reader(bytes);
    let header: Vec<String> =
        reader.headers().map_err(|_| file_error("is invalid"))?.iter().map(|h| h.trim().to_owned()).collect();
    if header.is_empty() || header.iter().all(String::is_empty) {
        return Err(file_error("is required"));
    }
    if header.len() > MAX_COLUMNS {
        return Err(file_error(format!("may not have more than {MAX_COLUMNS} items")));
    }
    let mut names = HashSet::new();
    if header.iter().any(|h| h.is_empty() || !names.insert(h.as_str())) {
        return Err(file_error("is invalid"));
    }
    let mut rows = Vec::new();
    for record in reader.records() {
        let record = record.map_err(|_| file_error("is invalid"))?;
        if rows.len() == MAX_ROWS {
            return Err(file_error(format!("may not have more than {MAX_ROWS} items")));
        }
        if record.iter().any(|c| c.chars().count() > MAX_CELL) {
            return Err(file_error(format!("may not be longer than {MAX_CELL} characters")));
        }
        rows.push(record.iter().map(str::to_owned).collect());
    }
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

/// `POST {resource}/imports` (the CSV file as the body): checked, stored.
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
    let body = raw_body(request, &app).await?;
    if body.len() > MAX_BYTES {
        return Err(AppError::PayloadTooLarge);
    }
    let (header, rows) = parse(&body)?;
    use sha2::Digest;
    let sha256 = hex::encode(sha2::Sha256::digest(&body));
    let key = StorageKey::generate("csv")?;
    let staged = Staged(storage.staging_dir().join(format!("import-{}.csv", uuid::Uuid::now_v7())));
    tokio::fs::write(&staged.0, &body).await.map_err(AppError::internal)?;
    let now = Utc::now();
    let txn = rustcms_core::db::begin_write(&ctx.db).await?;
    // One user's imports are counted and added one at a time.
    rustcms_core::db::xact_lock(&txn, &rustcms_core::db::lock_slot("idem", &format!("imports:{user_id}"))).await?;
    let waiting = entity::Entity::find()
        .filter(entity::Column::UserId.eq(user_id))
        .filter(entity::Column::Status.is_in(["uploaded", "running"]))
        .filter(entity::Column::ExpiresAt.gt(now))
        .count(&txn)
        .await?;
    if waiting >= PENDING_PER_USER {
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
        expires_at: Set(now + KEEP),
    }
    .insert(&txn)
    .await?;
    txn.commit().await?;
    if let Err(err) = storage.put_file(&key, &staged.0, "text/csv").await {
        entity::Entity::delete_by_id(row.id).exec(&ctx.db).await?;
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

async fn file_rows(app: &AppState, row: &entity::Model) -> AppResult<(Vec<String>, Vec<Vec<String>>)> {
    let storage = storage(app).ok_or_else(|| AppError::internal("storage not registered"))?;
    if row.disk != storage.disk() {
        return Err(AppError::internal("import stored on another disk"));
    }
    let (body, _) = storage.open(&StorageKey::parse(&row.storage_key)?).await?;
    let bytes = axum::body::to_bytes(body, MAX_BYTES + 1).await.map_err(AppError::internal)?;
    parse(&bytes)
}

/// A row as input: mapped columns only, cells read by field type.
fn row_data(
    resource: &dyn DynResource,
    header: &[String],
    mapping: &BTreeMap<String, String>,
    cells: &[String],
) -> Data {
    header
        .iter()
        .zip(cells)
        .filter_map(|(column, cell)| {
            let field = mapping.get(column)?;
            Some((field.clone(), resource.import_value(field, cell)))
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
        match write(&job, &row).await {
            Ok(()) => Ok(()),
            Err(Ok(Stopped(message))) => {
                entity::Entity::update_many()
                    .col_expr(entity::Column::Status, Expr::value("failed"))
                    .col_expr(entity::Column::Error, Expr::value(message))
                    .filter(entity::Column::Id.eq(row.id))
                    .exec(&db)
                    .await?;
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
    let mut counts = (0i32, 0i32, 0i32);
    let mut report: Vec<RowIssue> = Vec::new();
    for (batch, chunk) in rows.chunks(BATCH).enumerate() {
        // The user again at each batch: revoked access stops the import.
        let ctx = ctx_for(principal(app, row).await?);
        let resource = resource(&ctx, &row.resource).map_err(Err)?;
        if resource.admit_import(&ctx).await.is_err() {
            return Err(Ok(Stopped("revoked")));
        }
        let key = resource.import_key().filter(|k| mapping.values().any(|f| f == k)).map(str::to_owned);
        for (offset, cells) in chunk.iter().enumerate() {
            let number = u32::try_from(batch * BATCH + offset + 2).unwrap_or(u32::MAX);
            let ctx = ctx.clone().with_idempotency_key(Some(format!("import-{}-{number}", row.id)));
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
            match outcome {
                Ok(true) => counts.0 += 1,
                Ok(false) => counts.1 += 1,
                // Busy or unavailable: the whole job is retried (written rows replay).
                Err(err @ (AppError::Internal(_) | AppError::TooManyRequests { .. })) => return Err(Err(err)),
                Err(err) => {
                    counts.2 += 1;
                    for issue in issues(number, &err) {
                        if report.len() < MAX_REPORT {
                            report.push(issue);
                        }
                    }
                }
            }
        }
        // Progress after each batch.
        entity::Entity::update_many()
            .col_expr(entity::Column::Created, Expr::value(counts.0))
            .col_expr(entity::Column::Updated, Expr::value(counts.1))
            .col_expr(entity::Column::Failed, Expr::value(counts.2))
            .col_expr(entity::Column::Report, Expr::value(json!(report)))
            .filter(entity::Column::Id.eq(row.id))
            .exec(app.db())
            .await
            .map_err(|e| Err(e.into()))?;
    }
    // Done, recorded once (the status changes in the same transaction).
    let txn = rustcms_core::db::begin_write(app.db()).await.map_err(|e| Err(e.into()))?;
    let done = entity::Entity::update_many()
        .col_expr(entity::Column::Status, Expr::value("done"))
        .filter(entity::Column::Id.eq(row.id))
        .filter(entity::Column::Status.eq("running"))
        .exec(&txn)
        .await
        .map_err(|e| Err(e.into()))?;
    if done.rows_affected == 1 {
        let meta = json!({"import": row.id.to_string(), "rows": rows.len(), "created": counts.0, "updated": counts.1, "failed": counts.2});
        audit::record(&txn, &job.audit_context(), AuditEntry::new("imported", &row.resource).meta(meta))
            .await
            .map_err(Err)?;
    }
    txn.commit().await.map_err(|e| Err(e.into()))?;
    Ok(())
}

/// Deletes expired imports (rows and files), and fails imports whose job
/// never finished. Returns how many were deleted.
pub async fn prune(db: &sea_orm::DatabaseConnection, storage: &dyn Storage) -> AppResult<u64> {
    let now = Utc::now();
    entity::Entity::update_many()
        .col_expr(entity::Column::Status, Expr::value("failed"))
        .col_expr(entity::Column::Error, Expr::value("unfinished"))
        .filter(entity::Column::Status.eq("running"))
        .filter(entity::Column::CreatedAt.lt(now - chrono::Duration::hours(6)))
        .exec(db)
        .await?;
    let expired = entity::Entity::find().filter(entity::Column::ExpiresAt.lte(now)).all(db).await?;
    let mut deleted = 0;
    for row in expired.into_iter().take(500) {
        if row.disk == storage.disk() {
            storage.delete(&StorageKey::parse(&row.storage_key)?).await?;
        }
        entity::Entity::delete_by_id(row.id).exec(db).await?;
        deleted += 1;
    }
    storage.sweep(std::time::Duration::from_secs(3_600)).await?;
    Ok(deleted)
}
