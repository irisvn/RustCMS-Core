//! WebSocket on admin routes (ADR 0029), through a real listener: admission,
//! live rechecks, `authorize`, bounds both ways, cleanup and shutdown. All
//! databases.

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use axum::http::{Method, StatusCode};
use rustcms_auth::entities::session as session_entity;
use rustcms_auth::realtime::Realtime;
use rustcms_auth::{
    ADMIN_GUARD, Actor, AdminAccess, AdminRoutes, AdminSocket, AuthState, LiveSocket, RbacService, SendError,
    SocketMessage, SocketResponse, TargetFuture,
};
use rustcms_core::realtime::Shutdown;
use rustcms_core::{AppBuilder, AppError, AppResult, Config, Inject};
use rustcms_migration::Migrator;
use rustcms_testing::{TestApp, TestDb, TestSocket, db_test};
use sea_orm::sea_query::Expr;
use sea_orm::{ActiveModelTrait, EntityTrait, IntoActiveModel, Set};
use support::setup_with;
use tokio_tungstenite::tungstenite::Message;

/// Whether the socket's target may still be used.
struct Visible(AtomicBool);
/// Session closures dropped so far (each socket's cleanup ran).
struct Dropped(Arc<AtomicUsize>);

struct OnDrop(Arc<AtomicUsize>);
impl Drop for OnDrop {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn while_visible(visible: Arc<Visible>) -> impl Fn(rustcms_core::Principal) -> TargetFuture + Send + Sync + 'static {
    move |_principal| {
        let visible = visible.clone();
        Box::pin(async move { if visible.0.load(Ordering::SeqCst) { Ok(()) } else { Err(AppError::Forbidden) } })
    }
}

/// Echoes; `authorize` answers `ok` after a fresh check; `big` tries a
/// message over the limit; `firehose` sends until it cannot.
async fn echo(live: LiveSocket, Inject(visible): Inject<Visible>, Inject(dropped): Inject<Dropped>) -> SocketResponse {
    // Owned by the closure: counted even when it never runs (refused at open).
    let cleanup = OnDrop(dropped.0.clone());
    live.accept(move |mut socket: AdminSocket| async move {
        let _cleanup = cleanup;
        while let Some(message) = socket.recv().await {
            let reply = match &message {
                SocketMessage::Text(text) if text == "authorize" => match socket.authorize().await {
                    Ok(principal) if principal.can("live.use") => SocketMessage::Text("ok".into()),
                    Ok(_) => SocketMessage::Text("no".into()),
                    Err(_) => break,
                },
                SocketMessage::Text(text) if text == "big" => {
                    let result = socket.send(SocketMessage::Binary(vec![0; 1025])).await;
                    SocketMessage::Text(format!("{result:?}"))
                }
                SocketMessage::Text(text) if text == "firehose" => {
                    while socket.send(SocketMessage::Binary(vec![1; 1024])).await.is_ok() {}
                    break;
                }
                _ => message,
            };
            if socket.send(reply).await.is_err() {
                break;
            }
        }
    })
    .target(while_visible(visible))
}

/// Never reads: only its keepalive and its cleanup.
async fn quiet(live: LiveSocket, Inject(dropped): Inject<Dropped>) -> SocketResponse {
    let cleanup = OnDrop(dropped.0.clone());
    live.accept(move |socket| async move {
        // Held, never read: its input queue fills.
        let (_socket, _cleanup) = (socket, cleanup);
        std::future::pending::<()>().await;
    })
}

/// Says goodbye and returns: its last message is sent before the close.
async fn bye(live: LiveSocket) -> SocketResponse {
    live.accept(|socket| async move {
        for n in 0..3 {
            let _ = socket.send(SocketMessage::Text(format!("bye {n}"))).await;
        }
    })
}

async fn panics(live: LiveSocket) -> SocketResponse {
    live.accept(|_socket| async move { panic!("a session that fails") })
}

async fn rejects(_live: LiveSocket) -> AppResult<SocketResponse> {
    Err(AppError::Forbidden)
}

fn routes() -> AdminRoutes {
    AdminRoutes::new("/term")
        .socket("/echo", AdminAccess::can("live.use"), echo)
        .socket("/quiet", AdminAccess::can("live.use"), quiet)
        .socket("/bye", AdminAccess::can("live.use"), bye)
        .socket("/panics", AdminAccess::can("live.use"), panics)
        .socket("/rejects", AdminAccess::can("live.use"), rejects)
}

fn build(config: Config) -> AppBuilder {
    let config = config
        .with("realtime.socket_revalidate_secs", 1)
        .and_then(|c| c.with("realtime.max_per_user", 2))
        .and_then(|c| c.with("realtime.max_message_bytes", 1024))
        .and_then(|c| c.with("realtime.send_timeout_secs", 1))
        .and_then(|c| c.with("http.timeout_secs", 1))
        .unwrap();
    let mut app = AppBuilder::new(config);
    let admin = routes();
    app.provider(support::provider().admin_permissions(&admin));
    app.bind(Visible(AtomicBool::new(true)));
    app.bind(Dropped(Arc::default()));
    admin.register(&mut app);
    app
}

/// `seed.reader` gets the socket's permission.
const USER: (&str, &str, &[&str]) = (ADMIN_GUARD, "terminal", &["live.use"]);
const ECHO: &str = "/admin/api/x/term/echo";
const QUIET: &str = "/admin/api/x/term/quiet";
/// Rechecks every second, a 5 s check timeout at most: with margin.
const BOUND: Duration = Duration::from_secs(4);

async fn open(app: &TestApp, user: &rustcms_core::models::user::Model, path: &str) -> TestSocket {
    app.acting_as(user).await.socket(path).await.expect("socket admitted")
}

async fn echoes(socket: &mut TestSocket, text: &str) -> bool {
    socket.send_text(text).await;
    socket.next_data(Duration::from_secs(2)).await == Some(Message::text(text))
}

fn dropped(app: &TestApp) -> usize {
    app.state().service::<Dropped>().unwrap().0.load(Ordering::SeqCst)
}

/// Waits until `user` holds no place on this instance.
async fn released(app: &TestApp, user_id: i64) {
    let realtime = app.state().service::<Realtime>().unwrap();
    let freed = async {
        while realtime.open_for(user_id) > 0 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(5), freed).await.expect("the socket's place is released");
}

db_test!(handshakes_need_the_exact_origin_and_the_csrf_subprotocol, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup_with(build, db, &[USER]).await;
    let client = app.acting_as(&seed.reader).await;
    let csrf = client.csrf_token().unwrap();
    let good = format!("rustcms.v1, rustcms.csrf.{csrf}");
    // Admitted: only `rustcms.v1` comes back, never the token.
    let socket = client.socket(ECHO).await.unwrap();
    assert_eq!(socket.protocol.as_deref(), Some("rustcms.v1"));
    drop(socket);
    released(&app, seed.reader.id).await;
    // Two headers carrying the same two tokens are one list.
    let split = format!("rustcms.csrf.{csrf}");
    let headers = [("Sec-WebSocket-Protocol", split.as_str())];
    assert!(client.socket_with(ECHO, Some("rustcms.v1"), &headers).await.is_ok());
    released(&app, seed.reader.id).await;
    let refused = |r: Result<TestSocket, rustcms_testing::Refused>| r.err().map(|e| e.status);
    let forbidden = Some(StatusCode::FORBIDDEN);
    // The subprotocol list: exactly the two tokens.
    for protocols in [
        None,
        Some("rustcms.v1"),
        Some("rustcms.v1, rustcms.csrf.wrong"),
        Some(&*format!("rustcms.v2, rustcms.csrf.{csrf}")),
        Some(&*format!("{good}, other")),
        Some(&*format!("rustcms.v1,, rustcms.csrf.{csrf}")),
    ] {
        assert_eq!(refused(client.socket_with(ECHO, protocols, &[]).await), forbidden, "{protocols:?}");
    }
    // The origin: mandatory and exact; Fetch Metadata, when sent, same-origin.
    let no_origin = app.acting_as(&seed.reader).await.origin(None);
    let token = no_origin.csrf_token().unwrap();
    let protocols = format!("rustcms.v1, rustcms.csrf.{token}");
    for headers in [
        vec![],
        vec![("Origin", "https://evil.example")],
        vec![("Origin", "null")],
        vec![("Origin", "http://localhost:8000/admin")],
        vec![("Origin", "http://localhost:8000"), ("Origin", "http://localhost:8000")],
        vec![("Origin", "http://localhost:8000"), ("Sec-Fetch-Site", "same-site")],
    ] {
        let result = no_origin.socket_with(ECHO, Some(&protocols), &headers).await;
        assert_eq!(refused(result), forbidden, "{headers:?}");
    }
    let same = [("Origin", "http://localhost:8000"), ("Sec-Fetch-Site", "same-origin")];
    assert!(no_origin.socket_with(ECHO, Some(&protocols), &same).await.is_ok());
    released(&app, seed.reader.id).await;
    // The session and the permission, before anything else.
    assert_eq!(refused(app.client().socket_with(ECHO, Some(&good), &[]).await), Some(StatusCode::UNAUTHORIZED));
    assert_eq!(refused(app.acting_as(&seed.member).await.socket(ECHO).await), forbidden);
    // Not a socket: GET only, and an upgrade.
    let response = client.send(Method::POST, ECHO, None).await;
    assert_eq!(response.status, StatusCode::METHOD_NOT_ALLOWED);
    let response = client.send(Method::HEAD, ECHO, None).await;
    assert_eq!(response.status, StatusCode::METHOD_NOT_ALLOWED);
    let response = client.get(ECHO).await;
    assert!(matches!(response.status.as_u16(), 400 | 426), "{}", response.status);
    // A handler that refuses before accepting: an ordinary error, no upgrade.
    assert_eq!(refused(client.socket("/admin/api/x/term/rejects").await), forbidden);
    // Nothing refused took a place.
    assert_eq!(app.state().service::<Realtime>().unwrap().open_for(seed.reader.id), 0);
});

db_test!(messages_go_both_ways_within_the_size_limit, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup_with(build, db, &[USER]).await;
    let mut socket = open(&app, &seed.reader, ECHO).await;
    assert!(echoes(&mut socket, "hello").await);
    socket.send_binary(&[7; 1024]).await;
    assert_eq!(socket.next_data(Duration::from_secs(2)).await, Some(Message::binary(vec![7; 1024])));
    // An outgoing message over the limit is refused; the socket stays open.
    socket.send_text("big").await;
    let expected = format!("{:?}", Err::<(), _>(SendError::TooLarge));
    assert_eq!(socket.next_data(Duration::from_secs(2)).await, Some(Message::text(expected)));
    assert!(echoes(&mut socket, "still open").await);
    // It outlives the HTTP timeout (1 s): the request ended with the 101.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(echoes(&mut socket, "later").await);
    // An incoming one over the limit ends it (1009).
    use futures_util::SinkExt;
    let _ = socket.inner.send(Message::binary(vec![0; 1025])).await;
    assert_eq!(socket.closed(BOUND).await.map(|c| c.0), Some(1009));
    released(&app, seed.reader.id).await;
    assert_eq!(dropped(&app), 1, "the session closure was dropped");
});

db_test!(an_open_socket_ends_when_access_ends, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup_with(build, db, &[USER]).await;
    let db = app.state().db();
    let closes = |code: u16, reason: &'static str| Some((code, reason.to_owned()));
    // Signed out (every session row gone).
    let mut socket = open(&app, &seed.reader, ECHO).await;
    assert!(echoes(&mut socket, "a").await);
    session_entity::Entity::delete_many().exec(db).await.unwrap();
    assert_eq!(socket.closed(BOUND).await, closes(4401, "signed_out"));
    // The session expired.
    let mut socket = open(&app, &seed.reader, ECHO).await;
    assert!(echoes(&mut socket, "a").await);
    session_entity::Entity::update_many()
        .col_expr(session_entity::Column::ExpiresAt, Expr::value(chrono::Utc::now() - chrono::Duration::minutes(1)))
        .exec(db)
        .await
        .unwrap();
    assert_eq!(socket.closed(BOUND).await, closes(4401, "signed_out"));
    // Credentials changed elsewhere: the session row intact, the user's version moved.
    let mut socket = open(&app, &seed.reader, ECHO).await;
    assert!(echoes(&mut socket, "a").await);
    let user = rustcms_core::models::user::Entity::find_by_id(seed.reader.id).one(db).await.unwrap().unwrap();
    let mut bumped = user.clone().into_active_model();
    bumped.auth_version = Set(user.auth_version + 1);
    let user = bumped.update(db).await.unwrap();
    assert_eq!(socket.closed(BOUND).await, closes(4401, "signed_out"));
    // The permission removed on another instance (through RbacService).
    let mut socket = open(&app, &user, ECHO).await;
    assert!(echoes(&mut socket, "a").await);
    let other = build(app.state().config().clone()).build_with_db(db.clone()).await.unwrap();
    let auth = other.state().service::<AuthState>().unwrap();
    let role = rustcms_auth::entities::role::Entity::find()
        .all(db)
        .await
        .unwrap()
        .into_iter()
        .find(|r| r.name == "terminal")
        .unwrap();
    RbacService::new(db, &auth.resolver).revoke_role(Actor::System, user.id, role.id).await.unwrap();
    assert_eq!(socket.closed(BOUND).await, closes(4403, "forbidden"));
    // The target out of reach, while open and already at open.
    let mut socket = open(&app, &seed.admin, ECHO).await;
    assert!(echoes(&mut socket, "a").await);
    let visible = app.state().service::<Visible>().unwrap();
    visible.0.store(false, Ordering::SeqCst);
    assert_eq!(socket.closed(BOUND).await, closes(4403, "forbidden"));
    let mut socket = open(&app, &seed.admin, ECHO).await;
    assert_eq!(socket.closed(BOUND).await, closes(4403, "forbidden"));
    visible.0.store(true, Ordering::SeqCst);
    // The account disabled.
    let mut socket = open(&app, &seed.admin, ECHO).await;
    assert!(echoes(&mut socket, "a").await);
    let mut disabled = seed.admin.clone().into_active_model();
    disabled.is_active = Set(false);
    disabled.update(db).await.unwrap();
    assert_eq!(socket.closed(BOUND).await, closes(4401, "signed_out"));
    released(&app, seed.admin.id).await;
    released(&app, seed.reader.id).await;
    // Every closure was dropped, including the one refused at open (never run).
    assert_eq!(dropped(&app), 7);
});

db_test!(authorize_gives_a_fresh_principal_and_ends_a_revoked_socket, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup_with(build, db, &[USER]).await;
    let mut socket = open(&app, &seed.reader, ECHO).await;
    socket.send_text("authorize").await;
    assert_eq!(socket.next_data(Duration::from_secs(2)).await, Some(Message::text("ok")));
    let db = app.state().db();
    let auth = app.state().service::<AuthState>().unwrap();
    let role = rustcms_auth::entities::role::Entity::find()
        .all(db)
        .await
        .unwrap()
        .into_iter()
        .find(|r| r.name == "terminal")
        .unwrap();
    RbacService::new(db, &auth.resolver).revoke_role(Actor::System, seed.reader.id, role.id).await.unwrap();
    // The command is checked when it runs, not at the next recheck.
    let started = Instant::now();
    socket.send_text("authorize").await;
    assert_eq!(socket.closed(BOUND).await.map(|c| c.0), Some(4403));
    assert!(started.elapsed() < Duration::from_secs(1), "{:?}", started.elapsed());
});

db_test!(sockets_are_bounded_and_always_released, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup_with(build, db, &[USER]).await;
    let first = open(&app, &seed.reader, QUIET).await;
    let mut second = open(&app, &seed.reader, ECHO).await;
    let third = app.acting_as(&seed.reader).await.socket(ECHO).await;
    assert_eq!(third.err().map(|e| e.status), Some(StatusCode::TOO_MANY_REQUESTS), "max_per_user = 2");
    // A session that never reads still sees its client leave.
    drop(first);
    assert!(echoes(&mut second, "a").await);
    let dropped_before = dropped(&app);
    let realtime = app.state().service::<Realtime>().unwrap();
    let freed = async {
        while realtime.open_for(seed.reader.id) > 1 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(3), freed).await.expect("the quiet socket's place came back");
    assert!(dropped_before <= dropped(&app));
    // A client that floods a session that does not read: lagging (4008).
    let mut flooding = open(&app, &seed.reader, QUIET).await;
    use futures_util::SinkExt;
    for _ in 0..200 {
        if flooding.inner.send(Message::binary(vec![0; 1000])).await.is_err() {
            break;
        }
    }
    // The close frame is best effort: the server drops a connection with
    // unread input, which may reset it first.
    let code = flooding.closed(BOUND).await.map(|c| c.0);
    assert!(matches!(code, Some(4008) | None), "{code:?}");
    drop(second);
    released(&app, seed.reader.id).await;
});

db_test!(a_client_that_stops_reading_is_dropped, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup_with(build, db, &[USER]).await;
    let mut socket = open(&app, &seed.reader, ECHO).await;
    socket.send_text("firehose").await;
    // Never read: the server's sends stall (send_timeout_secs = 1), then the
    // socket ends and its place comes back.
    let started = Instant::now();
    released(&app, seed.reader.id).await;
    assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
    assert_eq!(dropped(&app), 1);
});

db_test!(a_session_that_returns_sends_its_last_messages_then_closes, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup_with(build, db, &[USER]).await;
    let mut socket = open(&app, &seed.reader, "/admin/api/x/term/bye").await;
    for n in 0..3 {
        assert_eq!(socket.next_data(Duration::from_secs(2)).await, Some(Message::text(format!("bye {n}"))));
    }
    assert_eq!(socket.closed(BOUND).await.map(|c| c.0), Some(1000));
    released(&app, seed.reader.id).await;
});

db_test!(a_failing_session_closes_with_an_error, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup_with(build, db, &[USER]).await;
    let mut socket = open(&app, &seed.reader, "/admin/api/x/term/panics").await;
    assert_eq!(socket.closed(BOUND).await, Some((1011, "error".to_owned())));
    released(&app, seed.reader.id).await;
});

db_test!(shutdown_closes_sockets_and_refuses_new_ones, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup_with(build, db, &[USER]).await;
    let mut echo = open(&app, &seed.reader, ECHO).await;
    let mut quiet = open(&app, &seed.admin, QUIET).await;
    assert!(echoes(&mut echo, "a").await);
    let shutdown = app.state().service::<Shutdown>().unwrap();
    shutdown.begin();
    assert_eq!(echo.closed(BOUND).await, Some((1001, "shutdown".to_owned())));
    assert_eq!(quiet.closed(BOUND).await, Some((1001, "shutdown".to_owned())));
    assert!(shutdown.drain(Duration::from_secs(2)).await, "every socket ended within the grace");
    assert_eq!(dropped(&app), 2);
    let refused = app.acting_as(&seed.reader).await.socket(ECHO).await;
    assert_eq!(refused.err().map(|e| e.status), Some(StatusCode::SERVICE_UNAVAILABLE));
});

#[test]
fn socket_routes_are_listed_with_their_permission() {
    let routes = routes();
    let listed: Vec<String> = routes.list().into_iter().map(|(m, p, _)| format!("{m} {p}")).collect();
    assert_eq!(listed, ["GET /term/echo", "GET /term/quiet", "GET /term/bye", "GET /term/panics", "GET /term/rejects"]);
    assert_eq!(routes.permissions().into_iter().collect::<Vec<_>>(), ["live.use"]);
}
