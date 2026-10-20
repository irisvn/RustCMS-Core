//! The Schedules page (System → Schedules, ADR 0031): each schedule's rule,
//! next and last run, and its run history for `schedules.view`; Run now for
//! those who also hold `schedules.manage` (recorded in the audit log).

use axum::Router;
use axum::extract::{Path, RawQuery, State};
use axum::http::{Method, StatusCode};
use axum::routing::{get, post};
use chrono::{DateTime, Utc};
use rustcms_core::audit::AuditEntry;
use rustcms_core::models::schedule_run;
use rustcms_core::schedule::{self, RunNowError, ScheduleAction, ScheduleConfig, Schedules};
use rustcms_core::{AppError, AppResult, AppState, Json, ValidationErrors};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
use serde::Serialize;

use crate::http::AdminCtx;

/// See the Schedules page.
pub const VIEW: &str = "schedules.view";
/// Run a schedule now (with [`VIEW`]).
pub const MANAGE: &str = "schedules.manage";

const MAX_PER_PAGE: u64 = 100;
const DEFAULT_PER_PAGE: u64 = 20;
/// A `running` run older than its timeout plus this is shown as "completion unknown".
const STALE_AFTER: chrono::Duration = chrono::Duration::hours(1);

/// Every schedules route as `(method, path)`.
pub(crate) fn routes(admin_path: &str) -> Vec<(Method, String)> {
    let base = format!("{}/api", admin_path.trim_end_matches('/'));
    vec![
        (Method::GET, format!("{base}/schedules")),
        (Method::GET, format!("{base}/schedules/{{name}}/runs")),
        (Method::POST, format!("{base}/schedules/{{name}}/run")),
    ]
}

pub(crate) fn router(router: Router<AppState>, admin_path: &str) -> Router<AppState> {
    let base = format!("{}/api", admin_path.trim_end_matches('/'));
    router
        .route(&format!("{base}/schedules"), get(list))
        .route(&format!("{base}/schedules/{{name}}/runs"), get(runs))
        .route(&format!("{base}/schedules/{{name}}/run"), post(run_now))
}

/// The schedules permissions (group `schedules`).
pub(crate) fn permissions() -> [&'static str; 2] {
    [VIEW, MANAGE]
}

/// What a schedule does.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ScheduleActionInfo {
    /// Enqueues a job.
    #[serde(rename_all = "camelCase")]
    Job {
        /// Its kind.
        job_kind: String,
    },
    /// Runs a closure (`.run` or `.task`).
    Closure,
}

/// One run of a schedule.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ScheduleRunInfo {
    /// Run id (UUID).
    pub id: String,
    /// The slot it ran for (scheduled runs).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot_at: Option<String>,
    /// `schedule` or `manual`.
    pub trigger: String,
    /// Started (or was dispatched) at.
    pub started_at: String,
    /// Finished at.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    /// `dispatched`, `running`, `succeeded`, `failed`, `skipped`, `cancelled` or
    /// `unknown` (also a `running` run past its time: its end was not recorded).
    pub status: String,
    /// How long it ran.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i64>,
    /// A task's summary.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// Why it failed (a safe message).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The job that ran it.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
    /// The user who ran it (id).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initiated_by: Option<String>,
}

/// One schedule as the page shows it.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ScheduleInfo {
    /// Name.
    pub name: String,
    /// When it fires, in words.
    pub rule: String,
    /// What it does.
    pub action: ScheduleActionInfo,
    /// Slots wait for the previous job.
    pub without_overlapping: bool,
    /// Next slot (RFC 3339).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_at: Option<String>,
    /// Last claimed slot.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_slot_at: Option<String>,
    /// Its latest run.
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last: Option<ScheduleRunInfo>,
}

/// Whether this instance runs the scheduler.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SchedulerInfo {
    /// `schedule.enabled`.
    pub enabled: bool,
    /// `schedule.grace_secs`.
    pub grace_secs: u64,
}

/// `GET schedules`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ScheduleList {
    /// Every schedule, as declared.
    pub data: Vec<ScheduleInfo>,
    /// The scheduler on this instance.
    pub scheduler: SchedulerInfo,
    /// Workers run jobs on this instance (`queue.enabled`).
    pub workers: bool,
    /// May run schedules now (`schedules.manage`).
    pub can_manage: bool,
}

/// A page of runs, newest first.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ScheduleRunPage {
    /// Runs.
    pub data: Vec<ScheduleRunInfo>,
    /// Continuation.
    pub meta: crate::queue::QueueJobPageMeta,
}

/// `POST schedules/{name}/run`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ScheduleRunResponse {
    /// The run, dispatched.
    pub run: ScheduleRunInfo,
}

fn instant(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

fn info(row: schedule_run::Model, timeout: std::time::Duration) -> ScheduleRunInfo {
    // A run whose end was never recorded (a crash) is not shown as running forever.
    let window = chrono::Duration::from_std(timeout).unwrap_or(chrono::Duration::MAX) + STALE_AFTER;
    let stale = matches!(row.status.as_str(), "running" | "dispatched")
        && row.job_id.is_none()
        && Utc::now() - row.started_at > window;
    ScheduleRunInfo {
        id: row.id.to_string(),
        slot_at: row.slot_at.map(instant),
        trigger: row.trigger,
        started_at: instant(row.started_at),
        finished_at: row.finished_at.map(instant),
        status: if stale { "unknown".to_owned() } else { row.status },
        duration_ms: row.duration_ms,
        summary: row.summary,
        error: row.error,
        job_id: row.job_id.map(|id| id.to_string()),
        initiated_by: row.initiated_by.map(|id| id.to_string()),
    }
}

fn schedules(state: &AppState) -> Schedules {
    state.service::<Schedules>().map(|s| (*s).clone()).unwrap_or_default()
}

/// `GET schedules`: every schedule with its next and last run.
async fn list(State(state): State<AppState>, AdminCtx(ctx): AdminCtx) -> AppResult<Json<ScheduleList>> {
    if !ctx.can(VIEW) {
        return Err(AppError::Forbidden);
    }
    let config = ScheduleConfig::of(state.config()).map_err(AppError::internal)?;
    let queue = rustcms_core::queue::QueueConfig::of(state.config()).map_err(AppError::internal)?;
    let states = schedule::states(&ctx.db).await?;
    let now = Utc::now();
    let all = schedules(&state);
    let mut data = Vec::new();
    for s in all.all() {
        let last = schedule_run::Entity::find()
            .filter(schedule_run::Column::Name.eq(s.name()))
            .order_by_desc(schedule_run::Column::StartedAt)
            .order_by_desc(schedule_run::Column::Id)
            .one(&ctx.db)
            .await?;
        let state_row = states.iter().find(|row| row.name == s.name());
        data.push(ScheduleInfo {
            name: s.name().to_owned(),
            rule: s.describe(),
            action: match s.action() {
                ScheduleAction::Job(kind) => ScheduleActionInfo::Job { job_kind: kind.to_owned() },
                ScheduleAction::Closure => ScheduleActionInfo::Closure,
            },
            without_overlapping: s.is_without_overlapping(),
            next_at: s.next_after(now).map(instant),
            last_slot_at: state_row.and_then(|row| row.last_slot_at).map(instant),
            last: last.map(|row| info(row, s.run_timeout())),
        });
    }
    Ok(Json(ScheduleList {
        data,
        scheduler: SchedulerInfo { enabled: config.enabled, grace_secs: config.grace_secs },
        workers: queue.enabled && queue.workers > 0,
        can_manage: ctx.can(MANAGE),
    }))
}

/// Where a page of runs starts: `(started_at, id)` of the last run seen.
type Cursor = (DateTime<Utc>, uuid::Uuid);

fn parse(query: Option<&str>) -> AppResult<(Option<Cursor>, u64)> {
    let (mut cursor, mut per_page) = (None, DEFAULT_PER_PAGE);
    let mut errors = ValidationErrors::new();
    let mut seen = std::collections::HashSet::new();
    for (name, value) in form_urlencoded::parse(query.unwrap_or_default().as_bytes()) {
        let name = name.into_owned();
        if !seen.insert(name.clone()) {
            errors.add(name.as_str(), "may only be given once");
            continue;
        }
        match name.as_str() {
            "per_page" => match value.parse::<u64>() {
                Ok(n) if (1..=MAX_PER_PAGE).contains(&n) => per_page = n,
                _ => errors.add(name.as_str(), format!("must be a number from 1 to {MAX_PER_PAGE}")),
            },
            "cursor" => match decode_cursor(&value) {
                Some(c) => cursor = Some(c),
                None => errors.add(name.as_str(), "is invalid"),
            },
            _ => errors.add(name.as_str(), "is not allowed"),
        }
    }
    errors.into_result()?;
    Ok((cursor, per_page))
}

fn decode_cursor(raw: &str) -> Option<Cursor> {
    if raw.len() > 128 {
        return None;
    }
    let text = String::from_utf8(hex::decode(raw).ok()?).ok()?;
    let (nanos, id) = text.split_once('|')?;
    Some((DateTime::from_timestamp_nanos(nanos.parse().ok()?), id.parse().ok()?))
}

/// `GET schedules/{name}/runs`: the runs of one schedule, newest first.
async fn runs(
    State(state): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Path(name): Path<String>,
    RawQuery(query): RawQuery,
) -> AppResult<Json<ScheduleRunPage>> {
    if !ctx.can(VIEW) {
        return Err(AppError::Forbidden);
    }
    let all = schedules(&state);
    let Some(s) = all.get(&name) else { return Err(AppError::NotFound) };
    let (cursor, per_page) = parse(query.as_deref())?;
    let mut rows = schedule::runs(&ctx.db, &name, cursor, per_page + 1).await?;
    let next_cursor = (rows.len() as u64 > per_page)
        .then(|| {
            rows.truncate(per_page as usize);
            rows.last()
                .map(|r| hex::encode(format!("{}|{}", r.started_at.timestamp_nanos_opt().unwrap_or_default(), r.id)))
        })
        .flatten();
    let timeout = s.run_timeout();
    Ok(Json(ScheduleRunPage {
        data: rows.into_iter().map(|row| info(row, timeout)).collect(),
        meta: crate::queue::QueueJobPageMeta { next_cursor },
    }))
}

/// What Run now records.
#[derive(Serialize)]
struct RunMeta<'a> {
    run: &'a str,
}

/// `POST schedules/{name}/run`: runs the schedule now (through the queue).
async fn run_now(
    State(state): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Path(name): Path<String>,
) -> AppResult<(StatusCode, Json<ScheduleRunResponse>)> {
    // Admission before the name is looked at, and again under the RBAC lock.
    if !(ctx.can(VIEW) && ctx.can(MANAGE)) {
        return Err(AppError::Forbidden);
    }
    let (txn, current) = rustcms_auth::rbac::begin_as(state.db(), &ctx.principal).await?;
    if !(current.can(VIEW) && current.can(MANAGE)) {
        return Err(AppError::Forbidden);
    }
    let actor = ctx.principal.id.parse::<i64>().ok();
    match schedule::run_now(&txn, &state, &name, actor).await? {
        Ok(run) => {
            let entry =
                AuditEntry::new("schedule.run", "schedule").key(&name).meta(RunMeta { run: &run.id.to_string() });
            ctx.audit(&txn, entry).await?;
            txn.commit().await?;
            let timeout = schedules(&state).get(&name).map_or(schedule::DEFAULT_TIMEOUT, |s| s.run_timeout());
            Ok((StatusCode::ACCEPTED, Json(ScheduleRunResponse { run: info(run, timeout) })))
        }
        Err(refusal) => {
            txn.rollback().await?;
            Err(match refusal {
                RunNowError::Unknown => AppError::NotFound,
                RunNowError::Running => AppError::Conflict {
                    code: "write_conflict",
                    reason: "running",
                    message: "The previous run of this schedule has not finished yet.",
                },
            })
        }
    }
}
