//! `ApiRoutes`: every route states its access, checked before the handler reads
//! anything; routes are mounted and described from one list. All databases.

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::body::Body;
use axum::extract::Path;
use axum::http::{Method, Request, StatusCode};
use rustcms_auth::{API_GUARD, Access, ApiRoutes, Auth, Throttle};
use rustcms_core::{AppBuilder, Config, Json, Pagination, Validate, Validated, ValidationErrors};
use rustcms_migration::Migrator;
use rustcms_testing::{TestDb, db_test, test_config};
use serde::Deserialize;
use serde_json::json;
use support::setup_with;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewNote {
    title: String,
}

impl Validate for NewNote {
    fn validate(&mut self) -> Result<(), ValidationErrors> {
        self.title = self.title.trim().to_owned();
        if self.title.is_empty() { Err(ValidationErrors::field("title", "is required")) } else { Ok(()) }
    }
}

fn notes() -> ApiRoutes {
    ApiRoutes::scope()
        .get("/", Access::can("notes.view_any"), |page: Pagination| async move { Json(json!({"page": page.page()})) })
        .post("/", Access::can("notes.create"), |_: Throttle<4>, Validated(note): Validated<NewNote>| async move {
            (StatusCode::CREATED, Json(json!({"title": note.title})))
        })
        .get("/{id}", Access::can("notes.view"), |Path(id): Path<i64>| async move { Json(json!({"id": id})) })
}

fn routes() -> ApiRoutes {
    ApiRoutes::new("/api/v1")
        .get("/ping", Access::public(), || async { "pong" })
        .get("/me", Access::authenticated(), |auth: Auth| async move { Json(json!({"id": auth.0.user.id})) })
        .nest("/notes", notes())
        // A child with its own prefix.
        .nest("/x", ApiRoutes::new("/v2").get("/items", Access::public(), || async { "items" }))
        // Public and protected methods on one path, in both orders.
        .get("/open", Access::public(), || async { "read" })
        .post("/open", Access::can("notes.create"), || async { "written" })
        .post("/closed", Access::can("notes.create"), || async { "written" })
        .get("/closed", Access::public(), || async { "read" })
}

fn build(config: Config) -> AppBuilder {
    let mut app = AppBuilder::new(config);
    let api = routes();
    // An explicit group wins over the one inferred from the name.
    app.provider(support::provider().permission(API_GUARD, "notes.view", "reading").api_permissions(&api));
    api.register(&mut app);
    app
}

db_test!(access_is_checked_before_the_request_is_read, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup_with(build, db, &[(API_GUARD, "note_reader", &["notes.view_any"])]).await;
    // Anyone.
    app.client().get("/api/v1/ping").await.assert_status(StatusCode::OK);
    // A token is needed; a session of the admin panel is not one.
    app.client().get("/api/v1/me").await.assert_status(StatusCode::UNAUTHORIZED);
    app.client().get("/api/v1/notes").await.assert_status(StatusCode::UNAUTHORIZED);
    app.acting_as(&seed.admin).await.get("/api/v1/notes").await.assert_status(StatusCode::UNAUTHORIZED);
    // Any token for `authenticated`; the permission for `can`, whatever the input.
    let member = app.with_token(&seed.member, &["*"]).await;
    member.get("/api/v1/me").await.assert_status(StatusCode::OK).assert_json("/id", json!(seed.member.id));
    member.get("/api/v1/notes").await.assert_status(StatusCode::FORBIDDEN);
    member.get("/api/v1/notes?page=zero").await.assert_status(StatusCode::FORBIDDEN);
    member.get("/api/v1/notes/not-a-number").await.assert_status(StatusCode::FORBIDDEN);
    let reader = app.with_token(&seed.reader, &["*"]).await;
    reader.get("/api/v1/notes?page=2").await.assert_status(StatusCode::OK).assert_json("/page", json!(2));
    reader.get("/api/v1/notes?page=0").await.assert_validation_error("page");
    reader.post_json("/api/v1/notes", &json!({"nonsense": true})).await.assert_status(StatusCode::FORBIDDEN);
    reader.get("/api/v1/notes/7").await.assert_status(StatusCode::FORBIDDEN);
    // The body of a refused request is never read.
    let polled = Arc::new(AtomicBool::new(false));
    let seen = polled.clone();
    let body = Body::from_stream(futures::stream::poll_fn(move |_| {
        seen.store(true, Ordering::SeqCst);
        std::task::Poll::Ready(None::<Result<Vec<u8>, std::io::Error>>)
    }));
    let request = Request::post("/api/v1/notes").header("content-type", "application/json").body(body).unwrap();
    reader.request(request).await.assert_status(StatusCode::FORBIDDEN);
    assert!(!polled.load(Ordering::SeqCst), "the body was read before access was checked");
    // A token's abilities narrow its user's permissions.
    let narrow = app.with_token(&seed.admin, &["notes.view"]).await;
    narrow.get("/api/v1/notes").await.assert_status(StatusCode::FORBIDDEN);
    narrow.get("/api/v1/notes/7").await.assert_status(StatusCode::OK).assert_json("/id", json!(7));
});

db_test!(methods_paths_and_nesting, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup_with(build, db, &[(API_GUARD, "note_reader", &["notes.view_any"])]).await;
    let anyone = app.client();
    let reader = app.with_token(&seed.reader, &["*"]).await;
    // HEAD follows GET's access; an undeclared method is 405.
    anyone.send(Method::HEAD, "/api/v1/notes", None).await.assert_status(StatusCode::UNAUTHORIZED);
    reader.send(Method::HEAD, "/api/v1/notes", None).await.assert_status(StatusCode::OK);
    reader.send(Method::PUT, "/api/v1/notes", Some(&json!({}))).await.assert_status(StatusCode::METHOD_NOT_ALLOWED);
    // Each method keeps its own access, whatever the declaration order.
    for path in ["/api/v1/open", "/api/v1/closed"] {
        anyone.get(path).await.assert_status(StatusCode::OK);
        anyone.post_json(path, &json!({})).await.assert_status(StatusCode::UNAUTHORIZED);
        reader.post_json(path, &json!({})).await.assert_status(StatusCode::FORBIDDEN);
    }
    // A nested child keeps its own prefix once.
    anyone.get("/api/v1/x/v2/items").await.assert_status(StatusCode::OK);
});

db_test!(validated_bodies_are_throttled_parsed_then_checked, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup_with(build, db, &[]).await;
    let admin = app.with_token(&seed.admin, &["*"]).await;
    let created = admin.post_json("/api/v1/notes", &json!({"title": "  Hello  "})).await;
    created.assert_status(StatusCode::CREATED).assert_json("/title", json!("Hello"));
    // `validate`: 422 with field errors, nothing else.
    let invalid = admin.post_json("/api/v1/notes", &json!({"title": "   "})).await;
    invalid.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        invalid.json(),
        json!({"message": "The given data was invalid.", "code": "validation_failed",
               "errors": {"title": ["is required"]}, "formErrors": [],
               "issues": {"title": [{"code": "required", "params": {}}]}, "formIssues": []})
    );
    // Not `T`: 422 with one form error and no field errors.
    let shape = admin.post_json("/api/v1/notes", &json!({"title": "x", "extra": 1})).await;
    shape.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    let body = shape.json();
    assert_eq!((body["code"].clone(), body["errors"].clone()), (json!("validation_failed"), json!({})), "{body}");
    assert_eq!(body["formErrors"].as_array().map(Vec::len), Some(1), "{body}");
    // Not JSON: 400, without validation fields.
    let malformed = Request::post("/api/v1/notes").header("content-type", "application/json").body(Body::from("{"));
    let malformed = admin.request(malformed.unwrap()).await;
    malformed.assert_status(StatusCode::BAD_REQUEST);
    let body = malformed.json();
    assert_eq!(body["code"], json!("bad_request"), "{body}");
    assert!(body.get("errors").is_none() && body.get("formErrors").is_none(), "{body}");
    // Four attempts a minute per user and route, valid or not, counted before the body is read.
    let limited = app.with_token(&seed.admin, &["*"]).await.post_json("/api/v1/notes", &json!({"title": "x"})).await;
    limited.assert_status(StatusCode::TOO_MANY_REQUESTS);
    assert!(limited.headers.contains_key("retry-after"));
});

db_test!(routes_are_described_and_their_permissions_declared, migrate = Migrator, |db: TestDb| async move {
    let (app, _) = setup_with(build, db, &[]).await;
    let declared: Vec<(String, String)> = support::auth(&app)
        .permissions
        .all()
        .into_iter()
        .filter(|((guard, name), _)| guard == API_GUARD && name.starts_with("notes."))
        .map(|((_, name), group)| (name, group.unwrap_or_default()))
        .collect();
    assert_eq!(
        declared,
        [("notes.create", "notes"), ("notes.view", "reading"), ("notes.view_any", "notes")]
            .map(|(n, g)| (n.to_owned(), g.to_owned()))
    );
    // API permissions are declared on the API guard only.
    assert!(
        !support::auth(&app)
            .permissions
            .all()
            .keys()
            .any(|(guard, name)| guard != API_GUARD && name.starts_with("notes."))
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
            "GET /api/v1/closed",
            "POST /api/v1/closed",
            "GET /api/v1/me",
            "GET /api/v1/notes",
            "POST /api/v1/notes",
            "GET /api/v1/notes/{id}",
            "GET /api/v1/open",
            "POST /api/v1/open",
            "GET /api/v1/ping",
            "GET /api/v1/x/v2/items",
        ]
    );
});

#[test]
#[should_panic(expected = "declared twice")]
fn a_route_declared_twice_is_refused() {
    let mut app = AppBuilder::new(test_config("sqlite::memory:"));
    ApiRoutes::new("/api")
        .get("/a", Access::public(), || async { "a" })
        .nest("/", ApiRoutes::scope().get("/a", Access::public(), || async { "b" }))
        .register(&mut app);
}

#[test]
#[should_panic(expected = "under the admin path")]
fn api_routes_stay_out_of_the_admin_path() {
    let mut app = AppBuilder::new(test_config("sqlite::memory:"));
    ApiRoutes::new("/admin").get("/report", Access::public(), || async { "x" }).register(&mut app);
}

#[test]
#[should_panic(expected = "under the admin path")]
fn a_capture_that_can_reach_the_admin_path_is_refused() {
    let mut app = AppBuilder::new(test_config("sqlite::memory:"));
    ApiRoutes::new("").get("/{section}/report", Access::public(), || async { "x" }).register(&mut app);
}

#[test]
#[should_panic(expected = "under the admin path `/back-office`")]
fn a_custom_admin_path_is_respected() {
    let config = test_config("sqlite::memory:").with("auth.admin_path", "/back-office").unwrap();
    let mut app = AppBuilder::new(config);
    ApiRoutes::new("/back-office").get("/x", Access::public(), || async { "x" }).register(&mut app);
}

#[test]
#[should_panic(expected = "API paths start with `/`")]
fn paths_start_with_a_slash() {
    let _ = ApiRoutes::new("/api").get("a", Access::public(), || async { "a" });
}

#[test]
#[should_panic(expected = "invalid permission name")]
fn permission_names_are_checked() {
    let _ = Access::can("posts view");
}
