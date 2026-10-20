//! The Queue page (System → Queue, ADR 0016): jobs by status for
//! `queue.view`, and retry / forget / cancel of one job for those who also
//! hold `queue.manage`. Statuses and conditions are the workers' own
//! (`rustcms_core::queue::manage`); payloads and worker tokens are never sent.

use std::collections::HashMap;

use axum::Router;
use axum::extract::{Path, RawQuery, State};
use axum::http::Method;
use axum::routing::{get, post};
use chrono::{DateTime, Utc};
use rustcms_core::audit::AuditEntry;
use rustcms_core::models::{job, job_run, user};
use rustcms_core::queue::manage::{self, Refusal};
use rustcms_core::queue::{Clock, JobStatus, QueueConfig};
use rustcms_core::{AppError, AppResult, AppState, Json, ValidationErrors};
use sea_orm::sea_query::Condition;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use serde::Serialize;

use crate::context::Ctx;
use crate::http::AdminCtx;

/// See the Queue page.
pub const VIEW: &str = "queue.view";
/// Retry, forget and cancel jobs (with [`VIEW`]).
pub const MANAGE: &str = "queue.manage";

const MAX_PER_PAGE: u64 = 100;
const DEFAULT_PER_PAGE: u64 = 50;
/// `last_error` as sent (it is a safe message already, kept short).
const MAX_ERROR_CHARS: usize = 2_000;

/// Every queue route as `(method, path)`.
pub(crate) fn routes(admin_path: &str) -> Vec<(Method, String)> {
    let base = format!("{}/api", admin_path.trim_end_matches('/'));
    vec![
        (Method::GET, format!("{base}/queue")),
        (Method::GET, format!("{base}/queue/jobs")),
        (Method::GET, format!("{base}/queue/history")),
        (Method::GET, format!("{base}/queue/jobs/{{id}}")),
        (Method::POST, format!("{base}/queue/jobs/{{id}}/retry")),
        (Method::POST, format!("{base}/queue/jobs/{{id}}/forget")),
        (Method::POST, format!("{base}/queue/jobs/{{id}}/cancel")),
    ]
}

pub(crate) fn router(router: Router<AppState>, admin_path: &str) -> Router<AppState> {
    let base = format!("{}/api", admin_path.trim_end_matches('/'));
    router
        .route(&format!("{base}/queue"), get(overview))
        .route(&format!("{base}/queue/jobs"), get(jobs))
        .route(&format!("{base}/queue/history"), get(history))
        .route(&format!("{base}/queue/jobs/{{id}}"), get(detail))
        .route(&format!("{base}/queue/jobs/{{id}}/retry"), post(retry))
        .route(&format!("{base}/queue/jobs/{{id}}/forget"), post(forget))
        .route(&format!("{base}/queue/jobs/{{id}}/cancel"), post(cancel))
}

/// The queue permissions (group `queue`).
pub(crate) fn permissions() -> [&'static str; 2] {
    [VIEW, MANAGE]
}

/// Jobs of one queue, by status.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct QueueStats {
    /// Queue name.
    pub name: String,
    /// Run as soon as a worker is free.
    pub pending: u64,
    /// Wait for their time (a backoff, a delayed dispatch).
    pub scheduled: u64,
    /// Owned by a worker.
    pub running: u64,
    /// Last attempt's lease ended without an answer: failed at the next worker pass.
    pub stalled: u64,
    /// Will not run again unless retried.
    pub failed: u64,
}

/// What the workers of this instance do.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct QueueWorkers {
    /// `queue.enabled`: `serve` runs workers.
    pub enabled: bool,
    /// Workers per process.
    pub workers: u64,
    /// Queues they take jobs from, in order.
    pub queues: Vec<String>,
}

/// `GET queue`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct QueueOverview {
    /// Queues with jobs, by name.
    pub queues: Vec<QueueStats>,
    /// This instance's worker settings.
    pub workers: QueueWorkers,
    /// May retry, forget and cancel (`queue.manage`).
    pub can_manage: bool,
    /// Jobs finished in the last 24 hours (from the history).
    pub last_day: QueueLastDay,
}

/// Jobs finished in the last 24 hours.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct QueueLastDay {
    /// Done.
    pub done: u64,
    /// Failed for good.
    pub failed: u64,
}

/// A finished job (`job_runs`): a job retried and finished again has several.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct JobRun {
    /// This record's id (UUID).
    pub id: String,
    /// The job's id (UUID).
    pub job_id: String,
    /// Queue name.
    pub queue: String,
    /// `Job::KIND`.
    pub kind: String,
    /// `Job::describe`.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// `done`, `failed` or `cancelled`.
    pub status: String,
    /// Runs used.
    pub attempts: i32,
    /// Dispatched at (RFC 3339).
    pub created_at: String,
    /// Its last run started at (RFC 3339).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    /// Finished at (RFC 3339).
    pub finished_at: String,
    /// How long its last run took.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i64>,
    /// Why it failed (a safe message).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    /// The user who dispatched it.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initiated_by: Option<QueueUser>,
}

/// A page of finished jobs, newest first.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct JobRunPage {
    /// Finished jobs.
    pub data: Vec<JobRun>,
    /// Continuation.
    pub meta: QueueJobPageMeta,
}

/// `GET queue/jobs/{id}`: the job if it still exists, and its history.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct QueueJobDetail {
    /// The job, while it exists.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job: Option<QueueJob>,
    /// Its finished runs, newest first (at most 20).
    pub runs: Vec<JobRun>,
}

/// Who dispatched a job.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct QueueUser {
    /// User id (string).
    pub id: String,
    /// Name, for viewers who may list users.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// One job as the page shows it: never its payload or its worker's token.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct QueueJob {
    /// Job id (UUID).
    pub id: String,
    /// Queue name.
    pub queue: String,
    /// `Job::KIND`.
    pub kind: String,
    /// `Job::VERSION` of its payload.
    pub version: i32,
    /// `pending`, `scheduled` (shown as Delayed), `running`, `stalled` or `failed`.
    pub status: String,
    /// `Job::describe`.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Its current run started at (running jobs).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    /// Runs so far.
    pub attempts: i32,
    /// Most runs.
    pub max_attempts: i32,
    /// Not run before (RFC 3339).
    pub available_at: String,
    /// Its worker owns it until (running jobs).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reserved_until: Option<String>,
    /// Why the last run failed (a safe message).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    /// Dispatched at (RFC 3339).
    pub created_at: String,
    /// Failed at (RFC 3339).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed_at: Option<String>,
    /// The user who dispatched it.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initiated_by: Option<QueueUser>,
}

/// A page of jobs, newest first.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct QueueJobPage {
    /// Jobs.
    pub data: Vec<QueueJob>,
    /// Continuation.
    pub meta: QueueJobPageMeta,
}

/// Where the next page starts.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct QueueJobPageMeta {
    /// Pass as `cursor` for older jobs; absent on the last page.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

fn clock(state: &AppState) -> AppResult<Clock> {
    let config = QueueConfig::of(state.config()).map_err(AppError::internal)?;
    Ok(Clock::of(&config))
}

/// `GET queue`: counts per queue and status.
async fn overview(State(state): State<AppState>, AdminCtx(ctx): AdminCtx) -> AppResult<Json<QueueOverview>> {
    if !ctx.can(VIEW) {
        return Err(AppError::Forbidden);
    }
    let config = QueueConfig::of(state.config()).map_err(AppError::internal)?;
    let counts = manage::counts(&ctx.db, &Clock::of(&config)).await?;
    let queues = counts
        .into_iter()
        .map(|(name, c)| QueueStats {
            name,
            pending: c.pending,
            scheduled: c.scheduled,
            running: c.running,
            stalled: c.stalled,
            failed: c.failed,
        })
        .collect();
    Ok(Json(QueueOverview {
        queues,
        workers: QueueWorkers {
            enabled: config.enabled,
            workers: config.workers as u64,
            queues: config.queues.clone(),
        },
        can_manage: ctx.can(MANAGE),
        last_day: last_day(&ctx).await?,
    }))
}

async fn last_day(ctx: &Ctx) -> AppResult<QueueLastDay> {
    use sea_orm::PaginatorTrait;
    let since = Utc::now() - chrono::Duration::hours(24);
    let count = |status: &'static str| {
        job_run::Entity::find()
            .filter(job_run::Column::FinishedAt.gte(since))
            .filter(job_run::Column::Status.eq(status))
            .count(&ctx.db)
    };
    Ok(QueueLastDay { done: count("done").await?, failed: count("failed").await? })
}

/// Filters of `GET queue/jobs`, parsed after admission.
#[derive(Default)]
struct Filters {
    status: Option<JobStatus>,
    /// `done`, `failed` or `cancelled` (the history).
    finished: Option<&'static str>,
    queue: Option<String>,
    kind: Option<String>,
    since: Option<DateTime<Utc>>,
    until: Option<DateTime<Utc>>,
    cursor: Option<(DateTime<Utc>, uuid::Uuid)>,
    per_page: u64,
}

fn parse(query: Option<&str>) -> AppResult<Filters> {
    parse_for(query, false)
}

/// The filters of the jobs list, or of the history (`history`: finished statuses).
fn parse_for(query: Option<&str>, history: bool) -> AppResult<Filters> {
    let mut filters = Filters { per_page: DEFAULT_PER_PAGE, ..Default::default() };
    let mut errors = ValidationErrors::new();
    let mut seen = std::collections::HashSet::new();
    for (name, value) in form_urlencoded::parse(query.unwrap_or_default().as_bytes()) {
        let name = name.into_owned();
        if !["status", "queue", "kind", "since", "until", "cursor", "per_page"].contains(&name.as_str()) {
            errors.add(name.as_str(), "is not allowed");
            continue;
        }
        if !seen.insert(name.clone()) {
            errors.add(name.as_str(), "may only be given once");
            continue;
        }
        let text = |errors: &mut ValidationErrors| {
            if value.is_empty() || value.chars().count() > 64 {
                errors.add(name.as_str(), "may not be longer than 64 characters");
                None
            } else {
                Some(value.to_string())
            }
        };
        match name.as_str() {
            "status" if history => match value.as_ref() {
                "done" => filters.finished = Some("done"),
                "failed" => filters.finished = Some("failed"),
                "cancelled" => filters.finished = Some("cancelled"),
                _ => errors.add(name.as_str(), "is not one of the options"),
            },
            "status" => match JobStatus::parse(&value) {
                Some(status) => filters.status = Some(status),
                None => errors.add(name.as_str(), "is not one of the options"),
            },
            "since" | "until" => match DateTime::parse_from_rfc3339(&value) {
                Ok(at) if name == "since" => filters.since = Some(at.with_timezone(&Utc)),
                Ok(at) => filters.until = Some(at.with_timezone(&Utc)),
                Err(_) => errors.add(name.as_str(), "must be a date and time (RFC 3339)"),
            },
            "queue" => filters.queue = text(&mut errors),
            "kind" => filters.kind = text(&mut errors),
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
    if let (Some(since), Some(until)) = (filters.since, filters.until)
        && since > until
    {
        errors.add("until", "must be after since");
    }
    errors.into_result()?;
    Ok(filters)
}

fn encode_cursor(row: &job::Model) -> String {
    // The exact stored instant (as the audit log's cursor).
    hex::encode(format!("{}|{}", row.created_at.timestamp_nanos_opt().unwrap_or_default(), row.id))
}

fn decode_cursor(raw: &str) -> Option<(DateTime<Utc>, uuid::Uuid)> {
    if raw.len() > 128 {
        return None;
    }
    let text = String::from_utf8(hex::decode(raw).ok()?).ok()?;
    let (nanos, id) = text.split_once('|')?;
    Some((DateTime::from_timestamp_nanos(nanos.parse().ok()?), id.parse().ok()?))
}

/// `GET queue/jobs`: jobs, newest first, filtered in SQL.
async fn jobs(
    State(state): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    RawQuery(query): RawQuery,
) -> AppResult<Json<QueueJobPage>> {
    // Admission first: a caller without access learns nothing from 422s.
    if !ctx.can(VIEW) {
        return Err(AppError::Forbidden);
    }
    let filters = parse(query.as_deref())?;
    let clock = clock(&state)?;
    let mut select = job::Entity::find();
    if let Some(status) = filters.status {
        select = select.filter(status.condition(&clock));
    }
    if let Some(queue) = &filters.queue {
        select = select.filter(job::Column::Queue.eq(queue.as_str()));
    }
    if let Some(kind) = &filters.kind {
        select = select.filter(job::Column::Kind.eq(kind.as_str()));
    }
    if let Some(since) = filters.since {
        select = select.filter(job::Column::CreatedAt.gte(since));
    }
    if let Some(until) = filters.until {
        select = select.filter(job::Column::CreatedAt.lte(until));
    }
    if let Some((at, id)) = filters.cursor {
        select = select.filter(
            Condition::any()
                .add(job::Column::CreatedAt.lt(at))
                .add(Condition::all().add(job::Column::CreatedAt.eq(at)).add(job::Column::Id.lt(id))),
        );
    }
    let mut rows = select
        .order_by_desc(job::Column::CreatedAt)
        .order_by_desc(job::Column::Id)
        .limit(filters.per_page + 1)
        .all(&ctx.db)
        .await?;
    let next_cursor = (rows.len() as u64 > filters.per_page)
        .then(|| {
            rows.truncate(filters.per_page as usize);
            rows.last().map(encode_cursor)
        })
        .flatten();
    let names = names(&ctx, rows.iter().filter_map(|r| r.initiated_by)).await?;
    let data = rows.into_iter().map(|row| view(row, &clock, &names)).collect();
    Ok(Json(QueueJobPage { data, meta: QueueJobPageMeta { next_cursor } }))
}

fn run_cursor(row: &job_run::Model) -> String {
    hex::encode(format!("{}|{}", row.finished_at.timestamp_nanos_opt().unwrap_or_default(), row.id))
}

/// `GET queue/history`: finished jobs, newest first.
async fn history(AdminCtx(ctx): AdminCtx, RawQuery(query): RawQuery) -> AppResult<Json<JobRunPage>> {
    if !ctx.can(VIEW) {
        return Err(AppError::Forbidden);
    }
    let filters = parse_for(query.as_deref(), true)?;
    let mut select = job_run::Entity::find();
    if let Some(status) = filters.finished {
        select = select.filter(job_run::Column::Status.eq(status));
    }
    if let Some(queue) = &filters.queue {
        select = select.filter(job_run::Column::Queue.eq(queue.as_str()));
    }
    if let Some(kind) = &filters.kind {
        select = select.filter(job_run::Column::Kind.eq(kind.as_str()));
    }
    if let Some(since) = filters.since {
        select = select.filter(job_run::Column::FinishedAt.gte(since));
    }
    if let Some(until) = filters.until {
        select = select.filter(job_run::Column::FinishedAt.lte(until));
    }
    if let Some((at, id)) = filters.cursor {
        select = select.filter(
            Condition::any()
                .add(job_run::Column::FinishedAt.lt(at))
                .add(Condition::all().add(job_run::Column::FinishedAt.eq(at)).add(job_run::Column::Id.lt(id))),
        );
    }
    let mut rows = select
        .order_by_desc(job_run::Column::FinishedAt)
        .order_by_desc(job_run::Column::Id)
        .limit(filters.per_page + 1)
        .all(&ctx.db)
        .await?;
    let next_cursor = (rows.len() as u64 > filters.per_page)
        .then(|| {
            rows.truncate(filters.per_page as usize);
            rows.last().map(run_cursor)
        })
        .flatten();
    let names = names(&ctx, rows.iter().filter_map(|r| r.initiated_by)).await?;
    let data = rows.into_iter().map(|row| run_view(row, &names)).collect();
    Ok(Json(JobRunPage { data, meta: QueueJobPageMeta { next_cursor } }))
}

/// `GET queue/jobs/{id}`: the job, if it still exists, and its history.
async fn detail(
    State(state): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Path(raw): Path<String>,
) -> AppResult<Json<QueueJobDetail>> {
    if !ctx.can(VIEW) {
        return Err(AppError::Forbidden);
    }
    let Ok(id) = uuid::Uuid::parse_str(&raw) else { return Err(AppError::NotFound) };
    let job = job::Entity::find_by_id(id).one(&ctx.db).await?;
    let runs = job_run::Entity::find()
        .filter(job_run::Column::JobId.eq(id))
        .order_by_desc(job_run::Column::FinishedAt)
        .order_by_desc(job_run::Column::Id)
        .limit(20)
        .all(&ctx.db)
        .await?;
    if job.is_none() && runs.is_empty() {
        return Err(AppError::NotFound);
    }
    let users = job.iter().filter_map(|j| j.initiated_by).chain(runs.iter().filter_map(|r| r.initiated_by));
    let names = names(&ctx, users).await?;
    let clock = clock(&state)?;
    Ok(Json(QueueJobDetail {
        job: job.map(|row| view(row, &clock, &names)),
        runs: runs.into_iter().map(|row| run_view(row, &names)).collect(),
    }))
}

fn run_view(row: job_run::Model, names: &HashMap<i64, String>) -> JobRun {
    JobRun {
        id: row.id.to_string(),
        job_id: row.job_id.to_string(),
        queue: row.queue,
        kind: row.kind,
        description: row.description,
        status: row.status,
        attempts: row.attempts,
        created_at: instant(row.created_at),
        started_at: row.started_at.map(instant),
        finished_at: instant(row.finished_at),
        duration_ms: row.duration_ms,
        last_error: row.last_error.map(|e| e.chars().take(MAX_ERROR_CHARS).collect()),
        initiated_by: row.initiated_by.map(|id| QueueUser { id: id.to_string(), name: names.get(&id).cloned() }),
    }
}

/// Names of the dispatching users, for viewers who may list users.
async fn names(ctx: &Ctx, users: impl Iterator<Item = i64>) -> AppResult<HashMap<i64, String>> {
    if !ctx.can("users.view_any") {
        return Ok(HashMap::new());
    }
    let mut ids: Vec<i64> = users.collect();
    ids.sort_unstable();
    ids.dedup();
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let users = user::Entity::find().filter(user::Column::Id.is_in(ids)).all(&ctx.db).await?;
    Ok(users.into_iter().map(|u| (u.id, u.name)).collect())
}

fn instant(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

fn view(row: job::Model, clock: &Clock, names: &HashMap<i64, String>) -> QueueJob {
    let status = JobStatus::of(&row, clock);
    QueueJob {
        id: row.id.to_string(),
        status: status.as_str().to_owned(),
        reserved_until: row.reserved_until.filter(|_| status == JobStatus::Running).map(instant),
        started_at: row.started_at.filter(|_| status == JobStatus::Running).map(instant),
        description: row.description,
        last_error: row.last_error.map(|e| e.chars().take(MAX_ERROR_CHARS).collect()),
        initiated_by: row.initiated_by.map(|id| QueueUser { id: id.to_string(), name: names.get(&id).cloned() }),
        available_at: instant(row.available_at),
        created_at: instant(row.created_at),
        failed_at: row.failed_at.map(instant),
        queue: row.queue,
        kind: row.kind,
        version: row.version,
        attempts: row.attempts,
        max_attempts: row.max_attempts,
    }
}

/// What a job action records.
#[derive(Serialize)]
struct JobMeta<'a> {
    kind: &'a str,
    queue: &'a str,
}

/// Runs `action` on job `id` for a user holding both queue permissions now
/// (re-read under the RBAC lock), recording `event` in the same transaction.
async fn act(
    state: &AppState,
    ctx: &Ctx,
    raw_id: &str,
    event: &'static str,
    action: impl AsyncFnOnce(&sea_orm::DatabaseTransaction, uuid::Uuid, &Clock) -> AppResult<Result<(), Refusal>>,
) -> AppResult<axum::http::StatusCode> {
    // Admission before the id is even parsed, and again with the grants as
    // they are now (under the RBAC lock): no probing without access.
    if !(ctx.can(VIEW) && ctx.can(MANAGE)) {
        return Err(AppError::Forbidden);
    }
    let (txn, current) = rustcms_auth::rbac::begin_as(state.db(), &ctx.principal).await?;
    if !(current.can(VIEW) && current.can(MANAGE)) {
        return Err(AppError::Forbidden);
    }
    let Ok(id) = uuid::Uuid::parse_str(raw_id) else {
        txn.rollback().await?;
        return Err(AppError::NotFound);
    };
    let clock = clock(state)?;
    // Read before the write, for the audit entry (the write's own condition decides).
    let before = job::Entity::find_by_id(id).one(&txn).await?;
    match action(&txn, id, &clock).await? {
        Ok(()) => {
            let row = before.ok_or_else(|| AppError::internal("job vanished during its own write"))?;
            let entry = AuditEntry::new(event, "job").key(id).meta(JobMeta { kind: &row.kind, queue: &row.queue });
            ctx.audit(&txn, entry).await?;
            txn.commit().await?;
            Ok(axum::http::StatusCode::NO_CONTENT)
        }
        Err(refusal) => {
            txn.rollback().await?;
            Err(match refusal {
                Refusal::Missing => AppError::NotFound,
                Refusal::NotFailed => conflict("not_failed", "Only failed jobs can be retried or forgotten."),
                Refusal::Running => conflict("running", "A worker is running this job; wait until it ends."),
                Refusal::NotWaiting => conflict("not_waiting", "Only jobs waiting to run can be cancelled."),
            })
        }
    }
}

fn conflict(reason: &'static str, message: &'static str) -> AppError {
    AppError::Conflict { code: "write_conflict", reason, message }
}

/// `POST queue/jobs/{id}/retry`: a failed job runs again (attempts start over).
async fn retry(
    State(state): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Path(id): Path<String>,
) -> AppResult<axum::http::StatusCode> {
    act(&state, &ctx, &id, "queue.retried", async |txn, id, clock| manage::retry(txn, id, clock).await).await
}

/// `POST queue/jobs/{id}/forget`: a failed job is deleted.
async fn forget(
    State(state): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Path(id): Path<String>,
) -> AppResult<axum::http::StatusCode> {
    act(&state, &ctx, &id, "queue.forgotten", async |txn, id, clock| manage::forget(txn, id, clock).await).await
}

/// `POST queue/jobs/{id}/cancel`: a job waiting to run is deleted.
async fn cancel(
    State(state): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Path(id): Path<String>,
) -> AppResult<axum::http::StatusCode> {
    act(&state, &ctx, &id, "queue.cancelled", async |txn, id, clock| manage::cancel(txn, id, clock).await).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursors_round_trip_and_reject_garbage() {
        let at = DateTime::from_timestamp_micros(1_760_000_000_123_456).unwrap();
        let id = uuid::Uuid::now_v7();
        let raw = hex::encode(format!("{}|{id}", at.timestamp_nanos_opt().unwrap()));
        assert_eq!(decode_cursor(&raw), Some((at, id)));
        for bad in ["", "!!", "7878", &"a".repeat(200)] {
            assert!(decode_cursor(bad).is_none(), "{bad}");
        }
        assert!(parse(Some("status=nope")).is_err());
        assert!(parse(Some("status=failed&status=failed")).is_err());
        assert!(parse(Some("payload=1")).is_err());
        assert_eq!(parse(Some("status=stalled&per_page=5")).unwrap().per_page, 5);
    }
}
