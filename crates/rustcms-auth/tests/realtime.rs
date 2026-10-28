//! Server-sent events on admin routes (ADR 0027), through a real server on an
//! ephemeral port: admission, live rechecks, limits and shutdown. All
//! databases.

mod support;

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use axum::extract::Path;
use rustcms_auth::entities::session as session_entity;
use rustcms_auth::{
    ADMIN_GUARD, Actor, AdminAccess, AdminRoutes, AuthState, EventStream, LiveSession, RbacService, TargetFuture,
};
use rustcms_core::realtime::Event;
use rustcms_core::{AppBuilder, AppError, AppResult, AppState, Config, Inject};
use rustcms_migration::Migrator;
use rustcms_testing::{TestApp, TestDb, db_test};
use sea_orm::{ActiveModelTrait, EntityTrait, IntoActiveModel, Set};
use support::setup_with;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

/// Whether the stream's target may still be seen (the per-target check).
struct Visible(AtomicBool);

/// A target check written once, reused by any stream (`TargetFuture`).
fn while_visible(visible: Arc<Visible>) -> impl Fn(rustcms_core::Principal) -> TargetFuture + Send + Sync + 'static {
    move |_principal| {
        let visible = visible.clone();
        Box::pin(async move { if visible.0.load(Ordering::SeqCst) { Ok(()) } else { Err(AppError::Forbidden) } })
    }
}

async fn ticks(live: LiveSession, Path(id): Path<String>, Inject(visible): Inject<Visible>) -> AppResult<EventStream> {
    let ticks = futures::stream::unfold(0u64, move |n| async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        // Every fifth, as a producer that lost events upstream would.
        let event = if n % 5 == 4 { Event::resync() } else { Event::json("tick", &n).unwrap() };
        Some((event, n + 1))
    });
    let _ = id;
    Ok(live.stream(ticks).target(while_visible(visible)))
}

async fn silent(live: LiveSession) -> AppResult<EventStream> {
    Ok(live.stream(futures::stream::pending()))
}

fn routes() -> AdminRoutes {
    AdminRoutes::new("/live")
        .events("/ticks/{id}", AdminAccess::can("live.view"), ticks)
        .events("/silent", AdminAccess::can("live.view"), silent)
        // A handler that takes no LiveSession is still admitted by its route.
        .events("/bare", AdminAccess::can("live.view"), || async { "not a stream" })
}

fn build(config: Config) -> AppBuilder {
    let config = config
        .with("realtime.revalidate_secs", 1)
        .and_then(|c| c.with("realtime.max_per_user", 2))
        .and_then(|c| c.with("realtime.shutdown_grace_secs", 2))
        .and_then(|c| c.with("http.timeout_secs", 1))
        // Served instances: no scheduler (the framework's prune schedules).
        .and_then(|c| c.with("schedule.enabled", false))
        .unwrap();
    let mut app = AppBuilder::new(config);
    let admin = routes();
    app.provider(support::provider().admin_permissions(&admin));
    app.bind(Visible(AtomicBool::new(true)));
    admin.register(&mut app);
    app
}

/// `seed.reader` gets the admin permission of the stream.
const WATCHER: (&str, &str, &[&str]) = (ADMIN_GUARD, "watcher", &["live.view"]);
const ORIGIN: &str = "http://localhost:8000";
const PATH: &str = "/admin/api/x/live/ticks/c1";

/// Another instance of the app on the same database, served on a real port.
struct Server {
    addr: SocketAddr,
    state: AppState,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<anyhow::Result<()>>,
}

async fn serve(app: &TestApp) -> Server {
    let served = build(app.state().config().clone()).build_with_db(app.state().db().clone()).await.unwrap();
    let state = served.state().clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let task = tokio::spawn(served.serve_on(listener, async move {
        let _ = stopped.await;
    }));
    Server { addr, state, stop: Some(stop), task }
}

/// A session cookie for `user` (as signing in makes it).
async fn cookie(app: &TestApp, user: &rustcms_core::models::user::Model) -> String {
    let auth = app.state().service::<AuthState>().unwrap();
    let (token, _) =
        rustcms_auth::session::create_for_user(&auth, app.state().db(), user.id, user.auth_version).await.unwrap();
    format!("{}={token}", auth.config.cookie_name)
}

/// One SSE response read line by line.
struct Sse {
    status: u16,
    lines: tokio::io::Lines<BufReader<TcpStream>>,
}

async fn open(addr: SocketAddr, method: &str, headers: &[(&str, &str)]) -> Sse {
    open_at(addr, method, PATH, headers).await
}

async fn open_at(addr: SocketAddr, method: &str, path: &str, headers: &[(&str, &str)]) -> Sse {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let mut request = format!("{method} {path} HTTP/1.1\r\nHost: localhost:8000\r\nConnection: close\r\n");
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut lines = BufReader::new(stream).lines();
    let status_line = lines.next_line().await.unwrap().unwrap_or_default();
    let status = status_line.split(' ').nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    // Headers end with an empty line.
    while let Some(line) = lines.next_line().await.unwrap() {
        if line.trim().is_empty() {
            break;
        }
    }
    Sse { status, lines }
}

impl Sse {
    /// The next event `(name, data)` within `within`, or `None` (timeout or end).
    async fn next(&mut self, within: Duration) -> Option<(String, String)> {
        let deadline = Instant::now() + within;
        let (mut name, mut data) = (None, String::new());
        loop {
            let left = deadline.checked_duration_since(Instant::now())?;
            let line = tokio::time::timeout(left, self.lines.next_line()).await.ok()?.ok()??;
            let line = line.trim_end_matches('\r');
            if let Some(value) = line.strip_prefix("event:") {
                name = Some(value.trim().to_owned());
            } else if let Some(value) = line.strip_prefix("data:") {
                data = value.trim().to_owned();
            } else if line.is_empty()
                && let Some(name) = name.take()
            {
                return Some((name, std::mem::take(&mut data)));
            }
        }
    }

    /// Events until a `closed` one: its reason.
    async fn closed(&mut self, within: Duration) -> Option<String> {
        let deadline = Instant::now() + within;
        loop {
            let left = deadline.checked_duration_since(Instant::now())?;
            let (name, data) = self.next(left).await?;
            if name == "closed" {
                let value: serde_json::Value = serde_json::from_str(&data).unwrap();
                return value["reason"].as_str().map(str::to_owned);
            }
        }
    }
}

async fn signed_in(server: &Server, cookie: &str) -> Sse {
    open(server.addr, "GET", &[("Cookie", cookie), ("Origin", ORIGIN), ("Sec-Fetch-Site", "same-origin")]).await
}

db_test!(admission_needs_the_panels_own_origin_and_the_permission, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup_with(build, db, &[WATCHER]).await;
    let server = serve(&app).await;
    let reader = cookie(&app, &seed.reader).await;
    let member = cookie(&app, &seed.member).await;
    let get = |headers: Vec<(&'static str, String)>| {
        let headers: Vec<(&str, String)> = headers;
        async move {
            let borrowed: Vec<(&str, &str)> = headers.iter().map(|(n, v)| (*n, v.as_str())).collect();
            open(server.addr, "GET", &borrowed).await.status
        }
    };
    let with = |extra: &[(&'static str, &str)]| {
        let mut headers = vec![("Cookie", reader.clone())];
        headers.extend(extra.iter().map(|(n, v)| (*n, (*v).to_owned())));
        headers
    };
    // Same origin, however the browser says it. Each admitted stream is
    // dropped at once; its place comes back with its next (failed) write.
    let released = || tokio::time::sleep(Duration::from_millis(500));
    assert_eq!(get(with(&[("Origin", ORIGIN)])).await, 200);
    released().await;
    assert_eq!(get(with(&[("Sec-Fetch-Site", "same-origin")])).await, 200, "EventSource sends no Origin");
    released().await;
    assert_eq!(get(with(&[("Referer", "http://localhost:8000/admin/p/live")])).await, 200, "an old browser");
    released().await;
    // Anything else is refused.
    assert_eq!(get(with(&[])).await, 403, "no origin information at all");
    assert_eq!(get(with(&[("Origin", "https://evil.example")])).await, 403);
    assert_eq!(get(with(&[("Origin", "null")])).await, 403);
    assert_eq!(get(with(&[("Origin", "http://localhost:8000/admin")])).await, 403, "not an exact origin");
    assert_eq!(get(with(&[("Sec-Fetch-Site", "same-site")])).await, 403, "a sibling subdomain");
    assert_eq!(get(with(&[("Origin", ORIGIN), ("Sec-Fetch-Site", "cross-site")])).await, 403);
    assert_eq!(get(with(&[("Referer", "https://evil.example/")])).await, 403);
    // A header given twice, even one that would not decide.
    assert_eq!(get(with(&[("Origin", ORIGIN), ("Origin", ORIGIN)])).await, 403);
    assert_eq!(get(with(&[("Origin", ORIGIN), ("Referer", "http://localhost:8000/"), ("Referer", "x")])).await, 403);
    // The route admits before the handler: one without LiveSession too.
    let bare = |headers: Vec<(&'static str, String)>| async move {
        let borrowed: Vec<(&str, &str)> = headers.iter().map(|(n, v)| (*n, v.as_str())).collect();
        open_at(server.addr, "GET", "/admin/api/x/live/bare", &borrowed).await.status
    };
    assert_eq!(bare(with(&[])).await, 403, "no origin");
    assert_eq!(bare(with(&[("Origin", ORIGIN)])).await, 200);
    // Credentials and access come first.
    let same = [("Origin", ORIGIN.to_owned())];
    assert_eq!(get(same.to_vec()).await, 401, "no session");
    let mut member_headers = same.to_vec();
    member_headers.push(("Cookie", member));
    assert_eq!(get(member_headers).await, 403, "without the permission");
    let mut head = vec![("Cookie", reader.clone())];
    head.push(("Origin", ORIGIN.to_owned()));
    let borrowed: Vec<(&str, &str)> = head.iter().map(|(n, v)| (*n, v.as_str())).collect();
    assert_eq!(open(server.addr, "HEAD", &borrowed).await.status, 405, "HEAD starts nothing");
    // Admitted: events flow, longer than the request timeout (1 s).
    let mut stream = signed_in(&server, &reader).await;
    assert_eq!(stream.status, 200);
    let started = Instant::now();
    let mut last = None;
    while started.elapsed() < Duration::from_millis(1500) {
        last = stream.next(Duration::from_secs(2)).await;
        assert!(last.is_some(), "the stream stopped");
    }
    assert!(last.is_some());
    // A producer's own resync reaches the page as `event: resync`.
    let mut names = Vec::new();
    for _ in 0..6 {
        names.push(stream.next(Duration::from_secs(2)).await.unwrap().0);
    }
    assert!(names.contains(&"resync".to_owned()) && names.contains(&"tick".to_owned()), "{names:?}");
});

db_test!(an_open_stream_stops_when_access_ends, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup_with(build, db, &[WATCHER]).await;
    let server = serve(&app).await;
    let bound = Duration::from_secs(4); // revalidate (1 s) + check time, with margin
    let db = app.state().db();
    // Signed out (every session row gone).
    let mut stream = signed_in(&server, &cookie(&app, &seed.reader).await).await;
    assert!(stream.next(Duration::from_secs(2)).await.is_some());
    session_entity::Entity::delete_many().exec(db).await.unwrap();
    assert_eq!(stream.closed(bound).await.as_deref(), Some("signed_out"));
    // Credentials changed elsewhere: the session row is intact, the user's version moved.
    let mut stream = signed_in(&server, &cookie(&app, &seed.reader).await).await;
    assert!(stream.next(Duration::from_secs(2)).await.is_some());
    let user = rustcms_core::models::user::Entity::find_by_id(seed.reader.id).one(db).await.unwrap().unwrap();
    let mut bumped = user.clone().into_active_model();
    bumped.auth_version = Set(user.auth_version + 1);
    let user = bumped.update(db).await.unwrap();
    assert_eq!(stream.closed(bound).await.as_deref(), Some("signed_out"));
    // The permission removed (through RbacService, on the other instance).
    let mut stream = signed_in(&server, &cookie(&app, &user).await).await;
    assert!(stream.next(Duration::from_secs(2)).await.is_some());
    let auth = app.state().service::<AuthState>().unwrap();
    let role = rustcms_auth::entities::role::Entity::find()
        .all(db)
        .await
        .unwrap()
        .into_iter()
        .find(|r| r.name == "watcher")
        .unwrap();
    RbacService::new(db, &auth.resolver).revoke_role(Actor::System, user.id, role.id).await.unwrap();
    assert_eq!(stream.closed(bound).await.as_deref(), Some("forbidden"));
    // The target out of reach (the per-target check).
    let admin = cookie(&app, &seed.admin).await;
    let mut stream = signed_in(&server, &admin).await;
    assert!(stream.next(Duration::from_secs(2)).await.is_some());
    server.state.service::<Visible>().unwrap().0.store(false, Ordering::SeqCst);
    assert_eq!(stream.closed(bound).await.as_deref(), Some("forbidden"));
    // Already out of reach: closed before the first event.
    let mut stream = signed_in(&server, &admin).await;
    assert_eq!(stream.closed(bound).await.as_deref(), Some("forbidden"));
    server.state.service::<Visible>().unwrap().0.store(true, Ordering::SeqCst);
    // The account disabled.
    let mut stream = signed_in(&server, &admin).await;
    assert!(stream.next(Duration::from_secs(2)).await.is_some());
    let mut disabled = seed.admin.clone().into_active_model();
    disabled.is_active = Set(false);
    disabled.update(db).await.unwrap();
    assert_eq!(stream.closed(bound).await.as_deref(), Some("signed_out"));
});

db_test!(streams_are_bounded_per_user_and_released, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup_with(build, db, &[WATCHER]).await;
    let server = serve(&app).await;
    let reader = cookie(&app, &seed.reader).await;
    let mut first = signed_in(&server, &reader).await;
    let second = signed_in(&server, &reader).await;
    assert_eq!((first.status, second.status), (200, 200));
    assert!(first.next(Duration::from_secs(2)).await.is_some());
    assert_eq!(signed_in(&server, &reader).await.status, 429, "max_per_user = 2");
    // Another user is not affected.
    assert_eq!(signed_in(&server, &cookie(&app, &seed.admin).await).await.status, 200);
    // A stream whose client left releases its place by the next recheck.
    drop(second);
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(signed_in(&server, &reader).await.status, 200);
});

db_test!(a_silent_stream_left_by_its_client_frees_its_place_at_once, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup_with(build, db, &[WATCHER]).await;
    let server = serve(&app).await;
    let reader = cookie(&app, &seed.reader).await;
    let headers = [("Cookie", reader.as_str()), ("Origin", ORIGIN)];
    let first = open_at(server.addr, "GET", "/admin/api/x/live/silent", &headers).await;
    let _second = open_at(server.addr, "GET", "/admin/api/x/live/silent", &headers).await;
    assert_eq!(first.status, 200);
    assert_eq!(open_at(server.addr, "GET", "/admin/api/x/live/silent", &headers).await.status, 429);
    // Nothing is ever written to it: the closed connection is noticed by the
    // server dropping the body, well before a recheck would.
    drop(first);
    let realtime = server.state.service::<rustcms_auth::realtime::Realtime>().unwrap();
    let freed = async {
        while realtime.open_for(seed.reader.id) >= 2 {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    tokio::time::timeout(Duration::from_millis(900), freed).await.expect("released before the 1 s recheck");
});

db_test!(no_new_stream_while_stopping, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup_with(build, db, &[WATCHER]).await;
    let server = serve(&app).await;
    server.state.service::<rustcms_core::realtime::Shutdown>().unwrap().begin();
    assert_eq!(signed_in(&server, &cookie(&app, &seed.reader).await).await.status, 503);
});

db_test!(shutdown_ends_streams_within_the_grace, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup_with(build, db, &[WATCHER]).await;
    let mut server = serve(&app).await;
    let mut stream = signed_in(&server, &cookie(&app, &seed.reader).await).await;
    assert!(stream.next(Duration::from_secs(2)).await.is_some());
    let started = Instant::now();
    server.stop.take().unwrap().send(()).unwrap();
    assert_eq!(stream.closed(Duration::from_secs(3)).await.as_deref(), Some("shutdown"));
    tokio::time::timeout(Duration::from_secs(4), &mut server.task).await.expect("serve returned").unwrap().unwrap();
    assert!(started.elapsed() < Duration::from_secs(4), "within the 2 s grace, with margin");
});

#[test]
fn events_routes_are_listed_with_their_permission() {
    let routes = routes();
    let listed: Vec<String> = routes.list().into_iter().map(|(m, p, _)| format!("{m} {p}")).collect();
    assert_eq!(listed, ["GET /live/ticks/{id}", "GET /live/silent", "GET /live/bare"]);
    assert_eq!(routes.permissions().into_iter().collect::<Vec<_>>(), ["live.view"]);
}
