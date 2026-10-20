//! The scheduler (`AppBuilder::schedule`, ADR 0031): slots claimed once across
//! instances, in the claim's transaction with their job; only `serve` runs it;
//! overlap rules; shutdown. All databases. Periods of a second (the testing
//! hook), real time.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use chrono::{TimeZone, Utc};
use rustcms_core::models::{job, schedule_run, schedule_slot};
use rustcms_core::queue::{Job, JobCtx};
use rustcms_core::schedule::testing::{self, Claimed};
use rustcms_core::schedule::{Schedule, ScheduleRun};
use rustcms_core::{AppBuilder, AppCommand, AppState, Config};
use rustcms_testing::{TestApp, TestDb, db_test};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};

#[derive(serde::Serialize, serde::Deserialize)]
struct Tick;

#[async_trait::async_trait]
impl Job for Tick {
    const KIND: &'static str = "test.tick";
    async fn handle(self, _: JobCtx) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Runs of `slow` so far, and runs still in progress.
struct Runs(Arc<AtomicUsize>, Arc<AtomicUsize>);

struct OnDrop(Arc<AtomicUsize>);
impl Drop for OnDrop {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn build_with(config: Config, extra: impl FnOnce(&mut AppBuilder)) -> AppBuilder {
    // No queue workers: jobs stay pending (what `without_overlapping` sees).
    let config = config.with("queue.enabled", false).unwrap();
    let mut app = AppBuilder::new(config);
    app.schedule(testing::every("tick", Duration::from_secs(1)).job(|| Tick));
    extra(&mut app);
    app
}

fn build(config: Config) -> AppBuilder {
    build_with(config, |_| {})
}

async fn app(db: TestDb) -> TestApp {
    db.migrate::<rustcms_migration::Migrator>().await;
    TestApp::new(build, db).await
}

async fn ticks(state: &AppState) -> u64 {
    job::Entity::find().filter(job::Column::Kind.eq(Tick::KIND)).count(state.db()).await.unwrap()
}

/// Another instance of the app on the same database, serving (with its scheduler).
struct Server {
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<anyhow::Result<()>>,
}

async fn serve(app: &TestApp, build: impl FnOnce(Config) -> AppBuilder) -> Server {
    let served = build(app.state().config().clone()).build_with_db(app.state().db().clone()).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let task = tokio::spawn(served.serve_on(listener, async move {
        let _ = stopped.await;
    }));
    Server { stop: Some(stop), task }
}

impl Server {
    async fn stop(mut self) {
        self.stop.take().unwrap().send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(10), &mut self.task).await.expect("serve returned").unwrap().unwrap();
    }
}

db_test!(a_slot_is_claimed_once_across_instances, |db: TestDb| async move {
    let app = app(db).await;
    let other = build(app.state().config().clone()).build_with_db(app.state().db().clone()).await.unwrap();
    let slot = Utc.timestamp_opt(Utc::now().timestamp(), 0).unwrap();
    let grace = Duration::from_secs(30);
    // Both race for the same slot.
    let (a, b) = tokio::join!(
        testing::claim(app.state(), "tick", slot, grace),
        testing::claim(other.state(), "tick", slot, grace)
    );
    let outcomes = [a.unwrap(), b.unwrap()];
    assert_eq!(outcomes.iter().filter(|c| matches!(c, Claimed::Mine(Some(_)))).count(), 1, "{outcomes:?}");
    assert!(outcomes.contains(&Claimed::Taken), "{outcomes:?}");
    assert_eq!(ticks(app.state()).await, 1, "one job for the slot");
    // The claim names its job.
    let claim = schedule_slot::Entity::find().one(app.state().db()).await.unwrap().unwrap();
    assert!(claim.job_id.is_some());
    // A slot past the grace is not claimed.
    let old = slot - chrono::Duration::seconds(120);
    assert_eq!(testing::claim(app.state(), "tick", old, grace).await.unwrap(), Claimed::Late);
    assert_eq!(ticks(app.state()).await, 1);
});

db_test!(without_overlapping_waits_for_the_active_job, |db: TestDb| async move {
    db.migrate::<rustcms_migration::Migrator>().await;
    let app = TestApp::new(
        |config| {
            build_with(config, |app| {
                app.schedule(testing::every("solo", Duration::from_secs(1)).job(|| Tick).without_overlapping());
            })
        },
        db,
    )
    .await;
    let grace = Duration::from_secs(30);
    let now = Utc.timestamp_opt(Utc::now().timestamp(), 0).unwrap();
    let first = testing::claim(app.state(), "solo", now - chrono::Duration::seconds(2), grace).await.unwrap();
    let Claimed::Mine(Some(job_id)) = first else { panic!("{first:?}") };
    // The job is pending: the next slot (adjacent) waits.
    let second = testing::claim(app.state(), "solo", now - chrono::Duration::seconds(1), grace).await.unwrap();
    assert_eq!(second, Claimed::Overlapping);
    // The skipped slot is spent, and its run says so.
    let runs =
        schedule_run::Entity::find().filter(schedule_run::Column::Name.eq("solo")).all(app.state().db()).await.unwrap();
    let statuses: Vec<&str> = runs.iter().map(|r| r.status.as_str()).collect();
    assert!(statuses.contains(&"skipped") && statuses.contains(&"dispatched"), "{statuses:?}");
    let again = testing::claim(app.state(), "solo", now - chrono::Duration::seconds(1), grace).await.unwrap();
    assert_eq!(again, Claimed::Taken, "no other claim of a skipped slot");
    // Failed for good: the schedule resumes.
    let mut failed: job::ActiveModel =
        job::Entity::find_by_id(job_id).one(app.state().db()).await.unwrap().unwrap().into();
    failed.failed_at = Set(Some(Utc::now()));
    failed.update(app.state().db()).await.unwrap();
    assert!(matches!(testing::claim(app.state(), "solo", now, grace).await.unwrap(), Claimed::Mine(Some(_))));
});

db_test!(serve_runs_the_schedules_and_test_apps_do_not, |db: TestDb| async move {
    let app = app(db).await;
    // A TestApp (and console commands) never start the scheduler.
    tokio::time::sleep(Duration::from_millis(2200)).await;
    assert_eq!(ticks(app.state()).await, 0);
    // Two serving instances: each slot enqueues one job.
    let (a, b) = (serve(&app, build).await, serve(&app, build).await);
    tokio::time::sleep(Duration::from_millis(3500)).await;
    a.stop().await;
    b.stop().await;
    let jobs = ticks(app.state()).await;
    let slots = schedule_slot::Entity::find()
        .filter(schedule_slot::Column::Name.eq("tick"))
        .count(app.state().db())
        .await
        .unwrap();
    assert!(jobs >= 2, "{jobs} jobs");
    assert_eq!(jobs, slots, "one job per claimed slot, never two");
    // Stopped: nothing more is claimed.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(ticks(app.state()).await, jobs);
});

db_test!(a_run_never_overlaps_itself_and_stops_with_the_server, |db: TestDb| async move {
    db.migrate::<rustcms_migration::Migrator>().await;
    let (started, running) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let counters = Runs(started.clone(), running.clone());
    let build_slow = move |config: Config| {
        let (started, running) = (counters.0.clone(), counters.1.clone());
        build_with(config, move |app| {
            app.schedule(testing::every("slow", Duration::from_secs(1)).run(move |_| {
                let (started, running) = (started.clone(), running.clone());
                async move {
                    started.fetch_add(1, Ordering::SeqCst);
                    running.fetch_add(1, Ordering::SeqCst);
                    let _guard = OnDrop(running);
                    std::future::pending::<()>().await;
                    Ok(())
                }
            }));
        })
    };
    let app = TestApp::new(build, db).await;
    let server = serve(&app, build_slow).await;
    tokio::time::sleep(Duration::from_millis(3500)).await;
    // The first run never ends: later slots were skipped on this instance.
    assert_eq!(started.load(Ordering::SeqCst), 1);
    assert_eq!(running.load(Ordering::SeqCst), 1);
    // Stopping: the run gets the grace, then is aborted (its future dropped).
    server.stop().await;
    assert_eq!(running.load(Ordering::SeqCst), 0);
    // Its run was still going: recorded as unknown, not as running forever.
    let runs =
        schedule_run::Entity::find().filter(schedule_run::Column::Name.eq("slow")).all(app.state().db()).await.unwrap();
    assert_eq!(runs.len(), 1, "skipped slots on this instance are not claimed");
    assert_eq!((runs[0].status.as_str(), runs[0].error.as_deref()), ("unknown", Some("interrupted by shutdown")));
});

db_test!(scheduled_tasks_record_their_summary_and_failures, |db: TestDb| async move {
    db.migrate::<rustcms_migration::Migrator>().await;
    let build_tasks = |config: Config| {
        build_with(config, |app| {
            app.schedule(testing::every("summary", Duration::from_secs(1)).task(|ctx| async move {
                assert_eq!(ctx.trigger, rustcms_core::schedule::Trigger::Schedule);
                Ok(Some("2 certificates expire soon".to_owned()))
            }));
            app.schedule(testing::every("panics", Duration::from_secs(1)).task(|_| async { panic!("boom") }));
        })
    };
    let app = TestApp::new(build, db).await;
    let server = serve(&app, build_tasks).await;
    tokio::time::sleep(Duration::from_millis(2500)).await;
    server.stop().await;
    let of = |name: &'static str| {
        let db = app.state().db().clone();
        async move { schedule_run::Entity::find().filter(schedule_run::Column::Name.eq(name)).all(&db).await.unwrap() }
    };
    let ok = of("summary").await;
    assert!(
        ok.iter().any(|r| r.status == "succeeded" && r.summary.as_deref() == Some("2 certificates expire soon")),
        "{ok:?}"
    );
    assert!(ok.iter().all(|r| r.trigger == "schedule" && r.slot_at.is_some()));
    let panicked = of("panics").await;
    assert!(panicked.iter().any(|r| r.status == "failed" && r.error.as_deref() == Some("panicked")), "{panicked:?}");
});

db_test!(old_claims_are_pruned, |db: TestDb| async move {
    let app = app(db).await;
    let grace = Duration::from_secs(30);
    let now = Utc.timestamp_opt(Utc::now().timestamp(), 0).unwrap();
    testing::claim(app.state(), "tick", now, grace).await.unwrap();
    let old = schedule_slot::ActiveModel {
        name: Set("tick".into()),
        slot_at: Set(now - chrono::Duration::days(10)),
        claimed_at: Set(now - chrono::Duration::days(10)),
        instance: Set("old".into()),
        job_id: Set(None),
    };
    old.insert(app.state().db()).await.unwrap();
    assert_eq!(testing::prune(app.state(), now - chrono::Duration::days(7)).await.unwrap(), 1);
    assert_eq!(schedule_slot::Entity::find().count(app.state().db()).await.unwrap(), 1);
});

db_test!(pruning_handles_more_than_a_batch, |db: TestDb| async move {
    let app = app(db).await;
    let now = Utc.timestamp_opt(Utc::now().timestamp(), 0).unwrap();
    let old = now - chrono::Duration::days(10);
    let rows: Vec<schedule_slot::ActiveModel> = (0..1200)
        .map(|n| schedule_slot::ActiveModel {
            name: Set("tick".into()),
            slot_at: Set(old - chrono::Duration::seconds(n)),
            claimed_at: Set(old),
            instance: Set("old".into()),
            job_id: Set(None),
        })
        .collect();
    for chunk in rows.chunks(200) {
        schedule_slot::Entity::insert_many(chunk.to_vec()).exec_without_returning(app.state().db()).await.unwrap();
    }
    assert_eq!(testing::prune(app.state(), now - chrono::Duration::days(7)).await.unwrap(), 1200);
    assert_eq!(schedule_slot::Entity::find().count(app.state().db()).await.unwrap(), 0);
});

#[test]
fn a_malformed_schedule_section_is_an_error() {
    let config = rustcms_testing::test_config("sqlite::memory:").with("schedule.grace_secs", "soon").unwrap();
    assert!(rustcms_core::schedule::ScheduleConfig::of(&config).is_err());
    let config = rustcms_testing::test_config("sqlite::memory:").with("schedule.grace_secs", 0).unwrap();
    assert!(rustcms_core::schedule::ScheduleConfig::of(&config).is_err());
    let config = rustcms_testing::test_config("sqlite::memory:");
    assert!(rustcms_core::schedule::ScheduleConfig::of(&config).unwrap().enabled);
}

db_test!(schedule_run_enqueues_without_a_claim, |db: TestDb| async move {
    let app = app(db).await;
    let args = ScheduleRun.definition().get_matches_from(["schedule:run", "tick"]);
    ScheduleRun.run(app.state(), &args).await.unwrap();
    assert_eq!(ticks(app.state()).await, 1);
    assert_eq!(schedule_slot::Entity::find().count(app.state().db()).await.unwrap(), 0, "a manual run is no slot");
    let unknown = ScheduleRun.definition().get_matches_from(["schedule:run", "nope"]);
    assert!(ScheduleRun.run(app.state(), &unknown).await.is_err());
});

#[test]
#[should_panic(expected = "declared twice")]
fn schedule_names_are_unique() {
    let config = rustcms_testing::test_config("sqlite::memory:");
    let mut app = AppBuilder::new(config);
    app.schedule(Schedule::every("a", Duration::from_secs(60)).job(|| Tick));
    app.schedule(Schedule::every("a", Duration::from_secs(60)).job(|| Tick));
}

#[test]
fn a_job_schedule_registers_its_job_once() {
    let config = rustcms_testing::test_config("sqlite::memory:");
    let mut app = AppBuilder::new(config);
    app.job::<Tick>();
    app.schedule(Schedule::every("a", Duration::from_secs(60)).job(|| Tick));
    app.schedule(Schedule::every("b", Duration::from_secs(60)).job(|| Tick));
}
