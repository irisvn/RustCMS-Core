//! Several instances sharing Redis (feature `redis`): rate limits counted in
//! one place, periodic jobs run by one instance. Needs `RUSTCMS_TEST_REDIS`
//! (skipped otherwise, unless `RUSTCMS_TEST_REQUIRE` lists `redis`).
#![cfg(feature = "redis")]

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use rustcms_core::redis::Redis;
use rustcms_core::{AppError, AppState, Config};
use rustcms_migration::Migrator;
use rustcms_testing::{TestApp, TestDb, db_test, test_config};
use support::{auth, build};

/// The test Redis, or `None` (with a notice, or a failure when required).
fn redis_url() -> Option<String> {
    let url = std::env::var("RUSTCMS_TEST_REDIS").ok();
    if url.is_none() {
        let required = std::env::var("RUSTCMS_TEST_REQUIRE").unwrap_or_default();
        assert!(!required.split(',').any(|b| b.trim() == "redis"), "RUSTCMS_TEST_REDIS is required but not set");
        eprintln!("skipped: RUSTCMS_TEST_REDIS is not set");
    }
    url
}

/// One deployment's configuration: Redis with a prefix of its own (tests run in parallel).
fn config(db: &TestDb, url: &str) -> Config {
    test_config(db.url())
        .with("redis.url", url)
        .and_then(|c| c.with("redis.prefix", format!("rustcms-test-{}", rustcms_auth::secret::random_string(16))))
        .unwrap()
}

/// Two instances of one application: same configuration, separate pools and memory.
async fn two(db: TestDb, url: &str) -> (TestApp, TestApp) {
    let config = config(&db, url);
    let pool = db.reopen().await;
    let a = TestApp::with_config(build, config.clone(), db).await;
    let b = TestApp::with_config(build, config, pool).await;
    (a, b)
}

fn redis(app: &TestApp) -> Arc<Redis> {
    app.state().service::<Redis>().expect("Redis bound")
}

db_test!(rate_limits_are_shared_by_every_instance, migrate = Migrator, |db: TestDb| async move {
    let Some(url) = redis_url() else { return };
    let (a, b) = two(db, &url).await;
    let (la, lb) = (auth(&a), auth(&b));
    la.limiter.reserve(&[("login:x", 2)]).await.unwrap();
    lb.limiter.reserve(&[("login:x", 2)]).await.unwrap();
    let refused = la.limiter.reserve(&[("login:x", 2)]).await.unwrap_err();
    assert!(matches!(refused, AppError::TooManyRequests { retry_after } if (1..=60).contains(&retry_after)));

    // All or nothing: a full key stops the others from counting.
    for _ in 0..5 {
        assert!(lb.limiter.reserve(&[("ip:y", 5), ("login:x", 2)]).await.is_err());
    }
    for _ in 0..5 {
        la.limiter.reserve(&[("ip:y", 5)]).await.unwrap();
    }
    assert!(lb.limiter.reserve(&[("ip:y", 5)]).await.is_err());

    // A refund gives one attempt back and keeps the window; it never creates a key.
    lb.limiter.release("login:x").await.unwrap();
    la.limiter.reserve(&[("login:x", 2)]).await.unwrap();
    assert!(la.limiter.reserve(&[("login:x", 2)]).await.is_err());
    la.limiter.release("never-used").await.unwrap();
    la.limiter.reserve(&[("never-used", 1)]).await.unwrap();
    assert!(la.limiter.reserve(&[("never-used", 1)]).await.is_err());
    // `clear` starts over; a limit of zero refuses.
    lb.limiter.clear("login:x").await.unwrap();
    la.limiter.reserve(&[("login:x", 2)]).await.unwrap();
    assert!(la.limiter.reserve(&[("zero", 0)]).await.is_err());
});

db_test!(periodic_jobs_run_on_one_instance, migrate = Migrator, |db: TestDb| async move {
    let Some(url) = redis_url() else { return };
    let (a, b) = two(db, &url).await;
    let runs = Arc::new(AtomicU32::new(0));
    let job = |state: AppState, runs: Arc<AtomicU32>| async move {
        rustcms_core::jobs::exclusive(&state, "test-job", Duration::from_secs(5), || async move {
            runs.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(200)).await;
        })
        .await
    };
    let (ran_a, ran_b) = tokio::join!(job(a.state().clone(), runs.clone()), job(b.state().clone(), runs.clone()));
    assert_eq!(runs.load(Ordering::SeqCst), 1);
    assert!(ran_a != ran_b, "exactly one instance ran it");

    // A lease that expired while its job still ran is not released by that
    // job: the next owner keeps it.
    let key = redis(&a).key("lease", "slow");
    let slow = rustcms_core::jobs::exclusive(a.state(), "slow", Duration::from_millis(50), || async {
        tokio::time::sleep(Duration::from_millis(300)).await;
    });
    let next = async {
        tokio::time::sleep(Duration::from_millis(150)).await;
        rustcms_core::jobs::exclusive(b.state(), "slow", Duration::from_secs(5), || async {
            tokio::time::sleep(Duration::from_millis(400)).await;
        })
        .await
    };
    let check = async {
        tokio::time::sleep(Duration::from_millis(400)).await;
        let held: Option<String> = redis::cmd("GET").arg(&key).query_async(&mut redis(&a).connection()).await.unwrap();
        held
    };
    let (_, next_ran, held) = tokio::join!(slow, next, check);
    assert!(next_ran);
    assert!(held.is_some(), "the expired owner deleted its successor's lease");
});

/// A server that answers `+PONG` to everything: not what a limiter script returns.
async fn confused_server() -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = vec![0u8; 65_536];
                while let Ok(n) = socket.read(&mut buf).await {
                    if n == 0 {
                        break;
                    }
                    let text = String::from_utf8_lossy(&buf[..n]);
                    let commands = text.split("\r\n").filter(|l| l.starts_with('*')).count().max(1);
                    if socket.write_all("+PONG\r\n".repeat(commands).as_bytes()).await.is_err() {
                        break;
                    }
                }
            });
        }
    });
    format!("redis://{addr}")
}

db_test!(a_limiter_that_cannot_count_lets_nothing_through, migrate = Migrator, |db: TestDb| async move {
    if redis_url().is_none() {
        return;
    }
    let app = TestApp::new(build, db).await;
    let broken = Redis::connect(&confused_server().await, "rustcms-test-broken".into()).await.unwrap();
    let limiter = rustcms_auth::rate_limit::RateLimiter::default();
    limiter.use_redis(broken);
    assert!(matches!(limiter.reserve(&[("login:x", 5)]).await, Err(AppError::Internal(_))));
    let _ = app;
});

db_test!(an_unreachable_redis_stops_the_boot, migrate = Migrator, |db: TestDb| async move {
    if redis_url().is_none() {
        return;
    }
    // Nothing listens on port 9 (discard) in the test environment.
    let config = test_config(db.url()).with("redis.url", "redis://127.0.0.1:9").unwrap();
    let built = build(config).build_with_db(db.conn().clone()).await;
    assert!(built.is_err());
});
