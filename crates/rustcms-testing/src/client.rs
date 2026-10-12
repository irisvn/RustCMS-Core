use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Mutex;

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Method, Request, header};
use rustcms_auth::{AuthState, CSRF_HEADER};
use rustcms_core::models::user;
use serde_json::Value;

use crate::app::{TestApp, TestResponse};

/// A browser- or API-like client: keeps cookies, sends `Origin` and the CSRF
/// token, or a bearer token. Every request goes through the real guards.
pub struct TestClient<'a> {
    app: &'a TestApp,
    cookies: Mutex<BTreeMap<String, String>>,
    csrf: Mutex<Option<String>>,
    bearer: Option<String>,
    origin: Option<String>,
    peer: SocketAddr,
}

impl TestApp {
    /// An anonymous client sending `Origin: <app.url>`.
    pub fn client(&self) -> TestClient<'_> {
        let url = &self.state().config().app.url;
        let origin = url.split('/').take(3).collect::<Vec<_>>().join("/");
        TestClient {
            app: self,
            cookies: Mutex::default(),
            csrf: Mutex::default(),
            bearer: None,
            origin: Some(origin),
            peer: SocketAddr::from(([127, 0, 0, 1], 40000)),
        }
    }

    /// A client signed in to the admin guard as `user`, without a password.
    ///
    /// Creates a real session row; requests still pass the session guard, CSRF
    /// and `auth_version` checks. Use for authorization tests.
    pub async fn acting_as(&self, user: &user::Model) -> TestClient<'_> {
        let auth = self.state().service::<AuthState>().expect("AuthProvider registered");
        let (token, csrf) =
            rustcms_auth::session::create_for_user(&auth, self.state().db(), user.id, user.auth_version)
                .await
                .expect("create session");
        let client = self.client();
        client.cookies.lock().unwrap().insert(auth.config.cookie_name.clone(), token);
        *client.csrf.lock().unwrap() = Some(csrf);
        client
    }

    /// Makes `user` a super admin of every guard (through `RbacService`).
    ///
    /// # Panics
    /// Without `AuthProvider` or the framework migrations.
    pub async fn make_super_admin(&self, user: &user::Model) {
        use rustcms_auth::{Actor, RbacService};
        let auth = self.state().service::<AuthState>().expect("AuthProvider registered");
        let db = self.state().db();
        let rbac = RbacService::new(db, &auth.resolver);
        for guard in [rustcms_auth::ADMIN_GUARD, rustcms_auth::API_GUARD] {
            let role =
                rustcms_auth::entities::role::super_admin_id(db, guard).await.unwrap().expect("super_admin role");
            rbac.assign_role(Actor::System, user.id, role).await.expect("assign super_admin");
        }
    }

    /// A client signed in through the real HTTP flow (`csrf` then `login`).
    ///
    /// # Panics
    /// When sign-in fails.
    pub async fn login_as(&self, email: &str, password: &str) -> TestClient<'_> {
        let client = self.client();
        client.fetch_csrf().await;
        let body = serde_json::json!({ "email": email, "password": password });
        let response = client.post_json(&self.admin_api("auth/login"), &body).await;
        assert!(response.status.is_success(), "login failed: {}", String::from_utf8_lossy(&response.body));
        client.fetch_csrf().await;
        client
    }

    /// An API client with a fresh token of `user` limited to `abilities` (`["*"]` = all).
    pub async fn with_token(&self, user: &user::Model, abilities: &[&str]) -> TestClient<'_> {
        let abilities: Vec<String> = abilities.iter().map(|a| (*a).to_owned()).collect();
        let issued = rustcms_auth::tokens::issue(self.state().db(), user.id, "test", &abilities, None)
            .await
            .expect("issue token");
        self.client().bearer(&issued.plain_text)
    }

    /// `<admin_path>/api/<path>`.
    pub fn admin_api(&self, path: &str) -> String {
        let auth = self.state().service::<AuthState>().expect("AuthProvider registered");
        format!("{}/api/{path}", auth.config.admin_path.trim_end_matches('/'))
    }
}

impl TestClient<'_> {
    /// Sends `Authorization: Bearer <token>`.
    pub fn bearer(mut self, token: &str) -> Self {
        self.bearer = Some(token.to_owned());
        self
    }

    /// Sends this `Origin` (`None`: no Origin header).
    pub fn origin(mut self, origin: Option<&str>) -> Self {
        self.origin = origin.map(str::to_owned);
        self
    }

    /// Simulates the TCP peer.
    pub fn from(mut self, peer: SocketAddr) -> Self {
        self.peer = peer;
        self
    }

    /// Overrides the CSRF token sent (`None`: no header).
    pub fn set_csrf(&self, token: Option<&str>) {
        *self.csrf.lock().unwrap() = token.map(str::to_owned);
    }

    /// Current value of a cookie.
    pub fn cookie(&self, name: &str) -> Option<String> {
        self.cookies.lock().unwrap().get(name).cloned()
    }

    /// `GET <admin>/api/auth/csrf` and remember the token.
    pub async fn fetch_csrf(&self) -> String {
        let response = self.get(&self.app.admin_api("auth/csrf")).await;
        let token = response.json()["csrfToken"].as_str().expect("csrfToken").to_owned();
        self.set_csrf(Some(&token));
        token
    }

    /// `GET uri`.
    pub async fn get(&self, uri: &str) -> TestResponse {
        self.send(Method::GET, uri, None).await
    }

    /// `POST uri` with JSON.
    pub async fn post_json(&self, uri: &str, body: &Value) -> TestResponse {
        self.send(Method::POST, uri, Some(body)).await
    }

    /// Any method, optional JSON body.
    pub async fn send(&self, method: Method, uri: &str, body: Option<&Value>) -> TestResponse {
        let mut builder = Request::builder().method(method).uri(uri);
        if body.is_some() {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
        }
        let request = builder.body(body.map_or_else(Body::empty, |b| Body::from(b.to_string()))).expect("request");
        self.request(request).await
    }

    /// Sends `request` with this client's cookies and credentials.
    pub async fn request(&self, mut request: Request<Body>) -> TestResponse {
        let headers = request.headers_mut();
        let cookies =
            self.cookies.lock().unwrap().iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("; ");
        if !cookies.is_empty() {
            headers.insert(header::COOKIE, HeaderValue::from_str(&cookies).expect("cookie header"));
        }
        if let Some(origin) = &self.origin {
            headers.insert(header::ORIGIN, HeaderValue::from_str(origin).expect("origin"));
        }
        if let Some(csrf) = self.csrf.lock().unwrap().as_deref() {
            headers.insert(HeaderName::from_static(CSRF_HEADER), HeaderValue::from_str(csrf).expect("csrf"));
        }
        if let Some(token) = &self.bearer {
            headers.insert(header::AUTHORIZATION, HeaderValue::from_str(&format!("Bearer {token}")).expect("bearer"));
        }
        let response = self.app.request_from(self.peer, request).await;
        self.store_cookies(&response);
        response
    }

    fn store_cookies(&self, response: &TestResponse) {
        let mut jar = self.cookies.lock().unwrap();
        for value in response.headers.get_all(header::SET_COOKIE).iter().filter_map(|v| v.to_str().ok()) {
            let mut parts = value.split(';').map(str::trim);
            let Some((name, val)) = parts.next().and_then(|p| p.split_once('=')) else { continue };
            let expired = parts.any(|p| p.eq_ignore_ascii_case("Max-Age=0"));
            if expired || val.is_empty() {
                jar.remove(name);
            } else {
                jar.insert(name.to_owned(), val.to_owned());
            }
        }
    }
}
