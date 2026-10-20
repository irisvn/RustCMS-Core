//! Run history (0.8.0): finished jobs in `job_runs`, written with their
//! terminal transition; schedule runs, their outcome and summary; Run now
//! (job and closure schedules, at most once); the Queue and Schedules routes.
//! All databases.

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axum::http::StatusCode;
use rustcms_core::models::{audit_log, job, job_run, schedule_run};
use rustcms_core::queue::{self, Failure, Job, JobCtx, QueueConfig, Workers, manage};
use rustcms_core::schedule::{self, RunNowError, Schedule};
use rustcms_core::{AppBuilder, Config};
use rustcms_testing::{TestApp, TestDb, db_test};
use sea_orm::sea_query::Expr;
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Serialize, Deserialize)]
struct Deploy {
    app: String,
}

#[async_trait::async_trait]
impl Job for Deploy {
    const KIND: &'static str = "test.deploy";
    fn describe(&self) -> Option<String> {
        Some(format!("deploy {}\n#12{}", self.app, "x".repeat(300)))
    }
    async fn handle(self, _: JobCtx) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Fails until attempt 2.
#[derive(Serialize, Deserialize)]
struct Flaky;

#[async_trait::async_trait]
impl Job for Flaky {
    const KIND: &'static str = "test.flaky";
    fn max_attempts() -> u32 {
        3
    }
    fn backoff(_: u32) -> Duration {
        Duration::ZERO
    }
    async fn handle(self, ctx: JobCtx) -> anyhow::Result<()> {
        if ctx.attempt < 2 { anyhow::bail!("password=secret-sentinel") } else { Ok(()) }
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

/// An at-most-once job that takes long (shutdown fails it).
#[derive(Serialize, Deserialize)]
struct Once;

static ONCE_RUNS: AtomicUsize = AtomicUsize::new(0);

#[async_trait::async_trait]
impl Job for Once {
    const KIND: &'static str = "test.once";
    const AT_MOST_ONCE: bool = true;
    fn max_attempts() -> u32 {
        1
    }
    async fn handle(self, _: JobCtx) -> anyhow::Result<()> {
        ONCE_RUNS.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_secs(30)).await;
        Ok(())
    }
}

fn build(config: Config) -> AppBuilder {
    let mut app = support::app::build(config.with("queue.enabled", false).unwrap());
    app.job::<Deploy>().job::<Flaky>().job::<Refused>().job::<Once>();
    app.schedule(Schedule::every("deploys", Duration::from_secs(3600)).job(|| Deploy { app: "web".into() }));
    app.schedule(
        Schedule::every("solo", Duration::from_secs(3600)).job(|| Deploy { app: "solo".into() }).without_overlapping(),
    );
    let calls = Arc::new(AtomicUsize::new(0));
    app.schedule(Schedule::every("backups", Duration::from_secs(3600)).task(move |ctx| {
        let calls = calls.clone();
        async move {
            assert_eq!(ctx.trigger, schedule::Trigger::Manual);
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(Some(format!("3 databases backed up\u{7}, run {}", ctx.run_id)))
        }
    }));
    app.schedule(
        Schedule::every("alerts", Duration::from_secs(3600))
            .task(|_| async { Err(Failure::new("smtp_down", "the mail server\u{0} is down").into()) }),
    );
    app.schedule(
        Schedule::every("secrets", Duration::from_secs(3600))
            .run(|_| async { Err(rustcms_core::AppError::internal("token=secret-sentinel")) }),
    );
    app
}

async fn app(db: TestDb) -> (TestApp, support::app::Users) {
    let config = rustcms_testing::test_config(db.url());
    support::app::setup_built(db, config, build).await
}

async fn work(app: &TestApp) -> usize {
    queue::run_until_idle(app.state(), None, 20).await.unwrap()
}

async fn runs_of(app: &TestApp, kind: &str) -> Vec<job_run::Model> {
    job_run::Entity::find()
        .filter(job_run::Column::Kind.eq(kind))
        .order_by_asc(job_run::Column::FinishedAt)
        .all(app.state().db())
        .await
        .unwrap()
}

db_test!(finished_jobs_are_recorded_once_per_terminal_outcome, |db: TestDb| async move {
    let (app, _) = app(db).await;
    let conn = app.state().db();
    let deploy = queue::enqueue(conn, &Deploy { app: "web".into() }).await.unwrap();
    let flaky = queue::enqueue(conn, &Flaky).await.unwrap();
    let refused = queue::enqueue(conn, &Refused).await.unwrap();
    // The description is cut to 200 characters, control characters removed.
    let row = job::Entity::find_by_id(deploy).one(conn).await.unwrap().unwrap();
    let description = row.description.unwrap();
    assert!(description.starts_with("deploy web#12x") && description.chars().count() == 200, "{description}");
    for _ in 0..3 {
        work(&app).await;
    }
    let done = runs_of(&app, "test.deploy").await;
    assert_eq!(done.len(), 1);
    assert_eq!((done[0].job_id, done[0].status.as_str(), done[0].attempts), (deploy, "done", 1));
    assert!(done[0].started_at.is_some() && done[0].duration_ms.is_some());
    // A retried run is not history; only its final outcome is.
    let flaky_runs = runs_of(&app, "test.flaky").await;
    assert_eq!(flaky_runs.len(), 1, "intermediate retries write nothing");
    assert_eq!((flaky_runs[0].job_id, flaky_runs[0].status.as_str(), flaky_runs[0].attempts), (flaky, "done", 2));
    let failed = runs_of(&app, "test.refused").await;
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].status, "failed");
    assert_eq!(failed[0].last_error.as_deref(), Some("smtp_rejected: the server refused the message"));
    assert!(job::Entity::find_by_id(refused).one(conn).await.unwrap().is_some(), "a failed job stays for retry");
    // Retried, it fails again: a second row under the same job.
    let txn = rustcms_core::db::begin_write(conn).await.unwrap();
    let clock = manage::Clock::new(5);
    manage::retry(&txn, refused, &clock).await.unwrap().unwrap();
    txn.commit().await.unwrap();
    work(&app).await;
    assert_eq!(runs_of(&app, "test.refused").await.len(), 2);
    // Cancelled while waiting.
    let waiting = queue::enqueue(conn, &Deploy { app: "later".into() }).await.unwrap();
    let txn = rustcms_core::db::begin_write(conn).await.unwrap();
    manage::cancel(&txn, waiting, &clock).await.unwrap().unwrap();
    txn.commit().await.unwrap();
    let cancelled =
        job_run::Entity::find().filter(job_run::Column::JobId.eq(waiting)).one(conn).await.unwrap().unwrap();
    assert_eq!(cancelled.status, "cancelled");
    // No secret ever reaches the history.
    let all = job_run::Entity::find().all(conn).await.unwrap();
    assert!(all.iter().all(|r| !r.last_error.clone().unwrap_or_default().contains("secret-sentinel")));
});

db_test!(a_lease_expired_on_the_last_attempt_is_reaped_into_the_history, |db: TestDb| async move {
    let (app, _) = app(db).await;
    let conn = app.state().db();
    let id = queue::enqueue(conn, &Refused).await.unwrap();
    // Claimed by a worker that died: last attempt, lease long over.
    job::Entity::update_many()
        .col_expr(job::Column::Attempts, Expr::value(1))
        .col_expr(job::Column::ReservedBy, Expr::value("dead"))
        .col_expr(job::Column::ReservedUntil, Expr::value(chrono::Utc::now() - chrono::Duration::minutes(5)))
        .filter(job::Column::Id.eq(id))
        .exec(conn)
        .await
        .unwrap();
    work(&app).await;
    let rows = runs_of(&app, "test.refused").await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].last_error.as_deref(), Some("the lease expired during the last attempt"));
});

db_test!(no_history_when_its_retention_is_zero, |db: TestDb| async move {
    let config = rustcms_testing::test_config(db.url()).with("queue.history_retention_days", 0).unwrap();
    let (app, _) = support::app::setup_built(db, config, build).await;
    queue::enqueue(app.state().db(), &Deploy { app: "web".into() }).await.unwrap();
    work(&app).await;
    assert_eq!(job_run::Entity::find().count(app.state().db()).await.unwrap(), 0);
});

db_test!(an_at_most_once_job_stopped_by_shutdown_fails_and_never_runs_again, |db: TestDb| async move {
    let (app, _) = app(db).await;
    let conn = app.state().db();
    ONCE_RUNS.store(0, Ordering::SeqCst);
    let id = queue::enqueue(conn, &Once).await.unwrap();
    let config = QueueConfig { shutdown_secs: 0, ..QueueConfig::of(app.state().config()).unwrap() };
    let workers = Workers::start(app.state(), &config, None, 1);
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while ONCE_RUNS.load(Ordering::SeqCst) == 0 && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    workers.shutdown().await;
    let row = job::Entity::find_by_id(id).one(conn).await.unwrap().unwrap();
    assert!(row.failed_at.is_some(), "failed, not given back");
    assert_eq!(row.last_error.as_deref(), Some("interrupted by shutdown"));
    assert_eq!(work(&app).await, 0);
    assert_eq!(ONCE_RUNS.load(Ordering::SeqCst), 1, "never a second invocation");
});

db_test!(run_now_runs_closures_once_with_their_summary_or_safe_error, |db: TestDb| async move {
    let (app, users) = app(db).await;
    let conn = app.state().db();
    let run = |name: &'static str| {
        let state = app.state().clone();
        async move {
            let txn = rustcms_core::db::begin_write(state.db()).await.unwrap();
            let run = schedule::run_now(&txn, &state, name, Some(users_admin(&state).await)).await.unwrap();
            txn.commit().await.unwrap();
            run
        }
    };
    let backups = run("backups").await.unwrap();
    assert_eq!((backups.status.as_str(), backups.trigger.as_str()), ("dispatched", "manual"));
    // Still unfinished: a second Run now is refused.
    assert_eq!(run("backups").await.unwrap_err(), RunNowError::Running);
    // The schedule's own slot is skipped while it is unfinished.
    let slot = chrono::DateTime::from_timestamp(chrono::Utc::now().timestamp(), 0).unwrap();
    let claimed = schedule::testing::claim(app.state(), "backups", slot, Duration::from_secs(30)).await.unwrap();
    assert_eq!(claimed, schedule::testing::Claimed::Overlapping);
    work(&app).await;
    let done = schedule_run::Entity::find_by_id(backups.id).one(conn).await.unwrap().unwrap();
    assert_eq!(done.status, "succeeded");
    assert_eq!(done.summary, Some(format!("3 databases backed up, run {}", backups.id)), "control characters removed");
    assert!(done.finished_at.is_some() && done.duration_ms.is_some());
    // The internal job ran once: one history row, never retried.
    assert_eq!(runs_of(&app, "rustcms.schedule.run").await.len(), 1);

    let alerts = run("alerts").await.unwrap();
    work(&app).await;
    let failed = schedule_run::Entity::find_by_id(alerts.id).one(conn).await.unwrap().unwrap();
    assert_eq!(
        (failed.status.as_str(), failed.error.as_deref()),
        ("failed", Some("smtp_down: the mail server is down"))
    );
    let secrets = run("secrets").await.unwrap();
    work(&app).await;
    let generic = schedule_run::Entity::find_by_id(secrets.id).one(conn).await.unwrap().unwrap();
    assert_eq!(generic.error.as_deref(), Some("failed (see the logs)"), "an AppError's text is never kept");
    assert_eq!(run("nope").await.unwrap_err(), RunNowError::Unknown);
    let _ = users;
});

async fn users_admin(state: &rustcms_core::AppState) -> i64 {
    rustcms_core::models::user::Entity::find()
        .filter(rustcms_core::models::user::Column::Name.eq("Admin"))
        .one(state.db())
        .await
        .unwrap()
        .unwrap()
        .id
}

db_test!(run_now_of_a_job_schedule_records_the_jobs_outcome, |db: TestDb| async move {
    let (app, _) = app(db).await;
    let conn = app.state().db();
    let txn = rustcms_core::db::begin_write(conn).await.unwrap();
    let run = schedule::run_now(&txn, app.state(), "solo", Some(7)).await.unwrap().unwrap();
    txn.commit().await.unwrap();
    let job_id = run.job_id.unwrap();
    assert_eq!(job::Entity::find_by_id(job_id).one(conn).await.unwrap().unwrap().initiated_by, Some(7));
    // Without overlapping: the job is still pending.
    let txn = rustcms_core::db::begin_write(conn).await.unwrap();
    assert_eq!(schedule::run_now(&txn, app.state(), "solo", None).await.unwrap().unwrap_err(), RunNowError::Running);
    txn.rollback().await.unwrap();
    work(&app).await;
    let done = schedule_run::Entity::find_by_id(run.id).one(conn).await.unwrap().unwrap();
    assert_eq!(done.status, "succeeded", "stored from the job's own transition");
    // Queue history off or pruned: the schedule's outcome stays.
    job_run::Entity::delete_many().exec(conn).await.unwrap();
    assert_eq!(schedule_run::Entity::find_by_id(run.id).one(conn).await.unwrap().unwrap().status, "succeeded");
    // Cancelled before it ran.
    let txn = rustcms_core::db::begin_write(conn).await.unwrap();
    let run = schedule::run_now(&txn, app.state(), "deploys", None).await.unwrap().unwrap();
    manage::cancel(&txn, run.job_id.unwrap(), &manage::Clock::new(5)).await.unwrap().unwrap();
    txn.commit().await.unwrap();
    assert_eq!(schedule_run::Entity::find_by_id(run.id).one(conn).await.unwrap().unwrap().status, "cancelled");
});

db_test!(queue_and_schedules_routes, |db: TestDb| async move {
    let (app, users) = app(db).await;
    let conn = app.state().db();
    let admin = app.acting_as(&users.admin).await;
    let id = queue::enqueue(conn, &Deploy { app: "web".into() }).await.unwrap();
    let detail = admin.get(&app.admin_api(&format!("queue/jobs/{id}"))).await.json();
    assert!(detail["job"]["description"].as_str().unwrap().starts_with("deploy web"));
    assert_eq!(detail["runs"], json!([]));
    work(&app).await;
    let history = admin.get(&app.admin_api("queue/history?status=done&kind=test.deploy")).await.json();
    assert_eq!(history["data"].as_array().unwrap().len(), 1);
    assert_eq!(history["data"][0]["jobId"], json!(id.to_string()));
    let detail = admin.get(&app.admin_api(&format!("queue/jobs/{id}"))).await.json();
    assert!(detail.get("job").is_none() && detail["runs"].as_array().unwrap().len() == 1);
    admin
        .get(&app.admin_api(&format!("queue/jobs/{}", uuid::Uuid::now_v7())))
        .await
        .assert_status(StatusCode::NOT_FOUND);
    admin.get(&app.admin_api("queue/history?status=pending")).await.assert_validation_error("status");
    admin
        .get(&app.admin_api("queue/history?since=2026-10-20T10:00:00Z&until=2026-10-20T09:00:00Z"))
        .await
        .assert_validation_error("until");
    admin.get(&app.admin_api("queue/jobs?since=yesterday")).await.assert_validation_error("since");
    let overview = admin.get(&app.admin_api("queue")).await.json();
    assert_eq!(overview["lastDay"]["done"], json!(1));

    let list = admin.get(&app.admin_api("schedules")).await.json();
    let backups = list["data"].as_array().unwrap().iter().find(|s| s["name"] == "backups").unwrap().clone();
    assert_eq!(backups["action"], json!({"kind": "closure"}));
    assert!(backups["nextAt"].is_string() && backups["rule"] == json!("every 3600s"));
    let solo = list["data"].as_array().unwrap().iter().find(|s| s["name"] == "solo").unwrap().clone();
    assert_eq!(solo["action"], json!({"kind": "job", "jobKind": "test.deploy"}));
    assert_eq!(list["canManage"], json!(true));

    let response = admin.post_json(&app.admin_api("schedules/backups/run"), &json!({})).await;
    response.assert_status(StatusCode::ACCEPTED);
    let run_id = response.json()["run"]["id"].as_str().unwrap().to_owned();
    admin.post_json(&app.admin_api("schedules/backups/run"), &json!({})).await.assert_status(StatusCode::CONFLICT);
    admin.post_json(&app.admin_api("schedules/nope/run"), &json!({})).await.assert_status(StatusCode::NOT_FOUND);
    let audit =
        audit_log::Entity::find().filter(audit_log::Column::Event.eq("schedule.run")).count(conn).await.unwrap();
    assert_eq!(audit, 1);
    work(&app).await;
    let runs = admin.get(&app.admin_api("schedules/backups/runs")).await.json();
    assert_eq!(runs["data"][0]["id"], json!(run_id));
    assert_eq!(runs["data"][0]["status"], json!("succeeded"));
    assert!(runs["data"][0]["summary"].as_str().unwrap().starts_with("3 databases backed up"));
    admin.get(&app.admin_api("schedules/nope/runs")).await.assert_status(StatusCode::NOT_FOUND);
    admin.get(&app.admin_api("schedules/backups/runs?x=1")).await.assert_validation_error("x");
    // Viewers without manage see the page, not Run now.
    let editor = app.acting_as(&users.editor).await;
    editor.get(&app.admin_api("schedules")).await.assert_status(StatusCode::FORBIDDEN);
});
