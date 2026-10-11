//! The HTTP stack of `rustcms-core`, driven through `TestApp`.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::{get, post};
use rustcms_core::{AppBuilder, AppState, ClientIp, Config, Inject, Json, ServiceProvider};
use rustcms_testing::{TestApp, TestDb, test_config};
use serde::Deserialize;
use serde_json::{Value, json};

struct Greeter(&'static str);

struct Booted(AtomicBool);

struct GreeterProvider;

#[async_trait]
impl ServiceProvider for GreeterProvider {
    fn register(&self, app: &mut AppBuilder) {
        app.bind(Greeter("hello")).bind(Booted(AtomicBool::new(false)));
        app.routes(|r| r.route("/greet", get(|Inject(g): Inject<Greeter>| async move { g.0 })));
    }

    async fn boot(&self, state: &AppState) -> anyhow::Result<()> {
        state.service::<Booted>().expect("bound in register").0.store(true, Ordering::SeqCst);
        Ok(())
    }
}

#[derive(Deserialize)]
struct Payload {
    #[allow(dead_code)]
    name: String,
}

fn build(config: Config) -> AppBuilder {
    let mut app = AppBuilder::new(config);
    app.provider(GreeterProvider);
    app.routes(|r| {
        r.route("/echo", post(|Json(v): Json<Value>| async move { Json(v) }))
            .route("/typed", post(|Json(_): Json<Payload>| async { "ok" }))
            .route(
                "/raw",
                post(|body: Body| async move {
                    match axum::body::to_bytes(body, usize::MAX).await {
                        Ok(bytes) => format!("read {}", bytes.len()),
                        Err(_) => "stopped at limit".to_owned(),
                    }
                }),
            )
            .route("/bytes", post(|bytes: axum::body::Bytes| async move { bytes.len().to_string() }))
            .route("/ip", get(|ClientIp(ip): ClientIp| async move { ip.to_string() }))
            .route("/missing-service", get(|Inject(_): Inject<Arc<String>>| async { "unreachable" }))
            .route(
                "/slow",
                get(|| async {
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    "late"
                }),
            )
    });
    app
}

async fn app_with(configure: impl FnOnce(Config) -> Config) -> TestApp {
    let db = TestDb::sqlite().await;
    let config = configure(test_config(db.url()));
    TestApp::with_config(build, config, db).await
}

fn text(response: &rustcms_testing::TestResponse) -> String {
    String::from_utf8_lossy(&response.body).into_owned()
}

#[tokio::test]
async fn json_errors_follow_the_contract() {
    let app = app_with(|c| c.with("http.body_limit", 64).unwrap()).await;

    app.post_json("/echo", &json!({"a": 1})).await.assert_status(StatusCode::OK).assert_json("/a", json!(1));

    let big = json!({ "data": "x".repeat(200) });
    app.post_json("/echo", &big)
        .await
        .assert_status(StatusCode::PAYLOAD_TOO_LARGE)
        .assert_json("/code", json!("payload_too_large"));

    let malformed =
        Request::post("/echo").header("content-type", "application/json").body(Body::from("{oops")).unwrap();
    app.request(malformed).await.assert_status(StatusCode::BAD_REQUEST).assert_json("/code", json!("bad_request"));

    let wrong_shape = app.post_json("/typed", &json!({"title": "x"})).await;
    wrong_shape.assert_status(StatusCode::UNPROCESSABLE_ENTITY).assert_json("/code", json!("validation_failed"));
}

#[tokio::test]
async fn body_limit_applies_to_every_handler() {
    let app = app_with(|c| c.with("http.body_limit", 64).unwrap()).await;
    let raw = |declared: bool, size: usize| {
        let mut builder = Request::post("/raw");
        if declared {
            builder = builder.header("content-length", size);
        }
        builder.body(Body::from(vec![b'x'; size])).unwrap()
    };

    assert_eq!(text(&app.request(raw(true, 10)).await), "read 10");

    // Declared too large: rejected before the handler runs.
    app.request(raw(true, 1_000))
        .await
        .assert_status(StatusCode::PAYLOAD_TOO_LARGE)
        .assert_json("/code", json!("payload_too_large"));

    // No Content-Length (chunked): a raw handler cannot read past the limit.
    assert_eq!(text(&app.request(raw(false, 1_000)).await), "stopped at limit");

    // axum's own 413 is rendered in the JSON error format.
    let bytes = Request::post("/bytes").body(Body::from(vec![b'x'; 1_000])).unwrap();
    app.request(bytes)
        .await
        .assert_status(StatusCode::PAYLOAD_TOO_LARGE)
        .assert_json("/code", json!("payload_too_large"));
}

#[tokio::test]
async fn request_id_is_generated_or_propagated() {
    let app = app_with(|c| c).await;
    let generated = app.get("/health").await;
    assert_eq!(generated.headers["x-request-id"].len(), 36, "uuid");

    let request = Request::get("/health").header("x-request-id", "abc-123").body(Body::empty()).unwrap();
    assert_eq!(app.request(request).await.headers["x-request-id"], "abc-123");
}

#[tokio::test]
async fn client_ip_trusts_only_configured_proxies() {
    let app = app_with(|c| c.with("http.trusted_proxies", ["10.0.0.0/8"]).unwrap()).await;
    let via = |peer: &str, xff: &str| {
        let request = Request::get("/ip").header("x-forwarded-for", xff).body(Body::empty()).unwrap();
        (peer.parse::<SocketAddr>().unwrap(), request)
    };

    let (peer, request) = via("10.0.0.2:5000", "6.6.6.6, 198.51.100.7");
    assert_eq!(text(&app.request_from(peer, request).await), "198.51.100.7", "spoofed left-most hop ignored");

    let (peer, request) = via("203.0.113.9:5000", "198.51.100.7");
    assert_eq!(text(&app.request_from(peer, request).await), "203.0.113.9", "untrusted peer cannot set its IP");
}

#[tokio::test]
async fn providers_bind_services_and_boot() {
    let app = app_with(|c| c).await;
    assert!(app.state().service::<Booted>().unwrap().0.load(Ordering::SeqCst));
    assert_eq!(text(&app.get("/greet").await), "hello");

    let missing = app.get("/missing-service").await;
    missing.assert_status(StatusCode::INTERNAL_SERVER_ERROR).assert_json("/message", json!("Server error."));
    assert!(!text(&missing).contains("Arc"), "internal details are not leaked");
}

#[tokio::test]
async fn slow_requests_time_out() {
    let app = app_with(|c| c.with("http.timeout_secs", 1).unwrap()).await;
    app.get("/slow").await.assert_status(StatusCode::REQUEST_TIMEOUT);
}

#[tokio::test]
async fn health_reports_database_outage() {
    let app = app_with(|c| c).await;
    app.get("/health").await.assert_status(StatusCode::OK);
    app.state().db().clone().close().await.unwrap();
    app.get("/health")
        .await
        .assert_status(StatusCode::SERVICE_UNAVAILABLE)
        .assert_json("/database", json!("unavailable"));
}
