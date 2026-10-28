//! The framework's cleanups `rustcms.auth-prune` and
//! `rustcms.idempotency-prune` (ADR 0031): only a serving instance with the
//! scheduler runs them. Tokio's time (which the scheduler reads through
//! `TokioClock`) is advanced past an hour boundary, more than an hour after
//! boot: a boot-time loop would have run by then. All databases.

mod support;

use std::time::Duration;

use chrono::{DateTime, Utc};
use rustcms_auth::entities::session;
use rustcms_core::models::{schedule_run, schedule_state};
use rustcms_core::schedule::ScheduleList;
use rustcms_core::schedule::testing::TokioClock;
use rustcms_core::{AppCommand, AppState, Config};
use rustcms_testing::{TestApp, TestDb, db_test};
use sea_orm::sea_query::{Alias, Expr, Query};
use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, PaginatorTrait, QueryFilter, Statement};

const AUTH: &str = "rustcms.auth-prune";
const IDEMPOTENCY: &str = "rustcms.idempotency-prune";

/// Half past the current hour: the next slot is half an hour away. Near the
/// real time, so the scheduler's history pruning (which also wakes on the
/// jump) keeps the runs it just recorded.
fn start() -> DateTime<Utc> {
    DateTime::from_timestamp(Utc::now().timestamp().div_euclid(3600) * 3600 + 1800, 0).unwrap()
}

/// Moves tokio's time one second past the second hour boundary of `clock`
/// (within the slot's grace, and more than an hour: what a boot loop's first
/// tick waited), then lets the woken tasks work in real time.
async fn cross_an_hour(clock: &TokioClock) {
    let now = clock.now();
    let boundary = DateTime::from_timestamp((now.timestamp().div_euclid(3600) + 2) * 3600 + 1, 0).unwrap();
    tokio::time::pause();
    // One poll moves the clock (then `advance` yields): nothing awaits while
    // time is paused, so tokio never auto-advances it further.
    let mut advance = std::pin::pin!(tokio::time::advance((boundary - now).to_std().unwrap()));
    let _ = std::future::Future::poll(advance.as_mut(), &mut std::task::Context::from_waker(std::task::Waker::noop()));
    tokio::time::resume();
    tokio::time::sleep(Duration::from_millis(500)).await;
}

async fn app(db: TestDb) -> (TestApp, support::app::Users) {
    let config = rustcms_testing::test_config(db.url()).with("queue.enabled", false).unwrap();
    support::app::setup_with(db, config).await
}

/// An expired session and an expired idempotency receipt.
async fn expired_rows(app: &TestApp, users: &support::app::Users) {
    let db = app.state().db();
    let auth = app.state().service::<rustcms_auth::AuthState>().unwrap();
    rustcms_auth::session::create_for_user(&auth, db, users.admin.id, users.admin.auth_version).await.unwrap();
    let past = Utc::now() - chrono::Duration::minutes(1);
    session::Entity::update_many()
        .col_expr(session::Column::ExpiresAt, Expr::value(past))
        .filter(sea_orm::Condition::all())
        .exec(db)
        .await
        .unwrap();
    let insert = Query::insert()
        .into_table(Alias::new("idempotency_keys"))
        .columns(["id", "owner", "request_hash", "result", "created_at", "expires_at"].map(Alias::new))
        .values_panic([
            "expired".into(),
            "admin:1".into(),
            "x".into(),
            "{}".into(),
            (past - chrono::Duration::days(1)).into(),
            past.into(),
        ])
        .to_owned();
    db.execute(&insert).await.unwrap();
}

/// Sessions and idempotency receipts left.
async fn counts(state: &AppState) -> (u64, u64) {
    let db = state.db();
    let backend = db.get_database_backend();
    let row = db
        .query_one_raw(Statement::from_string(backend, "SELECT COUNT(*) AS n FROM idempotency_keys"))
        .await
        .unwrap()
        .unwrap();
    (session::Entity::find().count(db).await.unwrap(), row.try_get::<i64>("", "n").unwrap() as u64)
}

async fn runs(state: &AppState, name: &str) -> Vec<schedule_run::Model> {
    schedule_run::Entity::find().filter(schedule_run::Column::Name.eq(name)).all(state.db()).await.unwrap()
}

/// Another instance on the same database, serving, its scheduler reading `clock`.
struct Server {
    stop: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<anyhow::Result<()>>,
}

async fn serve(app: &TestApp, config: Config, clock: &TokioClock) -> Server {
    let enabled = rustcms_core::schedule::ScheduleConfig::of(&config).unwrap().enabled;
    let mut served = support::app::build(config);
    served.bind(clock.clone());
    let served = served.build_with_db(app.state().db().clone()).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let task = tokio::spawn(served.serve_on(listener, async move {
        let _ = stopped.await;
    }));
    // With the scheduler: each schedule has its state row, then reads its
    // clock (right after) before the jump. Without it, nothing to wait for.
    let wanted = if enabled { 2 } else { 0 };
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while schedule_state::Entity::find()
        .filter(schedule_state::Column::Name.is_in([AUTH, IDEMPOTENCY]))
        .count(app.state().db())
        .await
        .unwrap()
        < wanted
    {
        assert!(tokio::time::Instant::now() < deadline, "the scheduler did not start");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    Server { stop, task }
}

impl Server {
    async fn stop(self) {
        self.stop.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(10), self.task).await.expect("serve returned").unwrap().unwrap();
    }
}

db_test!(nothing_is_pruned_at_boot_or_by_a_command, |db: TestDb| async move {
    let (app, users) = app(db).await;
    expired_rows(&app, &users).await;
    assert_eq!(counts(app.state()).await, (1, 1));
    // Another process booting on the same database (a command, a worker).
    let other =
        support::app::build(app.state().config().clone()).build_with_db(app.state().db().clone()).await.unwrap();
    ScheduleList.run(other.state(), &ScheduleList.definition().get_matches_from(["schedule:list"])).await.unwrap();
    cross_an_hour(&TokioClock::starting_at(start())).await;
    assert_eq!(counts(app.state()).await, (1, 1), "no cleanup outside the scheduler");
    assert!(runs(app.state(), AUTH).await.is_empty() && runs(app.state(), IDEMPOTENCY).await.is_empty());
    drop(other);
});

db_test!(serve_without_the_scheduler_prunes_nothing, |db: TestDb| async move {
    let (app, users) = app(db).await;
    expired_rows(&app, &users).await;
    let clock = TokioClock::starting_at(start());
    let config = app.state().config().clone().with("schedule.enabled", false).unwrap();
    let server = serve(&app, config, &clock).await;
    cross_an_hour(&clock).await;
    server.stop().await;
    assert_eq!(counts(app.state()).await, (1, 1));
    assert!(runs(app.state(), AUTH).await.is_empty() && runs(app.state(), IDEMPOTENCY).await.is_empty());
});

db_test!(serve_with_the_scheduler_prunes_once_per_slot, |db: TestDb| async move {
    let (app, users) = app(db).await;
    expired_rows(&app, &users).await;
    let start = start();
    let clock = TokioClock::starting_at(start);
    let server = serve(&app, app.state().config().clone(), &clock).await;
    // Nothing before the slot.
    assert_eq!(counts(app.state()).await, (1, 1));
    cross_an_hour(&clock).await;
    // The runs finish in real time.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let (auth, idem) = (runs(app.state(), AUTH).await, runs(app.state(), IDEMPOTENCY).await);
        let done = |r: &[schedule_run::Model]| r.iter().any(|r| r.status != "running");
        if done(&auth) && done(&idem) {
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "{auth:?} {idem:?}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    server.stop().await;
    assert_eq!(counts(app.state()).await, (0, 0));
    let slot = DateTime::from_timestamp(start.timestamp() + 1800 + 3600, 0).unwrap();
    for (name, summary) in
        [(AUTH, "1 sessions, 0 JWT sign-ins, 0 links, 0 audit entries"), (IDEMPOTENCY, "1 expired keys")]
    {
        let runs = runs(app.state(), name).await;
        assert_eq!(runs.len(), 1, "one slot, one run: {runs:?}");
        let run = &runs[0];
        assert_eq!(
            (run.status.as_str(), run.trigger.as_str(), run.summary.as_deref(), run.slot_at),
            ("succeeded", "schedule", Some(summary), Some(slot))
        );
    }
});
