use std::net::SocketAddr;
use std::path::Path;

use axum::body::{Body, Bytes};
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use figment::providers::Serialized;
use http_body_util::BodyExt;
use rustcms_core::{App, AppBuilder, AppKey, AppState, Config};
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::db::TestDb;

/// Configuration for tests: the app's `config/` files, the `testing`
/// environment, a random `APP_KEY` and the given database URL (these three
/// override any environment variable).
pub fn test_config(database_url: &str) -> Config {
    let overrides = json!({
        "app": { "env": "testing", "key": AppKey::generate().encode() },
        "database": { "url": database_url },
    });
    let figment = Config::figment(Path::new("config")).expect("config files").merge(Serialized::defaults(overrides));
    Config::from_figment(figment).expect("test configuration")
}

/// The full application router, driven in-process.
pub struct TestApp {
    app: App,
    db: TestDb,
}

impl TestApp {
    /// Builds the app with [`test_config`] on `db`.
    pub async fn new(build: fn(Config) -> AppBuilder, db: TestDb) -> Self {
        Self::with_config(build, test_config(db.url()), db).await
    }

    /// Builds the app with an explicit configuration.
    pub async fn with_config(build: fn(Config) -> AppBuilder, config: Config, db: TestDb) -> Self {
        let app = build(config).build_with_db(db.conn().clone()).await.expect("build app");
        Self { app, db }
    }

    /// Shared state.
    pub fn state(&self) -> &AppState {
        self.app.state()
    }

    /// Test database.
    pub fn db(&self) -> &TestDb {
        &self.db
    }

    /// Sends a request from `127.0.0.1`.
    pub async fn request(&self, request: Request<Body>) -> TestResponse {
        self.request_from(SocketAddr::from(([127, 0, 0, 1], 40000)), request).await
    }

    /// Sends a request as if the TCP peer were `peer`.
    pub async fn request_from(&self, peer: SocketAddr, mut request: Request<Body>) -> TestResponse {
        request.extensions_mut().insert(ConnectInfo(peer));
        let response = self.app.router().oneshot(request).await.expect("infallible router");
        let (parts, body) = response.into_parts();
        let body = body.collect().await.expect("response body").to_bytes();
        TestResponse { status: parts.status, headers: parts.headers, body }
    }

    /// `GET uri`.
    pub async fn get(&self, uri: &str) -> TestResponse {
        self.request(Request::get(uri).body(Body::empty()).expect("request")).await
    }

    /// Sends `body` as JSON.
    pub async fn json(&self, method: Method, uri: &str, body: &Value) -> TestResponse {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .expect("request");
        self.request(request).await
    }

    /// `POST uri` with a JSON body.
    pub async fn post_json(&self, uri: &str, body: &Value) -> TestResponse {
        self.json(Method::POST, uri, body).await
    }
}

/// A buffered response.
#[derive(Debug)]
pub struct TestResponse {
    /// Status code.
    pub status: StatusCode,
    /// Headers.
    pub headers: HeaderMap,
    /// Body bytes.
    pub body: Bytes,
}

impl TestResponse {
    /// Body parsed as JSON.
    ///
    /// # Panics
    /// When the body is not JSON.
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body)
            .unwrap_or_else(|e| panic!("body is not JSON ({e}): {}", String::from_utf8_lossy(&self.body)))
    }

    /// Asserts the status code.
    #[track_caller]
    pub fn assert_status(&self, expected: StatusCode) -> &Self {
        assert_eq!(self.status, expected, "body: {}", String::from_utf8_lossy(&self.body));
        self
    }

    /// Asserts the value at a JSON pointer (`/data/0/name`).
    #[track_caller]
    pub fn assert_json(&self, pointer: &str, expected: Value) -> &Self {
        let body = self.json();
        assert_eq!(body.pointer(pointer), Some(&expected), "at `{pointer}` in {body}");
        self
    }

    /// Asserts a 422 with a message for `field`.
    #[track_caller]
    pub fn assert_validation_error(&self, field: &str) -> &Self {
        self.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
        let body = self.json();
        assert!(body["errors"][field].as_array().is_some_and(|m| !m.is_empty()), "no error for `{field}` in {body}");
        self
    }
}
