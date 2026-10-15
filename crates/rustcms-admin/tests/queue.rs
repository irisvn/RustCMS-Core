//! The job queue (ADR 0016): dispatch in the change's transaction, claims
//! with owner tokens and leases, retries, timeouts, safe errors, shutdown.
//! All databases.

mod support;

use std::time::Duration;

use rustcms_core::models::{audit_log, job, job_receipt};
use rustcms_core::queue::{self, Failure, Job, JobCtx, QueueConfig, Workers};
use rustcms_core::{AppBuilder, Config};
use rustcms_testing::{TestApp, TestDb, db_test};
use sea_orm::sea_query::Expr;
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Leaves one receipt per run: the effects a test counts.
async fn effect(ctx: &JobCtx, tag: &str) -> anyhow::Result<()> {
    let txn = rustcms_core::db::begin_write(ctx.state.db()).await?;
    queue::once(&txn, &format!("{tag}:{}", uuid::Uuid::now_v7())).await?;
    txn.commit().await?;
    Ok(())
}

#[derive(Serialize, Deserialize)]
struct Record {
    tag: String,
}

#[async_trait::async_trait]
impl Job for Record {
    const KIND: &'static str = "test.record";
    async fn handle(self, ctx: JobCtx) -> anyhow::Result<()> {
        effect(&ctx, &self.tag).await
    }
}

/// Fails (with a secret in its error) until attempt `until`.
#[derive(Serialize, Deserialize)]
struct Flaky {
    until: u32,
}

#[async_trait::async_trait]
impl Job for Flaky {
    const KIND: &'static str = "test.flaky";
    fn max_attempts() -> u32 {
        3
    }
    async fn handle(self, ctx: JobCtx) -> anyhow::Result<()> {
        if ctx.attempt < self.until {
            anyhow::bail!("connection refused, password=secret-sentinel");
        }
        effect(&ctx, "flaky").await
    }
}

#[derive(Serialize, Deserialize)]
struct Refused;

#[async_trait::async_trait]
impl Job for Refused {
    const KIND: &'static str = "test.refused";
    fn max_attempts() -> u32 {
        1
    }
    async fn handle(self, _: JobCtx) -> anyhow::Result<()> {
        Err(Failure::new("smtp_rejected", "the server refused the message").into())
    }
}

#[derive(Serialize, Deserialize)]
struct Panics;

#[async_trait::async_trait]
impl Job for Panics {
    const KIND: &'static str = "test.panics";
    fn max_attempts() -> u32 {
        1
    }
    async fn handle(self, _: JobCtx) -> anyhow::Result<()> {
        panic!("panic with secret-sentinel");
    }
}

/// Sleeps `ms`, then records; times out after one second.
#[derive(Serialize, Deserialize)]
struct Slow {
    ms: u64,
    tag: String,
}

#[async_trait::async_trait]
impl Job for Slow {
    const KIND: &'static str = "test.slow";
    fn max_attempts() -> u32 {
        2
    }
    fn timeout() -> Duration {
        Duration::from_secs(1)
    }
    async fn handle(self, ctx: JobCtx) -> anyhow::Result<()> {
        tokio::time::sleep(Duration::from_millis(self.ms)).await;
        effect(&ctx, &self.tag).await
    }
}

/// Runs for a while (a lease's worth), then records.
#[derive(Serialize, Deserialize)]
struct Long {
    ms: u64,
}

#[async_trait::async_trait]
impl Job for Long {
    const KIND: &'static str = "test.long";
    fn timeout() -> Duration {
        Duration::from_secs(60)
    }
    async fn handle(self, ctx: JobCtx) -> anyhow::Result<()> {
        tokio::time::sleep(Duration::from_millis(self.ms)).await;
        effect(&ctx, "long").await
    }
}

/// Runs for a while, then records — with one attempt only.
#[derive(Serialize, Deserialize)]
struct LastChance {
    ms: u64,
}

#[async_trait::async_trait]
impl Job for LastChance {
    const KIND: &'static str = "test.last_chance";
    fn max_attempts() -> u32 {
        1
    }
    fn timeout() -> Duration {
        Duration::from_secs(60)
    }
    async fn handle(self, ctx: JobCtx) -> anyhow::Result<()> {
        tokio::time::sleep(Duration::from_millis(self.ms)).await;
        effect(&ctx, "last").await
    }
}

/// Runs for a while, then fails.
#[derive(Serialize, Deserialize)]
struct SlowFailure {
    ms: u64,
}

#[async_trait::async_trait]
impl Job for SlowFailure {
    const KIND: &'static str = "test.slow_failure";
    fn timeout() -> Duration {
        Duration::from_secs(60)
    }
    async fn handle(self, _: JobCtx) -> anyhow::Result<()> {
        tokio::time::sleep(Duration::from_millis(self.ms)).await;
        anyhow::bail!("late failure")
    }
}

/// Records an audit entry as the job.
#[derive(Serialize, Deserialize)]
struct Audited;

#[async_trait::async_trait]
impl Job for Audited {
    const KIND: &'static str = "test.audited";
    async fn handle(self, ctx: JobCtx) -> anyhow::Result<()> {
        let txn = rustcms_core::db::begin_write(ctx.state.db()).await?;
        let entry = rustcms_core::audit::AuditEntry::new("test.done", "test");
        rustcms_core::audit::record(&txn, &ctx.audit_context(), entry).await?;
        txn.commit().await?;
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
struct Big {
    text: String,
}

#[async_trait::async_trait]
impl Job for Big {
    const KIND: &'static str = "test.big";
    async fn handle(self, _: JobCtx) -> anyhow::Result<()> {
        Ok(())
    }
}

fn build(config: Config) -> AppBuilder {
    let config = config.with("queue.lease_secs", 10).and_then(|c| c.with("queue.poll_ms", 100)).unwrap();
    let mut app = AppBuilder::new(config);
    app.job::<Record>()
        .job::<Flaky>()
        .job::<Refused>()
        .job::<Panics>()
        .job::<Slow>()
        .job::<Long>()
        .job::<Audited>()
        .job::<Big>()
        .job::<LastChance>()
        .job::<SlowFailure>();
    app
}

async fn app(db: TestDb) -> TestApp {
    db.migrate::<rustcms_migration::Migrator>().await;
    TestApp::new(build, db).await
}

async fn effects(app: &TestApp, tag: &str) -> u64 {
    job_receipt::Entity::find()
        .filter(job_receipt::Column::Key.starts_with(format!("{tag}:")))
        .count(app.state().db())
        .await
        .unwrap()
}

async fn jobs(app: &TestApp) -> Vec<job::Model> {
    job::Entity::find().all(app.state().db()).await.unwrap()
}

/// Makes every waiting job ready now (instead of waiting for its backoff).
async fn ready(app: &TestApp) {
    job::Entity::update_many()
        .col_expr(job::Column::AvailableAt, Expr::value(chrono::Utc::now() - chrono::Duration::seconds(1)))
        .exec(app.state().db())
        .await
        .unwrap();
}

async fn run(app: &TestApp) -> usize {
    queue::run_until_idle(app.state(), None, 1_000).await.unwrap()
}

db_test!(a_job_exists_only_if_its_change_commits, |db: TestDb| async move {
    let app = app(db).await;
    let conn = app.state().db();
    let txn = rustcms_core::db::begin_write(conn).await.unwrap();
    queue::dispatch(&txn, &Record { tag: "lost".into() }).await.unwrap();
    txn.rollback().await.unwrap();
    assert!(jobs(&app).await.is_empty());
    let txn = rustcms_core::db::begin_write(conn).await.unwrap();
    queue::dispatch(&txn, &Record { tag: "kept".into() }).await.unwrap();
    txn.commit().await.unwrap();
    queue::enqueue(conn, &Record { tag: "kept".into() }).await.unwrap();
    assert_eq!(run(&app).await, 2);
    assert_eq!(effects(&app, "kept").await, 2);
    assert!(jobs(&app).await.is_empty(), "done jobs are removed");
    // A payload over 64 KiB is refused at once.
    let big = Big { text: "x".repeat(70 * 1024) };
    assert!(queue::enqueue(conn, &big).await.is_err());
});

db_test!(failed_runs_retry_then_fail_with_a_safe_message, |db: TestDb| async move {
    let app = app(db).await;
    let conn = app.state().db();
    queue::enqueue(conn, &Flaky { until: 3 }).await.unwrap();
    run(&app).await;
    let first = jobs(&app).await.remove(0);
    assert_eq!((first.attempts, first.failed_at.is_none()), (1, true));
    assert!(first.available_at > chrono::Utc::now(), "waits for its backoff");
    assert!(first.last_error.as_deref().unwrap().starts_with("failed (see the logs"));
    assert_eq!(run(&app).await, 0, "not before its backoff");
    ready(&app).await;
    run(&app).await;
    ready(&app).await;
    run(&app).await;
    assert_eq!(effects(&app, "flaky").await, 1, "third attempt succeeded");
    assert!(jobs(&app).await.is_empty());

    // Out of attempts: failed, with the safe message only.
    queue::enqueue(conn, &Flaky { until: 10 }).await.unwrap();
    for _ in 0..3 {
        ready(&app).await;
        run(&app).await;
    }
    let failed = jobs(&app).await.remove(0);
    assert!(failed.failed_at.is_some());
    assert_eq!(failed.attempts, 3);
    queue::enqueue(conn, &Refused).await.unwrap();
    queue::enqueue(conn, &Panics).await.unwrap();
    run(&app).await;
    let all = jobs(&app).await;
    let error = |kind: &str| all.iter().find(|j| j.kind == kind).unwrap().last_error.clone().unwrap();
    assert_eq!(error("test.refused"), "smtp_rejected: the server refused the message");
    assert_eq!(error("test.panics"), "panicked");
    assert!(!format!("{all:?}").contains("secret-sentinel"), "no secret in the table");
});

db_test!(jobs_no_worker_knows_wait_then_fail, |db: TestDb| async move {
    let app = app(db).await;
    let conn = app.state().db();
    let now = chrono::Utc::now();
    let row = |kind: &str, version: i32, payload: serde_json::Value, created: chrono::DateTime<chrono::Utc>| {
        job::ActiveModel {
            id: sea_orm::Set(uuid::Uuid::now_v7()),
            queue: sea_orm::Set("default".into()),
            kind: sea_orm::Set(kind.into()),
            version: sea_orm::Set(version),
            payload: sea_orm::Set(payload),
            attempts: sea_orm::Set(0),
            max_attempts: sea_orm::Set(3),
            available_at: sea_orm::Set(now - chrono::Duration::seconds(1)),
            reserved_by: sea_orm::Set(None),
            reserved_until: sea_orm::Set(None),
            last_error: sea_orm::Set(None),
            initiated_by: sea_orm::Set(None),
            created_at: sea_orm::Set(created),
            failed_at: sea_orm::Set(None),
        }
    };
    let insert = |model: job::ActiveModel| async move { sea_orm::ActiveModelTrait::insert(model, conn).await.unwrap() };
    // A newer producer (unknown kind, newer version): given back, attempt not counted.
    let unknown = insert(row("test.future", 1, json!({}), now)).await;
    let newer = insert(row("test.record", 2, json!({"tag": "x"}), now)).await;
    // Too old to wait any longer; an older version without an upgrade; a payload that does not fit.
    let stale = insert(row("test.future", 1, json!({}), now - chrono::Duration::hours(2))).await;
    let older = insert(row("test.flaky", 0, json!({"until": 1}), now)).await;
    let garbled = insert(row("test.record", 1, json!({"wrong": true}), now)).await;
    run(&app).await;
    let find = |id| async move { job::Entity::find_by_id(id).one(conn).await.unwrap().unwrap() };
    for id in [unknown.id, newer.id] {
        let j = find(id).await;
        assert_eq!((j.attempts, j.failed_at.is_none()), (0, true), "{}", j.kind);
        assert!(j.available_at > chrono::Utc::now());
    }
    assert!(find(stale.id).await.last_error.unwrap().contains("no worker handles"));
    assert!(find(older.id).await.last_error.unwrap().contains("cannot be read"));
    assert_eq!(find(garbled.id).await.last_error.as_deref(), Some("the payload does not match the job"));
});

db_test!(an_expired_lease_is_taken_over_and_the_old_owner_cannot_settle, |db: TestDb| async move {
    let app = app(db).await;
    let conn = app.state().db();
    // A worker claimed it, then died.
    let id = queue::enqueue(conn, &Record { tag: "taken".into() }).await.unwrap();
    job::Entity::update_many()
        .col_expr(job::Column::ReservedBy, Expr::value("dead-worker"))
        .col_expr(job::Column::ReservedUntil, Expr::value(chrono::Utc::now() - chrono::Duration::seconds(30)))
        .col_expr(job::Column::Attempts, Expr::value(1))
        .exec(conn)
        .await
        .unwrap();
    assert_eq!(run(&app).await, 1);
    assert_eq!(effects(&app, "taken").await, 1);
    assert!(job::Entity::find_by_id(id).one(conn).await.unwrap().is_none());

    // Still within its lease: nobody else takes it.
    let id = queue::enqueue(conn, &Record { tag: "held".into() }).await.unwrap();
    job::Entity::update_many()
        .col_expr(job::Column::ReservedBy, Expr::value("live-worker"))
        .col_expr(job::Column::ReservedUntil, Expr::value(chrono::Utc::now() + chrono::Duration::seconds(30)))
        .filter(job::Column::Id.eq(id))
        .exec(conn)
        .await
        .unwrap();
    assert_eq!(run(&app).await, 0);

    // Died on its last attempt: failed, not run again.
    job::Entity::update_many()
        .col_expr(job::Column::ReservedUntil, Expr::value(chrono::Utc::now() - chrono::Duration::seconds(30)))
        .col_expr(job::Column::Attempts, Expr::col(job::Column::MaxAttempts))
        .filter(job::Column::Id.eq(id))
        .exec(conn)
        .await
        .unwrap();
    assert_eq!(run(&app).await, 0);
    let reaped = job::Entity::find_by_id(id).one(conn).await.unwrap().unwrap();
    assert!(reaped.failed_at.is_some(), "{reaped:?}");
    assert_eq!(effects(&app, "held").await, 0);
});

db_test!(a_run_that_loses_its_lease_is_stopped_and_not_settled, |db: TestDb| async move {
    let app = app(db).await;
    let conn = app.state().db().clone();
    let id = queue::enqueue(&conn, &Long { ms: 8_000 }).await.unwrap();
    let state = app.state().clone();
    let worker = tokio::spawn(async move { queue::run_until_idle(&state, None, 1).await.unwrap() });
    // Another worker took it over (its lease, its token).
    tokio::time::sleep(Duration::from_millis(500)).await;
    job::Entity::update_many()
        .col_expr(job::Column::ReservedBy, Expr::value("other-worker"))
        .filter(job::Column::Id.eq(id))
        .exec(&conn)
        .await
        .unwrap();
    worker.await.unwrap();
    let row = job::Entity::find_by_id(id).one(&conn).await.unwrap().unwrap();
    assert_eq!(row.reserved_by.as_deref(), Some("other-worker"), "the old run settled nothing");
    assert_eq!(effects(&app, "long").await, 0, "it was stopped");
});

db_test!(a_run_over_its_timeout_is_stopped_and_retried, |db: TestDb| async move {
    let app = app(db).await;
    let conn = app.state().db();
    queue::enqueue(conn, &Slow { ms: 5_000, tag: "late".into() }).await.unwrap();
    let started = std::time::Instant::now();
    run(&app).await;
    assert!(started.elapsed() < Duration::from_secs(4), "stopped at its timeout");
    let row = jobs(&app).await.remove(0);
    assert_eq!(row.last_error.as_deref(), Some("timed out after 1 s"));
    assert_eq!(effects(&app, "late").await, 0);
});

db_test!(two_workers_run_each_job_once, |db: TestDb| async move {
    let app = app(db).await;
    let conn = app.state().db();
    for _ in 0..50 {
        queue::enqueue(conn, &Record { tag: "many".into() }).await.unwrap();
    }
    let config = QueueConfig::of(app.state().config()).unwrap();
    let workers = Workers::start(app.state(), &config, None, 2);
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    while job::Entity::find().count(conn).await.unwrap() > 0 && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    workers.shutdown().await;
    assert_eq!(effects(&app, "many").await, 50);
    assert!(jobs(&app).await.is_empty());
});

db_test!(shutdown_lets_short_jobs_finish_and_gives_long_ones_back, |db: TestDb| async move {
    let app = app(db).await;
    let conn = app.state().db();
    let config = QueueConfig { shutdown_secs: 1, ..QueueConfig::of(app.state().config()).unwrap() };
    queue::enqueue(conn, &Long { ms: 300 }).await.unwrap();
    let workers = Workers::start(app.state(), &config, None, 1);
    tokio::time::sleep(Duration::from_millis(150)).await;
    workers.shutdown().await;
    assert_eq!(effects(&app, "long").await, 1, "finished within the drain");

    let id = queue::enqueue(conn, &Long { ms: 10_000 }).await.unwrap();
    let workers = Workers::start(app.state(), &config, None, 1);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let started = std::time::Instant::now();
    workers.shutdown().await;
    assert!(started.elapsed() < Duration::from_secs(4));
    let row = job::Entity::find_by_id(id).one(conn).await.unwrap().unwrap();
    assert_eq!((row.reserved_by, row.failed_at), (None, None), "given back for another run");
    assert_eq!(effects(&app, "long").await, 1);
    job::Entity::update_many().col_expr(job::Column::Payload, Expr::value(json!({"ms": 0}))).exec(conn).await.unwrap();
    assert_eq!(run(&app).await, 1, "and it runs again");
    assert_eq!(effects(&app, "long").await, 2);
});

db_test!(receipts_apply_an_effect_once, |db: TestDb| async move {
    let app = app(db).await;
    let conn = app.state().db();
    let txn = rustcms_core::db::begin_write(conn).await.unwrap();
    assert!(queue::once(&txn, "welcome:1").await.unwrap());
    txn.commit().await.unwrap();
    // A run repeated after a crash (effect done, job not deleted) skips it.
    let txn = rustcms_core::db::begin_write(conn).await.unwrap();
    assert!(!queue::once(&txn, "welcome:1").await.unwrap());
    txn.rollback().await.unwrap();
});

db_test!(jobs_act_as_the_job_for_their_initiator, |db: TestDb| async move {
    let app = app(db).await;
    let conn = app.state().db();
    let txn = rustcms_core::db::begin_write(conn).await.unwrap();
    queue::dispatch_with(&txn, &Audited, None, Some(42)).await.unwrap();
    txn.commit().await.unwrap();
    run(&app).await;
    let entry = audit_log::Entity::find().one(conn).await.unwrap().unwrap();
    assert_eq!(
        (entry.actor_kind.as_str(), entry.actor_user_id, entry.credential.as_deref()),
        ("job", Some(42), Some("queue:test.audited"))
    );
});

async fn run_command(app: &TestApp, command: &dyn rustcms_core::AppCommand, args: &[&str]) {
    let matches = command.definition().try_get_matches_from(args).unwrap();
    command.run(app.state(), &matches).await.unwrap();
}

db_test!(failed_jobs_are_listed_retried_and_forgotten, |db: TestDb| async move {
    let app = app(db).await;
    let conn = app.state().db();
    queue::enqueue(conn, &Refused).await.unwrap();
    queue::enqueue(conn, &Refused).await.unwrap();
    run(&app).await;
    run_command(&app, &queue::QueueFailed, &["queue:failed"]).await;
    let ids: Vec<_> = jobs(&app).await.into_iter().map(|j| j.id).collect();
    run_command(&app, &queue::QueueRetry, &["queue:retry", &ids[0].to_string()]).await;
    let retried = job::Entity::find_by_id(ids[0]).one(conn).await.unwrap().unwrap();
    assert_eq!((retried.failed_at, retried.attempts), (None, 0));
    run_command(&app, &queue::QueueForget, &["queue:forget", "--all"]).await;
    let left: Vec<_> = jobs(&app).await.into_iter().map(|j| j.id).collect();
    assert_eq!(left, [ids[0]], "only failed jobs are forgotten");
    run_command(&app, &queue::QueueWork, &["queue:work", "--once"]).await;
    assert_eq!(jobs(&app).await.len(), 1, "failed again after its one attempt");
});

/// Tags whose creation queues a job; a `boom` label fails after the dispatch.
struct QueuedTags;

#[async_trait::async_trait]
impl rustcms_admin::Resource for QueuedTags {
    type Entity = support::tag::Entity;
    const SLUG: &'static str = "queued_tags";

    fn meta() -> rustcms_admin::ResourceMeta {
        rustcms_admin::ResourceMeta::new("Tag").title("label").key(rustcms_admin::KeyStrategy::Natural)
    }

    fn fields() -> Vec<rustcms_admin::Field> {
        vec![
            rustcms_admin::Field::column(support::tag::Column::Slug).required(),
            rustcms_admin::Field::column(support::tag::Column::Label).required(),
        ]
    }

    async fn after_save(
        ctx: &rustcms_admin::Ctx,
        txn: &sea_orm::DatabaseTransaction,
        record: &support::tag::Model,
    ) -> rustcms_core::AppResult<()> {
        ctx.dispatch(txn, &Record { tag: format!("tag-{}", record.slug) }).await?;
        if record.label == "boom" {
            return Err(rustcms_core::AppError::internal("boom"));
        }
        Ok(())
    }
}

db_test!(resource_hooks_queue_jobs_with_their_change, |db: TestDb| async move {
    let app = app(db).await;
    support::create_tables(app.state().db()).await;
    let registry = rustcms_admin::Registry::new().register::<QueuedTags>().finish();
    let principal = support::admin();
    let ctx = rustcms_admin::Ctx::new(app.state().db().clone(), principal, registry.clone())
        .with_idempotency_key(Some("create-1".into()));
    let tags = registry.get("queued_tags").unwrap();
    let data = |label: &str| json!({"slug": "a", "label": label}).as_object().unwrap().clone();
    tags.create(&ctx, data("A")).await.unwrap();
    // A replay of the same create runs no hook: no second job.
    tags.create(&ctx, data("A")).await.unwrap();
    let queued = jobs(&app).await;
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].initiated_by, Some(1), "on behalf of the user");
    // The change fails after the dispatch: no job.
    let plain = rustcms_admin::Ctx::new(app.state().db().clone(), support::admin(), registry.clone());
    let boom = json!({"slug": "b", "label": "boom"}).as_object().unwrap().clone();
    tags.create(&plain, boom).await.unwrap_err();
    assert_eq!(jobs(&app).await.len(), 1);
    run(&app).await;
    assert_eq!(effects(&app, "tag-a").await, 1);
});

/// Moves job `id` to another owner with a live lease (a worker that took it over).
async fn take_over(app: &TestApp, id: uuid::Uuid) {
    job::Entity::update_many()
        .col_expr(job::Column::ReservedBy, Expr::value("other-worker"))
        .col_expr(job::Column::ReservedUntil, Expr::value(chrono::Utc::now() + chrono::Duration::seconds(60)))
        .filter(job::Column::Id.eq(id))
        .exec(app.state().db())
        .await
        .unwrap();
}

db_test!(a_late_ack_or_nack_changes_nothing, |db: TestDb| async move {
    let app = app(db).await;
    let conn = app.state().db();
    for done in [true, false] {
        let id = if done {
            queue::enqueue(conn, &Long { ms: 1_500 }).await.unwrap()
        } else {
            queue::enqueue(conn, &SlowFailure { ms: 1_500 }).await.unwrap()
        };
        let state = app.state().clone();
        let worker = tokio::spawn(async move { queue::run_until_idle(&state, None, 1).await.unwrap() });
        tokio::time::sleep(Duration::from_millis(400)).await;
        take_over(&app, id).await;
        worker.await.unwrap();
        let row = job::Entity::find_by_id(id).one(conn).await.unwrap().unwrap();
        assert_eq!(row.reserved_by.as_deref(), Some("other-worker"), "done: {done}");
        assert_eq!((row.failed_at, row.last_error.clone()), (None, None), "done: {done}");
        // Not taken by the next look either: the new owner's lease is live.
        assert_eq!(queue::run_until_idle(app.state(), None, 1).await.unwrap(), 0);
        job::Entity::delete_by_id(id).exec(conn).await.unwrap();
    }
});

db_test!(heartbeats_keep_a_long_run_owned, |db: TestDb| async move {
    let app = app(db).await;
    let conn = app.state().db().clone();
    // Longer than its 10 s lease.
    let id = queue::enqueue(&conn, &Long { ms: 12_000 }).await.unwrap();
    let state = app.state().clone();
    let worker = tokio::spawn(async move { queue::run_until_idle(&state, None, 1).await.unwrap() });
    tokio::time::sleep(Duration::from_millis(11_500)).await;
    assert_eq!(queue::run_until_idle(app.state(), None, 1).await.unwrap(), 0, "still owned");
    worker.await.unwrap();
    assert!(job::Entity::find_by_id(id).one(&conn).await.unwrap().is_none());
    assert_eq!(effects(&app, "long").await, 1);
});

db_test!(a_locked_first_job_is_skipped, |db: TestDb| async move {
    let app = app(db).await;
    if app.state().db().get_database_backend() == sea_orm::DbBackend::Sqlite {
        return; // One writer at a time: there is no row lock to skip.
    }
    let conn = app.state().db();
    let first = queue::enqueue(conn, &Record { tag: "locked".into() }).await.unwrap();
    queue::enqueue(conn, &Record { tag: "free".into() }).await.unwrap();
    let txn = rustcms_core::db::begin_write(conn).await.unwrap();
    use sea_orm::QuerySelect;
    job::Entity::find_by_id(first).lock_exclusive().one(&txn).await.unwrap();
    assert_eq!(queue::run_until_idle(app.state(), None, 1).await.unwrap(), 1);
    assert_eq!((effects(&app, "free").await, effects(&app, "locked").await), (1, 0));
    txn.rollback().await.unwrap();
});

db_test!(workers_on_separate_pools_run_each_job_once, |db: TestDb| async move {
    let one = app(db.clone()).await;
    let two = TestApp::new(build, db).await;
    let conn = one.state().db();
    for _ in 0..30 {
        queue::enqueue(conn, &Record { tag: "pools".into() }).await.unwrap();
    }
    let config = QueueConfig::of(one.state().config()).unwrap();
    let (a, b) = (Workers::start(one.state(), &config, None, 1), Workers::start(two.state(), &config, None, 1));
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    while job::Entity::find().count(conn).await.unwrap() > 0 && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    a.shutdown().await;
    b.shutdown().await;
    assert_eq!(effects(&one, "pools").await, 30);
});

db_test!(a_shutdown_on_the_last_attempt_gives_the_attempt_back, |db: TestDb| async move {
    let app = app(db).await;
    let conn = app.state().db();
    let config = QueueConfig { shutdown_secs: 1, ..QueueConfig::of(app.state().config()).unwrap() };
    let id = queue::enqueue(conn, &LastChance { ms: 10_000 }).await.unwrap();
    let workers = Workers::start(app.state(), &config, None, 1);
    tokio::time::sleep(Duration::from_millis(300)).await;
    workers.shutdown().await;
    let row = job::Entity::find_by_id(id).one(conn).await.unwrap().unwrap();
    assert_eq!((row.attempts, row.failed_at, row.reserved_by), (0, None, None));
    // Another run takes it at once and finishes it.
    job::Entity::update_many().col_expr(job::Column::Payload, Expr::value(json!({"ms": 0}))).exec(conn).await.unwrap();
    assert_eq!(run(&app).await, 1);
    assert_eq!(effects(&app, "last").await, 1);
});

db_test!(concurrent_receipts_report_the_second_as_done, |db: TestDb| async move {
    let app = app(db).await;
    let conn = app.state().db().clone();
    let first = rustcms_core::db::begin_write(&conn).await.unwrap();
    assert!(queue::once(&first, "shared").await.unwrap());
    let second = tokio::spawn(async move {
        let txn = rustcms_core::db::begin_write(&conn).await.unwrap();
        let fresh = queue::once(&txn, "shared").await.unwrap();
        txn.commit().await.unwrap();
        fresh
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    first.commit().await.unwrap();
    assert!(!second.await.unwrap(), "already recorded");
});

db_test!(oversized_stored_payloads_fail_and_old_failures_are_deleted, |db: TestDb| async move {
    let app = app(db).await;
    let conn = app.state().db();
    let id = queue::enqueue(conn, &Record { tag: "x".into() }).await.unwrap();
    job::Entity::update_many()
        .col_expr(job::Column::Payload, Expr::value(json!({"tag": "x".repeat(70 * 1024)})))
        .exec(conn)
        .await
        .unwrap();
    run(&app).await;
    let row = job::Entity::find_by_id(id).one(conn).await.unwrap().unwrap();
    assert_eq!(row.last_error.as_deref(), Some("the payload is too large"));
    // Failed long ago: `--once` (cron) deletes it.
    job::Entity::update_many()
        .col_expr(job::Column::FailedAt, Expr::value(chrono::Utc::now() - chrono::Duration::days(40)))
        .exec(conn)
        .await
        .unwrap();
    run(&app).await;
    assert!(jobs(&app).await.is_empty());
});
