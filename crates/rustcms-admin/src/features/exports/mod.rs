//! Exports of resource lists as CSV (ADR 0021): the list as the user sees it
//! (scope, filters, search, sort, per-record policy), readable fields only,
//! text protected against spreadsheet formulas.
//!
//! - `GET {resource}/export?…`: at most [`SYNC_ROWS`] rows and [`SYNC_BYTES`]
//!   bytes, built in full before anything is sent (beyond: 422).
//! - `POST {resource}/exports?…`: in the background (a queued job; needs the
//!   storage provider). The job reloads its user at each batch (active, same
//!   credential version, export allowed, the fields still readable) and
//!   publishes its file and the records in it together; the file is served
//!   only to that user, while they may still export the resource, read those
//!   fields and view every record in it.

mod csv;
pub mod entity;
pub mod object;

use std::sync::Arc;

use async_trait::async_trait;
use axum::Router;
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
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
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::Data;
use crate::context::Ctx;
use crate::http::{AdminCtx, AdminPanel, idempotency_key, raw_body};
use crate::registry::DynResource;
use crate::resource::ListQuery;

/// Most rows of an export answered at once.
pub const SYNC_ROWS: usize = 1_000;
/// Most bytes of an export answered at once.
pub const SYNC_BYTES: usize = 10 * 1024 * 1024;
/// Most rows of a background export.
pub const MAX_ROWS: usize = 10_000;
/// Most bytes of a background export's file.
pub const MAX_BYTES: usize = 50 * 1024 * 1024;
/// Background exports a user may have waiting or running.
const PENDING_PER_USER: u64 = 3;
/// How long a background export's file is kept.
const KEEP: chrono::Duration = chrono::Duration::hours(24);
/// Rows read (and the user checked again) at a time by the job.
const BATCH: usize = 500;
/// How long after expiry an export is pruned.
const PRUNE_GRACE: chrono::Duration = chrono::Duration::hours(1);
/// Runs of the job.
const ATTEMPTS: u32 = 3;

/// How a column's values are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellKind {
    /// Text: protected against spreadsheet formulas.
    Text,
    /// A JSON document: its JSON text, protected like text.
    Json,
    /// Numbers, booleans, dates, keys: as they are.
    Plain,
}

/// Every export route as `(method, path)`.
pub(crate) fn routes(admin_path: &str) -> Vec<(Method, String)> {
    let base = format!("{}/api", admin_path.trim_end_matches('/'));
    vec![
        (Method::GET, format!("{base}/resources/{{slug}}/export")),
        (Method::POST, format!("{base}/resources/{{slug}}/exports")),
        (Method::GET, format!("{base}/exports")),
        (Method::GET, format!("{base}/exports/{{id}}/file")),
    ]
}

pub(crate) fn router(router: Router<AppState>, admin_path: &str) -> Router<AppState> {
    let base = format!("{}/api", admin_path.trim_end_matches('/'));
    router
        .route(&format!("{base}/resources/{{slug}}/export"), get(export_now))
        .route(&format!("{base}/resources/{{slug}}/exports"), post(create))
        .route(&format!("{base}/exports"), get(list))
        .route(&format!("{base}/exports/{{id}}/file"), get(download))
}

/// A background export as its owner sees it.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ExportView {
    /// Id (UUID).
    pub id: String,
    /// Resource slug.
    pub resource: String,
    /// `pending`, `running`, `done` or `failed`.
    pub status: String,
    /// Rows written (when done).
    pub rows: i32,
    /// Why it failed: `revoked`, `account_changed`, `fields_changed`,
    /// `too_large`, `unfinished` or `failed`.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// RFC 3339.
    pub created_at: String,
    /// When the file is deleted (RFC 3339).
    pub expires_at: String,
}

fn view(row: &entity::Model) -> ExportView {
    ExportView {
        id: row.id.to_string(),
        resource: row.resource.clone(),
        status: row.status.clone(),
        rows: row.rows,
        error: row.error.clone(),
        created_at: row.created_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        expires_at: row.expires_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    }
}

fn resource(ctx: &Ctx, slug: &str) -> AppResult<Arc<dyn DynResource>> {
    ctx.registry.get(slug).cloned().ok_or(AppError::NotFound)
}

fn storage(app: &AppState) -> Option<Arc<dyn Storage>> {
    app.service::<Arc<dyn Storage>>().map(|s| s.as_ref().clone())
}

/// The header line of `columns`.
fn header_line(columns: &[(String, CellKind)]) -> String {
    csv::line(columns.iter().map(|(name, _)| csv::header(name)))
}

/// The line of one record.
fn row_line(columns: &[(String, CellKind)], record: &Data) -> String {
    csv::line(columns.iter().map(|(name, kind)| csv::cell(record.get(name).unwrap_or(&Value::Null), *kind)))
}

/// What the audit log keeps of a query: its shape, never searched or filtered values.
fn query_shape(query: &ListQuery) -> Value {
    json!({
        "filters": query.filters.iter().map(|f| json!({"field": f.field, "op": f.op})).collect::<Vec<_>>(),
        "search": query.search.as_deref().is_some_and(|s| !s.is_empty()),
        "sort": query.sort,
        "trashed": query.trashed,
    })
}

async fn audit_export(ctx: &Ctx, slug: &str, rows: usize, query: &ListQuery, export: Option<&str>) -> AppResult<()> {
    let txn = rustcms_core::db::begin_write(&ctx.db).await?;
    let mut meta = json!({"rows": rows, "query": query_shape(query)});
    if let Some(id) = export {
        meta["export"] = json!(id);
    }
    audit::record(&txn, &ctx.audit_context, AuditEntry::new("exported", slug).meta(meta)).await?;
    txn.commit().await?;
    Ok(())
}

fn csv_response(body: impl IntoResponse, slug: &str, len: Option<u64>) -> AppResult<Response> {
    let mut response = body.into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/csv; charset=utf-8"));
    if let Some(len) = len {
        headers.insert(header::CONTENT_LENGTH, len.into());
    }
    let date = Utc::now().format("%Y-%m-%d");
    headers.insert(header::CONTENT_DISPOSITION, crate::media::disposition(&format!("{slug}-{date}.csv"), false));
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("sandbox; default-src 'none'"));
    Ok(response)
}

fn too_large() -> AppError {
    let mut errors = ValidationErrors::new();
    errors.add_form("Too large to export at once; export in the background.");
    errors.into()
}

/// `GET {resource}/export?{list query}`: the list now, as CSV.
async fn export_now(
    AdminCtx(ctx): AdminCtx,
    Path(slug): Path<String>,
    Query(pairs): Query<Vec<(String, String)>>,
) -> AppResult<Response> {
    let resource = resource(&ctx, &slug)?;
    resource.admit_export(&ctx).await?;
    let query = resource.canonical_query(&ctx, &crate::http::list_query(pairs)?).await?;
    let keys = resource.export_keys(&ctx, &query, SYNC_ROWS).await?;
    let columns = resource.export_columns(&ctx);
    let names: Vec<String> = columns.iter().map(|(n, _)| n.clone()).collect();
    let mut body = header_line(&columns);
    let mut rows = 0;
    // A chunk at a time: only the file is kept, within its budget.
    for chunk in keys.chunks(100) {
        for (_, record) in resource.export_rows(&ctx, &query, chunk, &names).await? {
            body.push_str(&row_line(&columns, &record));
            rows += 1;
            if body.len() > SYNC_BYTES {
                return Err(too_large());
            }
        }
    }
    audit_export(&ctx, &slug, rows, &query, None).await?;
    let len = body.len() as u64;
    csv_response(body, &slug, Some(len))
}

/// `POST {resource}/exports?{list query}` (no body, or `{}`): a background
/// export; with an `Idempotency-Key`, a retry answers with the same export.
async fn create(
    State(app): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Path(slug): Path<String>,
    Query(pairs): Query<Vec<(String, String)>>,
    request: Request,
) -> AppResult<(StatusCode, Json<ExportView>)> {
    let resource = resource(&ctx, &slug)?;
    resource.admit_export(&ctx).await?;
    let storage = storage(&app).ok_or(AppError::NotFound)?;
    let key = idempotency_key(request.headers())?;
    let body = raw_body(request, &app).await?;
    // The query is the URL's: a body would be ignored, so none is taken.
    if !body.iter().all(u8::is_ascii_whitespace) && body.as_ref() != b"{}" {
        return Err(ValidationErrors::field("body", "is not allowed").into());
    }
    let query = resource.canonical_query(&ctx, &crate::http::list_query(pairs)?).await?;
    let user_id: i64 = ctx.principal.id.parse().map_err(|_| AppError::Forbidden)?;
    let auth_version = ctx.principal.auth_version.ok_or(AppError::Forbidden)?;
    let columns: Vec<String> = resource.export_columns(&ctx).into_iter().map(|(n, _)| n).collect();
    let claim = key.as_deref().map(|key| {
        crate::idempotency::Claim::new(
            &ctx.principal,
            &slug,
            "export",
            key,
            &serde_json::to_value(&query).unwrap_or_default(),
        )
    });
    let txn = rustcms_core::db::begin_write(&ctx.db).await?;
    // One user's exports are counted and added one at a time.
    rustcms_core::db::xact_lock(&txn, &rustcms_core::db::lock_slot("idem", &format!("exports:{user_id}"))).await?;
    if let Some(claim) = &claim
        && !crate::idempotency::acquire(&txn, claim).await?
    {
        txn.rollback().await?;
        let receipt = crate::idempotency::receipt(&ctx.db, claim).await?;
        let id = receipt["id"].as_str().and_then(|id| uuid::Uuid::parse_str(id).ok());
        let row = entity::Entity::find_by_id(id.ok_or_else(|| AppError::internal("export receipt without an id"))?)
            .one(&ctx.db)
            .await?
            .ok_or(AppError::NotFound)?;
        return Ok((StatusCode::CREATED, Json(view(&row))));
    }
    let waiting = entity::Entity::find()
        .filter(entity::Column::UserId.eq(user_id))
        .filter(entity::Column::Status.is_in(["pending", "running"]))
        .count(&txn)
        .await?;
    if waiting >= PENDING_PER_USER {
        txn.rollback().await?;
        return Err(AppError::TooManyRequests { retry_after: 60 });
    }
    let now = Utc::now();
    let row = entity::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        user_id: Set(user_id),
        auth_version: Set(auth_version),
        resource: Set(slug.clone()),
        query: Set(serde_json::to_value(&query).map_err(AppError::internal)?),
        columns: Set(json!(columns)),
        keys: Set(None),
        status: Set("pending".into()),
        rows: Set(0),
        disk: Set(Some(storage.disk().to_owned())),
        storage_key: Set(None),
        error: Set(None),
        created_at: Set(now),
        expires_at: Set(now + KEEP),
    }
    .insert(&txn)
    .await?;
    ctx.dispatch(&txn, &ExportJob { id: row.id }).await?;
    if let Some(claim) = &claim {
        crate::idempotency::complete(&txn, claim, &json!({"id": row.id.to_string()})).await?;
    }
    txn.commit().await?;
    Ok((StatusCode::CREATED, Json(view(&row))))
}

/// `GET exports`: the user's background exports (newest first, not expired).
async fn list(AdminCtx(ctx): AdminCtx) -> AppResult<Json<Vec<ExportView>>> {
    let Ok(user_id) = ctx.principal.id.parse::<i64>() else { return Ok(Json(Vec::new())) };
    let rows = entity::Entity::find()
        .filter(entity::Column::UserId.eq(user_id))
        .filter(entity::Column::ExpiresAt.gt(Utc::now()))
        .order_by_desc(entity::Column::CreatedAt)
        .limit(20)
        .all(&ctx.db)
        .await?;
    Ok(Json(rows.iter().map(view).collect()))
}

fn stale() -> AppError {
    AppError::Conflict {
        code: "export_stale",
        reason: "stale",
        message: "Your access has changed since this export was made; export again.",
    }
}

/// `GET exports/{id}/file`: the file, for its owner while they still may see it.
async fn download(State(app): State<AppState>, AdminCtx(ctx): AdminCtx, Path(id): Path<String>) -> AppResult<Response> {
    // Unknown, someone else's and expired exports are alike.
    let id = uuid::Uuid::parse_str(&id).map_err(|_| AppError::NotFound)?;
    let row = entity::Entity::find_by_id(id)
        .filter(entity::Column::UserId.eq(ctx.principal.id.parse::<i64>().unwrap_or(-1)))
        .filter(entity::Column::ExpiresAt.gt(Utc::now()))
        .one(&ctx.db)
        .await?
        .ok_or(AppError::NotFound)?;
    if row.status != "done" {
        return Err(AppError::Conflict {
            code: "export_not_ready",
            reason: "pending",
            message: "This export is not ready.",
        });
    }
    if ctx.principal.auth_version != Some(row.auth_version) {
        return Err(stale());
    }
    let resource = resource(&ctx, &row.resource)?;
    resource.admit_export(&ctx).await?;
    let columns: Vec<String> = serde_json::from_value(row.columns.clone()).map_err(AppError::internal)?;
    let readable: Vec<String> = resource.export_columns(&ctx).into_iter().map(|(n, _)| n).collect();
    if !columns.iter().all(|c| readable.contains(c)) {
        return Err(stale());
    }
    let query: ListQuery = serde_json::from_value(row.query.clone()).map_err(AppError::internal)?;
    let keys: Vec<String> =
        serde_json::from_value(row.keys.clone().unwrap_or(json!([]))).map_err(AppError::internal)?;
    // Every record in the file is one the user may still see.
    for chunk in keys.chunks(BATCH) {
        if resource.export_rows(&ctx, &query, chunk, &[]).await?.len() != chunk.len() {
            return Err(stale());
        }
    }
    let storage = storage(&app).ok_or(AppError::NotFound)?;
    if row.disk.as_deref() != Some(storage.disk()) {
        return Err(AppError::internal("export stored on another disk"));
    }
    let key = row.storage_key.as_deref().ok_or_else(|| AppError::internal("export without a file"))?;
    let (body, len) = storage.open(&StorageKey::parse(key)?).await?;
    audit_export(&ctx, &row.resource, keys.len(), &query, Some(&row.id.to_string())).await?;
    csv_response(body, &row.resource, Some(len))
}

/// Builds a background export (`admin.export`).
#[derive(Debug, Serialize, Deserialize)]
pub struct ExportJob {
    /// The export.
    pub id: uuid::Uuid,
}

/// Why an export cannot be made (a code its owner's panel translates; never retried).
struct Refused(&'static str);

async fn fail(db: &sea_orm::DatabaseConnection, id: uuid::Uuid, code: &str) -> AppResult<()> {
    entity::Entity::update_many()
        .col_expr(entity::Column::Status, Expr::value("failed"))
        .col_expr(entity::Column::Error, Expr::value(code))
        .filter(entity::Column::Id.eq(id))
        .filter(entity::Column::Status.is_in(["pending", "running"]))
        .exec(db)
        .await?;
    Ok(())
}

#[async_trait]
impl Job for ExportJob {
    const KIND: &'static str = "admin.export";

    fn max_attempts() -> u32 {
        ATTEMPTS
    }

    fn timeout() -> std::time::Duration {
        std::time::Duration::from_secs(600)
    }

    async fn handle(self, job: JobCtx) -> anyhow::Result<()> {
        let db = job.state.db().clone();
        let Some(row) = entity::Entity::find_by_id(self.id).one(&db).await? else { return Ok(()) };
        if row.status == "done" || row.status == "failed" {
            return Ok(());
        }
        // Delayed past its expiry: never started (pruning may be removing it).
        if row.expires_at <= Utc::now() {
            fail(&db, row.id, "unfinished").await?;
            return Ok(());
        }
        match build(&job, &row).await {
            Ok(()) => Ok(()),
            Err(Ok(Refused(code))) => {
                fail(&db, row.id, code).await?;
                Ok(())
            }
            Err(Err(err)) => {
                // The last run: the export says so (the queue keeps the job as failed).
                if job.attempt >= ATTEMPTS {
                    fail(&db, row.id, "failed").await?;
                }
                Err(err.into())
            }
        }
    }
}

/// The user of `row` as they are now: active, same credential version, with
/// their current permissions.
async fn principal(app: &AppState, row: &entity::Model) -> Result<Principal, Result<Refused, AppError>> {
    let db = app.db();
    let user = user::Entity::find_by_id(row.user_id).one(db).await.map_err(|e| Err(e.into()))?;
    let Some(user) = user.filter(|u| u.is_active && u.auth_version == row.auth_version) else {
        return Err(Ok(Refused("account_changed")));
    };
    let auth = app.service::<AuthState>().ok_or_else(|| Err(AppError::internal("auth not registered")))?;
    auth.resolver.principal(db, ADMIN_GUARD, &user, None).await.map_err(Err)
}

/// A refusal of the engine as a code for the owner; other errors are retried.
fn refusal(err: AppError) -> Result<Refused, AppError> {
    match err {
        AppError::Forbidden | AppError::NotFound | AppError::Validation(_) => Ok(Refused("revoked")),
        other => Err(other),
    }
}

/// A temporary file removed when dropped (also when the run is cancelled).
struct Staged(std::path::PathBuf);

impl Drop for Staged {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// The fields fixed for the export as the user may read them now (any lost: refused).
fn fixed_columns(
    resource: &dyn DynResource,
    ctx: &Ctx,
    fixed: &[String],
) -> Result<Vec<(String, CellKind)>, Result<Refused, AppError>> {
    let current = resource.export_columns(ctx);
    if !fixed.iter().all(|c| current.iter().any(|(n, _)| n == c)) {
        return Err(Ok(Refused("fields_changed")));
    }
    Ok(current.into_iter().filter(|(n, _)| fixed.contains(n)).collect())
}

fn internal(err: impl std::fmt::Display) -> Result<Refused, AppError> {
    Err(AppError::internal(err.to_string()))
}

async fn build(job: &JobCtx, row: &entity::Model) -> Result<(), Result<Refused, AppError>> {
    use tokio::io::AsyncWriteExt;
    let app = &job.state;
    let panel = app.service::<AdminPanel>().ok_or_else(|| internal("AdminProvider not registered"))?;
    let storage = storage(app).ok_or_else(|| internal("storage not registered"))?;
    let ctx_for = |principal: Principal| {
        Ctx::new(app.db().clone(), principal, panel.registry.clone()).with_audit(job.audit_context())
    };
    let ctx = ctx_for(principal(app, row).await?);
    let resource = resource(&ctx, &row.resource).map_err(refusal)?;
    resource.admit_export(&ctx).await.map_err(refusal)?;
    let fixed: Vec<String> = serde_json::from_value(row.columns.clone()).map_err(internal)?;
    let columns = fixed_columns(resource.as_ref(), &ctx, &fixed)?;
    let names: Vec<String> = columns.iter().map(|(n, _)| n.clone()).collect();
    let query: ListQuery = serde_json::from_value(row.query.clone()).map_err(internal)?;
    let keys = resource.export_keys(&ctx, &query, MAX_ROWS).await.map_err(refusal)?;
    entity::Entity::update_many()
        .col_expr(entity::Column::Status, Expr::value("running"))
        .filter(entity::Column::Id.eq(row.id))
        .filter(entity::Column::Status.eq("pending"))
        .exec(app.db())
        .await
        .map_err(|e| Err(e.into()))?;

    // Every run its own file and object (a run cancelled at shutdown may
    // still be copying when the job runs again).
    let staged = Staged(storage.staging_dir().join(format!("export-{}-{}.csv", row.id, uuid::Uuid::now_v7())));
    let mut file = tokio::fs::File::create(&staged.0).await.map_err(internal)?;
    let mut bytes = 0usize;
    let text = header_line(&columns);
    bytes += text.len();
    file.write_all(text.as_bytes()).await.map_err(internal)?;
    let mut written = Vec::with_capacity(keys.len());
    for chunk in keys.chunks(BATCH) {
        // The user again at each batch: revoked access or fields stop the export.
        let ctx = ctx_for(principal(app, row).await?);
        resource.admit_export(&ctx).await.map_err(refusal)?;
        let columns = fixed_columns(resource.as_ref(), &ctx, &fixed)?;
        for (key, record) in resource.export_rows(&ctx, &query, chunk, &names).await.map_err(refusal)? {
            let text = row_line(&columns, &record);
            bytes += text.len();
            if bytes > MAX_BYTES {
                return Err(Ok(Refused("too_large")));
            }
            file.write_all(text.as_bytes()).await.map_err(internal)?;
            // Keys of the rows written (records gone meanwhile are left out).
            written.push(key);
        }
    }
    file.flush().await.map_err(internal)?;
    drop(file);
    let key = StorageKey::generate("csv").map_err(|e| Err(e.into()))?;
    // Recorded before the upload: pruning deletes it whatever happens next.
    object::ActiveModel {
        storage_key: Set(key.as_str().to_owned()),
        export_id: Set(row.id),
        disk: Set(storage.disk().to_owned()),
        created_at: Set(Utc::now()),
    }
    .insert(app.db())
    .await
    .map_err(|e| Err(e.into()))?;
    storage.put_file(&key, &staged.0, "text/csv; charset=utf-8").await.map_err(|e| Err(e.into()))?;
    // File and records published together, by the first run to finish.
    let published = entity::Entity::update_many()
        .col_expr(entity::Column::Status, Expr::value("done"))
        .col_expr(entity::Column::Rows, Expr::value(i32::try_from(written.len()).unwrap_or(i32::MAX)))
        .col_expr(entity::Column::Keys, Expr::value(json!(written)))
        .col_expr(entity::Column::StorageKey, Expr::value(key.as_str()))
        .col_expr(entity::Column::Error, Expr::value(Option::<String>::None))
        .filter(entity::Column::Id.eq(row.id))
        .filter(entity::Column::Status.is_in(["pending", "running"]))
        .exec(app.db())
        .await
        .map_err(|e| Err(e.into()))?;
    if published.rows_affected == 0 {
        // Another run finished first (or the export failed meanwhile): this file is not used.
        storage.delete(&key).await.map_err(|e| Err(e.into()))?;
        object::Entity::delete_by_id(key.as_str().to_owned()).exec(app.db()).await.map_err(|e| Err(e.into()))?;
    }
    Ok(())
}

/// Deletes expired exports (rows and every run's file), fails exports whose
/// job never finished, and sweeps abandoned temporary files. Returns how many
/// exports were deleted.
pub async fn prune(db: &sea_orm::DatabaseConnection, storage: &dyn Storage) -> AppResult<u64> {
    let now = Utc::now();
    // Longer than every run of the job with its backoff.
    entity::Entity::update_many()
        .col_expr(entity::Column::Status, Expr::value("failed"))
        .col_expr(entity::Column::Error, Expr::value("unfinished"))
        .filter(entity::Column::Status.is_in(["pending", "running"]))
        .filter(entity::Column::CreatedAt.lt(now - chrono::Duration::hours(6)))
        .exec(db)
        .await?;
    // An hour after expiry: longer than a run of the job (a late upload has
    // finished, or was stopped), so every object it wrote is recorded.
    let expired =
        entity::Entity::find().filter(entity::Column::ExpiresAt.lte(now - PRUNE_GRACE)).limit(500).all(db).await?;
    let mut deleted = 0;
    for row in expired {
        // The published object and every run's (recorded before its upload).
        let objects = object::Entity::find().filter(object::Column::ExportId.eq(row.id)).all(db).await?;
        let mut keys: Vec<String> =
            objects.iter().filter(|o| o.disk == storage.disk()).map(|o| o.storage_key.clone()).collect();
        if row.disk.as_deref() == Some(storage.disk())
            && let Some(key) = &row.storage_key
            && !keys.contains(key)
        {
            keys.push(key.clone());
        }
        for key in keys {
            storage.delete(&StorageKey::parse(&key)?).await?;
        }
        object::Entity::delete_many().filter(object::Column::ExportId.eq(row.id)).exec(db).await?;
        entity::Entity::delete_by_id(row.id).exec(db).await?;
        deleted += 1;
    }
    storage.sweep(std::time::Duration::from_secs(3_600)).await?;
    Ok(deleted)
}
