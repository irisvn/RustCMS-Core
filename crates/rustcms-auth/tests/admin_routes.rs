//! `AdminRoutes`: JSON routes for the panel's own pages on its session, each
//! with its access, checked before the handler reads anything (ADR 0026).
//! All databases.

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::body::Body;
use axum::extract::Path;
use axum::http::{Method, Request, StatusCode};
use rustcms_auth::{ADMIN_GUARD, AdminAccess, AdminRoutes, Auth};
use rustcms_core::{AppBuilder, Config, Json};
use rustcms_migration::Migrator;
use rustcms_testing::admin_routes::{Case, assert_access};
use rustcms_testing::{TestDb, db_test, test_config};
use sea_orm::{ActiveModelTrait, IntoActiveModel, Set};
use serde::Deserialize;
use serde_json::json;
use support::setup_with;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Restart {
    #[allow(dead_code)]
    force: bool,
}

fn routes() -> AdminRoutes {
    AdminRoutes::new("/docker")
        .get("/me", AdminAccess::authenticated(), |auth: Auth| async move { Json(json!({"id": auth.0.user.id})) })
        .get("/containers", AdminAccess::can("docker.view"), || async { Json(json!({"containers": []})) })
        .post(
            "/containers/{id}/restart",
            AdminAccess::can("docker.manage"),
            |Path(id): Path<String>, Json(_): Json<Restart>| async move { Json(json!({"restarted": id})) },
        )
        // Two methods on one path, each with its own permission.
        .get("/stacks", AdminAccess::can("docker.view"), || async { "read" })
        .post("/stacks", AdminAccess::can("docker.manage"), || async { "written" })
        .nest("/logs", AdminRoutes::scope().get("/{id}", AdminAccess::can("docker.view"), || async { "log" }))
}

fn build(config: Config) -> AppBuilder {
    let mut app = AppBuilder::new(config);
    let admin = routes();
    app.provider(support::provider().admin_permissions(&admin));
    admin.register(&mut app);
    app
}

/// `seed.reader` gets an admin role with both Docker permissions.
const OPERATOR: (&str, &str, &[&str]) = (ADMIN_GUARD, "operator", &["docker.view", "docker.manage"]);

db_test!(access_is_checked_before_the_request_is_read, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup_with(build, db, &[OPERATOR]).await;
    let anonymous = app.client();
    // An anonymous visitor with a valid CSRF token: unsafe methods reach the access check.
    let visitor = app.client();
    visitor.fetch_csrf().await;
    let member = app.acting_as(&seed.member).await;
    let operator = app.acting_as(&seed.reader).await;
    let admin = app.acting_as(&seed.admin).await;
    // An API token (or a JWT) is not an admin credential.
    let token = app.with_token(&seed.admin, &["*"]).await;
    let restart = |id: &str| format!("/docker/containers/{id}/restart");
    assert_access(
        &app,
        &routes(),
        &[
            Case::new(Method::GET, "/docker/me")
                .expect("anonymous", &anonymous, StatusCode::UNAUTHORIZED)
                .expect("token", &token, StatusCode::UNAUTHORIZED)
                .expect("member", &member, StatusCode::OK),
            Case::new(Method::GET, "/docker/containers")
                .expect("anonymous", &anonymous, StatusCode::UNAUTHORIZED)
                .expect("token", &token, StatusCode::UNAUTHORIZED)
                .expect("member", &member, StatusCode::FORBIDDEN)
                .expect("operator", &operator, StatusCode::OK)
                .expect("super admin", &admin, StatusCode::OK),
            // `{}` is not a valid body: refused before it is parsed, accepted only when allowed.
            Case::new(Method::POST, &restart("web"))
                .expect("visitor", &visitor, StatusCode::UNAUTHORIZED)
                .expect("member", &member, StatusCode::FORBIDDEN)
                .expect("operator", &operator, StatusCode::UNPROCESSABLE_ENTITY),
            Case::new(Method::GET, "/docker/stacks").expect("member", &member, StatusCode::FORBIDDEN).expect(
                "operator",
                &operator,
                StatusCode::OK,
            ),
            Case::new(Method::POST, "/docker/stacks")
                .expect("visitor", &visitor, StatusCode::UNAUTHORIZED)
                .expect("member", &member, StatusCode::FORBIDDEN)
                .expect("operator", &operator, StatusCode::OK),
            Case::new(Method::GET, "/docker/logs/7").expect("member", &member, StatusCode::FORBIDDEN).expect(
                "operator",
                &operator,
                StatusCode::OK,
            ),
        ],
    )
    .await;
    // Allowed and valid.
    let done = operator.post_json("/admin/api/x/docker/containers/web/restart", &json!({"force": true})).await;
    done.assert_status(StatusCode::OK).assert_json("/restarted", json!("web"));
    // The body of a refused request is never read.
    let polled = Arc::new(AtomicBool::new(false));
    let seen = polled.clone();
    let body = Body::from_stream(futures::stream::poll_fn(move |_| {
        seen.store(true, Ordering::SeqCst);
        std::task::Poll::Ready(None::<Result<Vec<u8>, std::io::Error>>)
    }));
    let request =
        Request::post("/admin/api/x/docker/containers/web/restart").header("content-type", "application/json");
    member.request(request.body(body).unwrap()).await.assert_status(StatusCode::FORBIDDEN);
    assert!(!polled.load(Ordering::SeqCst), "the body was read before access was checked");
});

db_test!(the_admin_guard_applies_first, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup_with(build, db, &[OPERATOR]).await;
    let url = "/admin/api/x/docker/stacks";
    // A write without the CSRF token, or from another origin, is refused before access.
    let operator = app.acting_as(&seed.reader).await;
    operator.set_csrf(None);
    operator
        .post_json(url, &json!({}))
        .await
        .assert_status(StatusCode::FORBIDDEN)
        .assert_json("/code", json!("csrf_mismatch"));
    let foreign = app.acting_as(&seed.reader).await.origin(Some("https://evil.example"));
    foreign.post_json(url, &json!({})).await.assert_status(StatusCode::FORBIDDEN);
    // Responses are private.
    let ok = app.acting_as(&seed.reader).await.get(url).await;
    ok.assert_status(StatusCode::OK);
    assert_eq!(ok.header("cache-control").as_deref(), Some("private, no-store"));
    // HEAD follows GET; an undeclared method is 405.
    let operator = app.acting_as(&seed.reader).await;
    operator.send(Method::HEAD, url, None).await.assert_status(StatusCode::OK);
    app.client().send(Method::HEAD, url, None).await.assert_status(StatusCode::UNAUTHORIZED);
    operator.send(Method::PUT, url, Some(&json!({}))).await.assert_status(StatusCode::METHOD_NOT_ALLOWED);
    // Credentials changed (another device changed the password): the session is anonymous.
    let mut user = seed.reader.clone().into_active_model();
    user.auth_version = Set(seed.reader.auth_version + 1);
    user.update(app.state().db()).await.unwrap();
    operator.get(url).await.assert_status(StatusCode::UNAUTHORIZED);
    // An unknown route under the namespace is the admin API's 404.
    app.acting_as(&seed.admin).await.get("/admin/api/x/nothing").await.assert_status(StatusCode::NOT_FOUND);
});

db_test!(routes_are_described_and_their_permissions_declared, migrate = Migrator, |db: TestDb| async move {
    let (app, _) = setup_with(build, db, &[]).await;
    let declared: Vec<(String, String)> = support::auth(&app)
        .permissions
        .all()
        .into_iter()
        .filter(|((_, name), _)| name.starts_with("docker."))
        .map(|((guard, name), group)| (format!("{guard} {name}"), group.unwrap_or_default()))
        .collect();
    assert_eq!(
        declared,
        [("admin docker.manage", "docker"), ("admin docker.view", "docker")].map(|(n, g)| (n.to_owned(), g.to_owned())),
        "declared in the admin guard only"
    );
    let described: Vec<String> = build(test_config("sqlite::memory:"))
        .described_routes()
        .into_iter()
        .filter(|r| r.source == "app")
        .map(|r| format!("{} {}", r.method, r.path))
        .collect();
    assert_eq!(
        described,
        [
            "GET /admin/api/x/docker/containers",
            "POST /admin/api/x/docker/containers/{id}/restart",
            "GET /admin/api/x/docker/logs/{id}",
            "GET /admin/api/x/docker/me",
            "GET /admin/api/x/docker/stacks",
            "POST /admin/api/x/docker/stacks",
        ]
    );
});

#[test]
#[should_panic(expected = "declared twice")]
fn a_route_declared_twice_is_refused() {
    let mut app = AppBuilder::new(test_config("sqlite::memory:"));
    AdminRoutes::new("/a")
        .get("/b", AdminAccess::authenticated(), || async { "a" })
        .nest("/", AdminRoutes::scope().get("/b", AdminAccess::authenticated(), || async { "b" }))
        .register(&mut app);
}

#[test]
fn routes_follow_a_custom_admin_path() {
    let config = test_config("sqlite::memory:").with("auth.admin_path", "/back-office").unwrap();
    let mut app = AppBuilder::new(config);
    AdminRoutes::new("/x").get("/y", AdminAccess::authenticated(), || async { "y" }).register(&mut app);
    let described: Vec<String> = app.described_routes().into_iter().map(|r| r.path).collect();
    assert!(described.contains(&"/back-office/api/x/x/y".to_owned()), "{described:?}");
}

db_test!(a_route_without_a_case_fails_the_access_table, migrate = Migrator, |db: TestDb| async move {
    let (app, _) = setup_with(build, db, &[]).await;
    let anonymous = app.client();
    let result = futures::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(assert_access(
        &app,
        &routes(),
        &[Case::new(Method::GET, "/docker/me").expect("anonymous", &anonymous, StatusCode::UNAUTHORIZED)],
    )))
    .await;
    let message = result.err().and_then(|e| e.downcast::<String>().ok()).map(|m| *m).unwrap_or_default();
    assert!(message.contains("has no case"), "{message}");
});
