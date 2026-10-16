//! A job queue in the database (ADR 0016).
//!
//! - [`dispatch`] adds a job in the transaction of the change that needs it:
//!   no change, no job (an outbox). [`enqueue`] adds one on its own.
//! - Workers (`serve`, `rustcms queue:work`) claim a job with an owner token
//!   and a lease, keep it with heartbeats while it runs, and remove it only if
//!   they still own it. A job runs **at least once**: make its effects
//!   idempotent ([`once`] records a database effect in its transaction).
//! - A failed run is retried after [`Job::backoff`]; after
//!   [`Job::max_attempts`] the job stays as failed (`queue:failed`,
//!   `queue:retry`, `queue:forget`).

mod commands;
pub mod manage;
mod worker;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sea_orm::sea_query::OnConflict;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ConnectionTrait, DatabaseConnection, DatabaseTransaction, EntityTrait,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use commands::{QueueFailed, QueueForget, QueueRetry, QueueWork, definitions};
pub use manage::{Clock, JobStatus, QueueCounts};
pub use worker::{Workers, run_until_idle};

use crate::audit::{AuditActor, AuditContext};
use crate::models::{job, job_receipt};
use crate::{AppError, AppResult, AppState};

/// Largest serialized payload.
pub const MAX_PAYLOAD_BYTES: usize = 64 * 1024;
/// The queue a job goes to unless it says otherwise.
pub const DEFAULT_QUEUE: &str = "default";

/// Work to do later, in a worker.
///
/// ```rust,ignore
/// use rustcms::async_trait;
///
/// #[derive(Serialize, Deserialize)]
/// struct SendWelcome { learner_id: i64 }
///
/// #[async_trait]
/// impl Job for SendWelcome {
///     const KIND: &'static str = "lms.send_welcome";
///     async fn handle(self, ctx: JobCtx) -> anyhow::Result<()> {
///         todo!()
///     }
/// }
/// ```
#[async_trait]
pub trait Job: Serialize + DeserializeOwned + Send + Sync + 'static {
    /// Name of the job: `[a-z0-9_.]{1,64}`, unique in the application.
    const KIND: &'static str;
    /// Version of the payload; raise it when the struct changes (see [`Job::upgrade`]).
    const VERSION: u32 = 1;
    /// Queue it goes to.
    const QUEUE: &'static str = DEFAULT_QUEUE;

    /// Most runs (1 to 100).
    fn max_attempts() -> u32 {
        5
    }

    /// Wait before run `attempt + 1`, after run `attempt` failed.
    fn backoff(attempt: u32) -> Duration {
        let seconds = 10u64.saturating_mul(1 << attempt.saturating_sub(1).min(16));
        Duration::from_secs(seconds.min(3_600))
    }

    /// Longest run; then it is stopped and counts as failed.
    fn timeout() -> Duration {
        Duration::from_secs(300)
    }

    /// The payload of an older `version`, as this version reads it. By
    /// default older payloads are refused (the job fails, saying why).
    fn upgrade(version: u32, payload: Value) -> anyhow::Result<Value> {
        let _ = payload;
        anyhow::bail!("payload version {version} is no longer supported")
    }

    /// Does the work. An error (or a panic, or the timeout) retries it.
    async fn handle(self, ctx: JobCtx) -> anyhow::Result<()>;
}

/// A failure whose message is safe to keep (`queue:failed` shows it): return
/// it from [`Job::handle`]. Any other error is kept as "failed (see the
/// logs)", so a secret in an error message never reaches the table.
#[derive(Debug, Clone)]
pub struct Failure {
    /// Stable code (`smtp_rejected`).
    pub code: String,
    /// Message without secrets or personal data.
    pub message: String,
}

impl Failure {
    /// A failure with a safe `code` and `message`.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self { code: code.into(), message: message.into() }
    }

    /// The same failure, not retried: the job fails at once (retrying cannot
    /// help until someone changes something; `queue:retry` still runs it again).
    pub fn permanent(self) -> Permanent {
        Permanent(self)
    }
}

/// A [`Failure`] the queue does not retry (see [`Failure::permanent`]).
#[derive(Debug, Clone)]
pub struct Permanent(pub Failure);

impl std::fmt::Display for Permanent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for Permanent {}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for Failure {}

/// What a running job knows.
#[derive(Clone)]
pub struct JobCtx {
    /// The application.
    pub state: AppState,
    /// The job (also a good idempotency key for external services).
    pub id: uuid::Uuid,
    /// This run (1 for the first).
    pub attempt: u32,
    /// The user who asked for it, if any. Their permissions are not carried:
    /// load them again if the work depends on them.
    pub initiated_by: Option<i64>,
    kind: String,
}

impl JobCtx {
    /// Who the audit log says acted: the job, on behalf of its initiator.
    pub fn audit_context(&self) -> AuditContext {
        AuditContext::new(AuditActor::job(&self.kind, self.initiated_by), Default::default())
    }
}

/// `[queue]` settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct QueueConfig {
    /// Whether `serve` runs workers.
    pub enabled: bool,
    /// Workers per process (`serve` and `queue:work`); 0: none in `serve`.
    pub workers: usize,
    /// Queues the workers take jobs from, in order of preference.
    pub queues: Vec<String>,
    /// A run owns its job this long, renewed by heartbeats.
    pub lease_secs: u64,
    /// Wait between looks when the queue is empty.
    pub poll_ms: u64,
    /// On shutdown, how long running jobs may finish.
    pub shutdown_secs: u64,
    /// Upper bound of any job's [`Job::timeout`].
    pub max_timeout_secs: u64,
    /// Tolerated clock difference between instances.
    pub clock_skew_secs: u64,
    /// How long a job no worker knows may wait (a rolling deploy) before it fails.
    pub unknown_kind_grace_secs: u64,
    /// Failed jobs and receipts are deleted after this many days.
    pub failed_retention_days: u32,
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            workers: 2,
            queues: vec![DEFAULT_QUEUE.to_owned()],
            lease_secs: 60,
            poll_ms: 1_000,
            shutdown_secs: 30,
            max_timeout_secs: 3_600,
            clock_skew_secs: 5,
            unknown_kind_grace_secs: 3_600,
            failed_retention_days: 30,
        }
    }
}

/// Whether `name` is a valid queue name.
pub fn valid_queue(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'.' | b'-'))
}

fn valid_kind(kind: &str) -> bool {
    (1..=64).contains(&kind.len())
        && kind.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'.'))
}

impl QueueConfig {
    /// The `[queue]` section, validated.
    pub fn of(config: &crate::Config) -> anyhow::Result<Self> {
        config.section::<Self>("queue")?.validated()
    }

    fn validated(self) -> anyhow::Result<Self> {
        let range = |name: &str, value: u64, min: u64, max: u64| {
            anyhow::ensure!((min..=max).contains(&value), "queue.{name} must be between {min} and {max}");
            Ok(())
        };
        range("workers", self.workers as u64, 0, 64)?;
        range("lease_secs", self.lease_secs, 10, 3_600)?;
        range("poll_ms", self.poll_ms, 100, 60_000)?;
        range("shutdown_secs", self.shutdown_secs, 0, 3_600)?;
        range("max_timeout_secs", self.max_timeout_secs, 1, 86_400)?;
        range("clock_skew_secs", self.clock_skew_secs, 0, 60)?;
        range("unknown_kind_grace_secs", self.unknown_kind_grace_secs, 0, 7 * 86_400)?;
        range("failed_retention_days", u64::from(self.failed_retention_days), 1, 3_650)?;
        anyhow::ensure!(!self.queues.is_empty(), "queue.queues lists no queue");
        for queue in &self.queues {
            anyhow::ensure!(valid_queue(queue), "queue.queues: `{queue}` is not 1–64 of `a-z`, `0-9`, `_`, `.`, `-`");
        }
        Ok(self)
    }
}

/// How a run ended.
pub(crate) enum Outcome {
    Done,
    /// Retry (or fail, on the last attempt), keeping this safe message.
    Retry(String),
    /// Fail now: retrying cannot help.
    Fail(String),
    /// A newer producer: give it back for a worker that knows it.
    Unknown,
}

/// A registered job, type-erased.
#[async_trait]
pub(crate) trait Handler: Send + Sync {
    fn backoff(&self, attempt: u32) -> Duration;
    fn timeout(&self) -> Duration;
    async fn run(&self, version: u32, payload: Value, ctx: JobCtx) -> Outcome;
}

struct Typed<J>(std::marker::PhantomData<J>);

#[async_trait]
impl<J: Job> Handler for Typed<J> {
    fn backoff(&self, attempt: u32) -> Duration {
        J::backoff(attempt)
    }

    fn timeout(&self) -> Duration {
        J::timeout()
    }

    async fn run(&self, version: u32, payload: Value, ctx: JobCtx) -> Outcome {
        if version > J::VERSION {
            return Outcome::Unknown;
        }
        let payload = if version < J::VERSION {
            match J::upgrade(version, payload) {
                Ok(payload) => payload,
                Err(err) => {
                    tracing::warn!(kind = J::KIND, id = %ctx.id, err = %format!("{err:#}"), "job payload not upgraded");
                    return Outcome::Fail(format!(
                        "payload version {version} cannot be read by version {}",
                        J::VERSION
                    ));
                }
            }
        } else {
            payload
        };
        if serde_json::to_vec(&payload).map_or(true, |b| b.len() > MAX_PAYLOAD_BYTES) {
            return Outcome::Fail("the payload is too large".into());
        }
        let Ok(job) = serde_json::from_value::<J>(payload) else {
            return Outcome::Fail("the payload does not match the job".into());
        };
        let id = ctx.id;
        match job.handle(ctx).await {
            Ok(()) => Outcome::Done,
            Err(err) => {
                // A failure's own text is safe to log; any other error is logged as it is.
                if let Some(Permanent(failure)) = err.downcast_ref::<Permanent>() {
                    tracing::warn!(kind = J::KIND, %id, code = %failure.code, "job failed (not retried)");
                    return Outcome::Fail(cut(&failure.to_string(), 500));
                }
                if let Some(failure) = err.downcast_ref::<Failure>() {
                    tracing::warn!(kind = J::KIND, %id, code = %failure.code, "job failed");
                    return Outcome::Retry(cut(&failure.to_string(), 500));
                }
                tracing::warn!(kind = J::KIND, %id, err = %format!("{err:#}"), "job failed");
                Outcome::Retry(format!("failed (see the logs for job {id})"))
            }
        }
    }
}

fn cut(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// The jobs an application handles (see [`crate::AppBuilder::job`]).
#[derive(Default, Clone)]
pub struct Jobs {
    handlers: BTreeMap<&'static str, Arc<dyn Handler>>,
}

impl Jobs {
    /// Registers `J`.
    ///
    /// # Panics
    /// On an invalid or repeated kind, or limits out of range.
    pub fn register<J: Job>(&mut self) {
        assert!(valid_kind(J::KIND), "job `{}`: kinds are 1–64 of `a-z`, `0-9`, `_`, `.`", J::KIND);
        assert!(valid_queue(J::QUEUE), "job `{}`: invalid queue `{}`", J::KIND, J::QUEUE);
        assert!((1..=100).contains(&J::max_attempts()), "job `{}`: max_attempts must be 1 to 100", J::KIND);
        assert!(J::VERSION >= 1 && J::VERSION <= i32::MAX as u32, "job `{}`: VERSION must be at least 1", J::KIND);
        assert!(!J::timeout().is_zero(), "job `{}`: the timeout must not be zero", J::KIND);
        assert!(
            self.handlers.insert(J::KIND, Arc::new(Typed::<J>(std::marker::PhantomData))).is_none(),
            "job `{}` registered twice",
            J::KIND
        );
    }

    pub(crate) fn get(&self, kind: &str) -> Option<&Arc<dyn Handler>> {
        self.handlers.get(kind)
    }

    /// Whether no job is registered.
    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }
}

/// The current time, as stored (microseconds).
pub(crate) fn now() -> DateTime<Utc> {
    let now = Utc::now();
    DateTime::from_timestamp_micros(now.timestamp_micros()).unwrap_or(now)
}

fn row<J: Job>(job: &J, at: DateTime<Utc>, initiated_by: Option<i64>) -> AppResult<job::ActiveModel> {
    let payload =
        serde_json::to_value(job).map_err(|_| AppError::internal(format!("job `{}` does not serialize", J::KIND)))?;
    let size = serde_json::to_vec(&payload).map_or(usize::MAX, |b| b.len());
    if size > MAX_PAYLOAD_BYTES {
        return Err(AppError::internal(format!("job `{}`: the payload exceeds {MAX_PAYLOAD_BYTES} bytes", J::KIND)));
    }
    let created = now();
    Ok(job::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        queue: Set(J::QUEUE.to_owned()),
        kind: Set(J::KIND.to_owned()),
        version: Set(J::VERSION as i32),
        payload: Set(payload),
        attempts: Set(0),
        max_attempts: Set(J::max_attempts() as i32),
        available_at: Set(at.max(created)),
        reserved_by: Set(None),
        reserved_until: Set(None),
        last_error: Set(None),
        initiated_by: Set(initiated_by),
        created_at: Set(created),
        failed_at: Set(None),
    })
}

async fn insert<C: ConnectionTrait>(conn: &C, model: job::ActiveModel) -> AppResult<uuid::Uuid> {
    Ok(model.insert(conn).await?.id)
}

/// Adds `job` in `txn`, the transaction of the change it follows from: it
/// exists only if that change commits.
pub async fn dispatch<J: Job>(txn: &DatabaseTransaction, job: &J) -> AppResult<uuid::Uuid> {
    insert(txn, row(job, now(), None)?).await
}

/// Like [`dispatch`], run no earlier than `at`, on behalf of `initiated_by`.
pub async fn dispatch_with<J: Job>(
    txn: &DatabaseTransaction,
    job: &J,
    at: Option<DateTime<Utc>>,
    initiated_by: Option<i64>,
) -> AppResult<uuid::Uuid> {
    insert(txn, row(job, at.unwrap_or_else(now), initiated_by)?).await
}

/// Adds `job` on its own (not tied to a change).
pub async fn enqueue<J: Job>(db: &DatabaseConnection, job: &J) -> AppResult<uuid::Uuid> {
    insert(db, row(job, now(), None)?).await
}

/// Records the effect `key` in `txn`; `false` if it was recorded before. Use
/// it in a job's transaction so a run repeated after a crash applies the
/// effect once: `if !queue::once(&txn, &format!("welcome:{id}")).await? { return Ok(()) }`.
pub async fn once(txn: &DatabaseTransaction, key: &str) -> AppResult<bool> {
    if key.is_empty() || key.len() > 191 {
        return Err(AppError::internal("queue::once keys are 1 to 191 bytes"));
    }
    // One statement: a concurrent run with the same key waits for this one,
    // then inserts nothing.
    let inserted = if txn.get_database_backend() == sea_orm::DbBackend::MySql {
        // sea-query renders MySQL's form as `ON DUPLICATE KEY IGNORE`, which MySQL rejects.
        let statement = sea_orm::Statement::from_sql_and_values(
            sea_orm::DbBackend::MySql,
            "INSERT IGNORE INTO `job_receipts` (`key`, `created_at`) VALUES (?, ?)",
            [key.into(), now().into()],
        );
        txn.execute_raw(statement).await?.rows_affected()
    } else {
        let receipt = job_receipt::ActiveModel { key: Set(key.to_owned()), created_at: Set(now()) };
        job_receipt::Entity::insert(receipt)
            .on_conflict(OnConflict::column(job_receipt::Column::Key).do_nothing().to_owned())
            .exec_without_returning(txn)
            .await?
    };
    Ok(inserted == 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize, Deserialize)]
    struct Noop;

    #[async_trait]
    impl Job for Noop {
        const KIND: &'static str = "test.noop";
        async fn handle(self, _: JobCtx) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn backoff_grows_and_is_capped() {
        assert_eq!(Noop::backoff(1), Duration::from_secs(10));
        assert_eq!(Noop::backoff(2), Duration::from_secs(20));
        assert_eq!(Noop::backoff(40), Duration::from_secs(3_600));
    }

    #[test]
    #[should_panic(expected = "registered twice")]
    fn a_kind_is_registered_once() {
        let mut jobs = Jobs::default();
        jobs.register::<Noop>();
        jobs.register::<Noop>();
    }

    #[test]
    fn config_is_validated() {
        let ok = QueueConfig::default().validated();
        assert!(ok.is_ok());
        for bad in [
            QueueConfig { poll_ms: 0, ..Default::default() },
            QueueConfig { lease_secs: 1, ..Default::default() },
            QueueConfig { queues: vec![], ..Default::default() },
            QueueConfig { queues: vec!["Bad Name".into()], ..Default::default() },
            QueueConfig { workers: 1_000, ..Default::default() },
        ] {
            assert!(bad.validated().is_err());
        }
    }

    #[test]
    fn names() {
        assert!(valid_kind("lms.send_welcome") && !valid_kind("Lms") && !valid_kind(""));
        assert!(valid_queue("mail-1") && !valid_queue("a b"));
    }
}
