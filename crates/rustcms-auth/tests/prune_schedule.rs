//! `rustcms.auth-prune` (ADR 0031): the hourly auth cleanup is a framework
//! schedule, run only by `serve` (or `schedule:run`), never at boot, by a
//! command or in tests. All databases.

mod support;

use std::time::Duration;

use chrono::Utc;
use rustcms_auth::entities::{session, user_token};
use rustcms_core::models::schedule_run;
use rustcms_core::schedule::{ScheduleList, ScheduleRun, Schedules};
use rustcms_core::{AppCommand, AppState};
use rustcms_migration::Migrator;
use rustcms_testing::{TestApp, TestDb, db_test};
use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, EntityTrait, PaginatorTrait, QueryFilter,
};
use support::{auth, setup};

const NAME: &str = "rustcms.auth-prune";

/// One expired session and one expired reset link.
async fn expired_rows(app: &TestApp, seed: &support::Seed) {
    let db = app.state().db();
    rustcms_auth::session::create_for_user(&auth(app), db, seed.member.id, seed.member.auth_version).await.unwrap();
    session::Entity::update_many()
        .col_expr(session::Column::ExpiresAt, Expr::value(Utc::now() - chrono::Duration::minutes(1)))
        .filter(sea_orm::Condition::all())
        .exec(db)
        .await
        .unwrap();
    let now = Utc::now();
    user_token::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        user_id: Set(seed.member.id),
        purpose: Set("password_reset".into()),
        email: Set(seed.member.email.clone()),
        auth_version: Set(seed.member.auth_version),
        token_hash: Set("x".repeat(64)),
        request_id: Set(uuid::Uuid::now_v7()),
        requested_at: Set(now - chrono::Duration::hours(2)),
        issued_at: Set(now - chrono::Duration::hours(2)),
        expires_at: Set(now - chrono::Duration::minutes(1)),
        sent_at: Set(Some(now - chrono::Duration::hours(2))),
    }
    .insert(db)
    .await
    .unwrap();
}

async fn counts(state: &AppState) -> (u64, u64) {
    let db = state.db();
    (session::Entity::find().count(db).await.unwrap(), user_token::Entity::find().count(db).await.unwrap())
}

async fn command(state: &AppState, cmd: &dyn AppCommand, args: &[&str]) -> anyhow::Result<()> {
    cmd.run(state, &cmd.definition().get_matches_from(args)).await
}

async fn runs(state: &AppState) -> Vec<schedule_run::Model> {
    schedule_run::Entity::find().filter(schedule_run::Column::Name.eq(NAME)).all(state.db()).await.unwrap()
}

db_test!(auth_prune_is_a_framework_schedule, migrate = Migrator, |db: TestDb| async move {
    let (app, _) = setup(db).await;
    let schedules = app.state().service::<Schedules>().unwrap();
    let schedule = schedules.get(NAME).expect("registered");
    assert_eq!(schedule.describe(), "every 3600s");
    assert_eq!(schedule.run_timeout(), Duration::from_secs(1800));
});

db_test!(nothing_is_pruned_at_boot_or_by_a_command, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    expired_rows(&app, &seed).await;
    let before = counts(app.state()).await;
    assert_eq!(before, (1, 1));
    // Another process booting on the same database (a command, a worker).
    let other = support::build(app.state().config().clone()).build_with_db(app.state().db().clone()).await.unwrap();
    command(other.state(), &ScheduleList, &["schedule:list"]).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(counts(app.state()).await, before, "no cleanup outside the scheduler");
    assert!(runs(app.state()).await.is_empty());
});

db_test!(schedule_run_prunes_and_records_its_summary, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    expired_rows(&app, &seed).await;
    command(app.state(), &ScheduleRun, &["schedule:run", NAME]).await.unwrap();
    assert_eq!(counts(app.state()).await, (0, 0));
    let runs = runs(app.state()).await;
    assert_eq!(runs.len(), 1);
    assert_eq!((runs[0].status.as_str(), runs[0].trigger.as_str()), ("succeeded", "manual"));
    assert_eq!(runs[0].summary.as_deref(), Some("1 sessions, 0 JWT sign-ins, 1 links, 0 audit entries"));
});

db_test!(a_failing_cleanup_does_not_stop_the_others, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    expired_rows(&app, &seed).await;
    // The JWT cleanup (second of four) fails: its tables are gone.
    let conn = app.state().db();
    conn.execute_unprepared("DROP TABLE jwt_refresh_tokens").await.unwrap();
    conn.execute_unprepared("DROP TABLE jwt_families").await.unwrap();
    let err = command(app.state(), &ScheduleRun, &["schedule:run", NAME]).await.unwrap_err();
    assert!(err.to_string().contains("some cleanups failed"), "{err}");
    assert_eq!(counts(app.state()).await, (0, 0), "the parts before and after it ran");
    let runs = runs(app.state()).await;
    assert_eq!(
        (runs[0].status.as_str(), runs[0].error.as_deref()),
        ("failed", Some("prune_failed: some cleanups failed (see the logs)"))
    );
});

db_test!(serve_without_the_scheduler_prunes_nothing, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    expired_rows(&app, &seed).await;
    let config = app.state().config().clone().with("schedule.enabled", false).unwrap();
    let served = support::build(config).build_with_db(app.state().db().clone()).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let task = tokio::spawn(served.serve_on(listener, async move {
        let _ = stopped.await;
    }));
    tokio::time::sleep(Duration::from_millis(500)).await;
    stop.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(10), task).await.unwrap().unwrap().unwrap();
    assert_eq!(counts(app.state()).await, (1, 1));
    assert!(runs(app.state()).await.is_empty());
});
