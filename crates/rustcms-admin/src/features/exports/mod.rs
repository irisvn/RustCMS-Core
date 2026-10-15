//! Exports of resource lists as CSV (ADR 0021): the list as the user sees it
//! (scope, filters, search, sort, per-record policy), readable fields only,
//! text protected against spreadsheet formulas.
//!
//! - `GET {resource}/export?…`: at most [`SYNC_ROWS`] rows, built in full before
//!   anything is sent (too many: 422).
//! - `POST {resource}/exports?…`: in the background (a queued job; needs the
//!   storage provider). The job reloads its user at each batch (active, same
//!   credential version) and fixes the records it exported; the file is
//!   served only to that user, while they may still export the resource, read
//!   those fields and view every record in it.

mod csv;
pub mod entity;

use std::sync::Arc;

use async_trait::async_trait;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use chrono::Utc;
use rustcms_auth::{ADMIN_GUARD, AuthState};
use rustcms_core::audit::{self, AuditEntry};
use rustcms_core::models::user;
use rustcms_core::queue::{Job, JobCtx};
use rustcms_core::storage::{Storage, StorageKey};
use rustcms_core::{AppError, AppResult, AppState, Json, Principal};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::Data;
use crate::context::Ctx;
use crate::http::{AdminCtx, AdminPanel};
use crate::registry::DynResource;
use crate::resource::ListQuery;

/// Most rows of an export answered at once.
pub const SYNC_ROWS: usize = 1_000;
/// Most rows of a background export.
pub const MAX_ROWS: usize = 10_000;
/// Background exports a user may have waiting or running.
const PENDING_PER_USER: u64 = 3;
/// How long a background export's file is kept.
const KEEP: chrono::Duration = chrono::Duration::hours(24);
/// Rows read (and the user checked again) at a time by the job.
const BATCH: usize = 500;

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
    /// Why it failed.
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

/// The file's text: a header of field names, then one line per row.
fn render(columns: &[(String, bool)], rows: &[(String, Data)]) -> String {
    let mut out = csv::line(columns.iter().map(|(name, _)| csv::header(name)));
    for (_, row) in rows {
        out.push_str(&csv::line(
            columns.iter().map(|(name, text)| csv::cell(row.get(name).unwrap_or(&Value::Null), *text)),
        ));
    }
    out
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
    let rows = resource.export_rows(&ctx, &query, &keys, &names).await?;
    let body = render(&columns, &rows);
    audit_export(&ctx, &slug, rows.len(), &query, None).await?;
    let len = body.len() as u64;
    csv_response(body, &slug, Some(len))
}

/// `POST {resource}/exports?{list query}`: a background export.
async fn create(
    State(app): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Path(slug): Path<String>,
    Query(pairs): Query<Vec<(String, String)>>,
) -> AppResult<(StatusCode, Json<ExportView>)> {
    let resource = resource(&ctx, &slug)?;
    resource.admit_export(&ctx).await?;
    let storage = storage(&app).ok_or(AppError::NotFound)?;
    let query = resource.canonical_query(&ctx, &crate::http::list_query(pairs)?).await?;
    let user_id: i64 = ctx.principal.id.parse().map_err(|_| AppError::Forbidden)?;
    let auth_version = ctx.principal.auth_version.ok_or(AppError::Forbidden)?;
    let waiting = entity::Entity::find()
        .filter(entity::Column::UserId.eq(user_id))
        .filter(entity::Column::Status.is_in(["pending", "running"]))
        .count(&ctx.db)
        .await?;
    if waiting >= PENDING_PER_USER {
        return Err(AppError::TooManyRequests { retry_after: 60 });
    }
    let columns: Vec<String> = resource.export_columns(&ctx).into_iter().map(|(n, _)| n).collect();
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
        // The object's name is fixed now: a retried job writes the same one,
        // and pruning the row deletes it whatever happened.
        disk: Set(Some(storage.disk().to_owned())),
        storage_key: Set(Some(StorageKey::generate("csv")?.as_str().to_owned())),
        error: Set(None),
        created_at: Set(now),
        expires_at: Set(now + KEEP),
    };
    let txn = rustcms_core::db::begin_write(&ctx.db).await?;
    let row = row.insert(&txn).await?;
    ctx.dispatch(&txn, &ExportJob { id: row.id }).await?;
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
    if resource.export_rows(&ctx, &query, &keys, &[]).await?.len() != keys.len() {
        return Err(stale());
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

/// Why an export cannot be made (shown to its owner; never retried).
struct Refused(&'static str);

#[async_trait]
impl Job for ExportJob {
    const KIND: &'static str = "admin.export";

    fn max_attempts() -> u32 {
        3
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
        match build(&job, &row).await {
            Ok(()) => Ok(()),
            Err(Ok(Refused(message))) => {
                let mut failed: entity::ActiveModel = row.into();
                failed.status = Set("failed".into());
                failed.error = Set(Some(message.into()));
                failed.update(&db).await?;
                Ok(())
            }
            // Retried by the queue (the same object is written again).
            Err(Err(err)) => Err(err.into()),
        }
    }
}

/// The user of `row` as they are now: active, same credential version, with
/// their current permissions.
async fn principal(app: &AppState, row: &entity::Model) -> Result<Principal, Result<Refused, AppError>> {
    let db = app.db();
    let user = user::Entity::find_by_id(row.user_id).one(db).await.map_err(|e| Err(e.into()))?;
    let Some(user) = user.filter(|u| u.is_active && u.auth_version == row.auth_version) else {
        return Err(Ok(Refused("The account changed since this export was asked for.")));
    };
    let auth = app.service::<AuthState>().ok_or_else(|| Err(AppError::internal("auth not registered")))?;
    auth.resolver.principal(db, ADMIN_GUARD, &user, None).await.map_err(Err)
}

/// A refusal of the engine as a message for the owner; other errors are retried.
fn refusal(err: AppError) -> Result<Refused, AppError> {
    match err {
        AppError::Forbidden | AppError::NotFound => Ok(Refused("You may no longer export these records.")),
        AppError::Validation(_) => Ok(Refused("These records can no longer be exported this way; export again.")),
        other => Err(other),
    }
}

async fn build(job: &JobCtx, row: &entity::Model) -> Result<(), Result<Refused, AppError>> {
    let app = &job.state;
    let panel = app.service::<AdminPanel>().ok_or_else(|| Err(AppError::internal("AdminProvider not registered")))?;
    let storage = storage(app).ok_or_else(|| Err(AppError::internal("storage not registered")))?;
    let ctx_for = |principal: Principal| {
        Ctx::new(app.db().clone(), principal, panel.registry.clone()).with_audit(job.audit_context())
    };
    let ctx = ctx_for(principal(app, row).await?);
    let resource = resource(&ctx, &row.resource).map_err(refusal)?;
    resource.admit_export(&ctx).await.map_err(refusal)?;
    let columns: Vec<String> = serde_json::from_value(row.columns.clone()).map_err(|e| Err(AppError::internal(e)))?;
    let current = resource.export_columns(&ctx);
    if !columns.iter().all(|c| current.iter().any(|(n, _)| n == c)) {
        return Err(Ok(Refused("You may no longer read some of the exported fields.")));
    }
    let columns: Vec<(String, bool)> = current.into_iter().filter(|(n, _)| columns.contains(n)).collect();
    let names: Vec<String> = columns.iter().map(|(n, _)| n.clone()).collect();
    let query: ListQuery = serde_json::from_value(row.query.clone()).map_err(|e| Err(AppError::internal(e)))?;
    let keys = resource.export_keys(&ctx, &query, MAX_ROWS).await.map_err(refusal)?;
    let mut active: entity::ActiveModel = row.clone().into();
    active.status = Set("running".into());
    active.update(app.db()).await.map_err(|e| Err(e.into()))?;

    let staged = storage.staging_dir().join(format!("export-{}-{}.csv", row.id, job.attempt));
    let result = async {
        let mut file = tokio::fs::File::create(&staged).await.map_err(|e| Err(AppError::internal(e)))?;
        use tokio::io::AsyncWriteExt;
        file.write_all(csv::line(names.iter().map(|n| csv::header(n))).as_bytes())
            .await
            .map_err(|e| Err(AppError::internal(e)))?;
        let mut written = Vec::with_capacity(keys.len());
        for chunk in keys.chunks(BATCH) {
            // The user again at each batch: revoked access stops the export.
            let ctx = ctx_for(principal(app, row).await?);
            resource.admit_export(&ctx).await.map_err(refusal)?;
            let rows = resource.export_rows(&ctx, &query, chunk, &names).await.map_err(refusal)?;
            let mut text = String::new();
            for (_, record) in &rows {
                text.push_str(&csv::line(
                    columns.iter().map(|(name, t)| csv::cell(record.get(name).unwrap_or(&Value::Null), *t)),
                ));
            }
            file.write_all(text.as_bytes()).await.map_err(|e| Err(AppError::internal(e)))?;
            // Keys of the rows written (records gone meanwhile are left out).
            written.extend(rows.into_iter().map(|(key, _)| key));
        }
        file.flush().await.map_err(|e| Err(AppError::internal(e)))?;
        drop(file);
        let key = row.storage_key.as_deref().ok_or_else(|| Err(AppError::internal("export without a key")))?;
        storage
            .put_file(&StorageKey::parse(key).map_err(|e| Err(e.into()))?, &staged, "text/csv; charset=utf-8")
            .await
            .map_err(|e| Err(e.into()))?;
        Ok::<_, Result<Refused, AppError>>(written)
    }
    .await;
    let _ = tokio::fs::remove_file(&staged).await;
    let written = result?;
    let mut done: entity::ActiveModel = row.clone().into();
    done.status = Set("done".into());
    done.rows = Set(i32::try_from(written.len()).unwrap_or(i32::MAX));
    done.keys = Set(Some(json!(written)));
    done.error = Set(None);
    done.update(app.db()).await.map_err(|e| Err(e.into()))?;
    Ok(())
}

/// Deletes expired exports (rows and files), and marks exports whose job
/// never finished as failed. Returns how many were deleted.
pub async fn prune(db: &sea_orm::DatabaseConnection, storage: &dyn Storage) -> AppResult<u64> {
    let now = Utc::now();
    entity::Entity::update_many()
        .col_expr(entity::Column::Status, sea_orm::sea_query::Expr::value("failed"))
        .col_expr(entity::Column::Error, sea_orm::sea_query::Expr::value("The export did not finish."))
        .filter(entity::Column::Status.is_in(["pending", "running"]))
        .filter(entity::Column::CreatedAt.lt(now - chrono::Duration::hours(2)))
        .exec(db)
        .await?;
    let expired = entity::Entity::find().filter(entity::Column::ExpiresAt.lte(now)).limit(500).all(db).await?;
    let mut deleted = 0;
    for row in expired {
        if let (Some(key), Some(disk)) = (&row.storage_key, &row.disk)
            && disk == storage.disk()
        {
            storage.delete(&StorageKey::parse(key)?).await?;
        }
        entity::Entity::delete_by_id(row.id).exec(db).await?;
        deleted += 1;
    }
    Ok(deleted)
}
