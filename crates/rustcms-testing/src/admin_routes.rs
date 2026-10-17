//! An access table for [`AdminRoutes`]: each route, each actor, the status it
//! must get. The expectations are written by the test, not derived from the
//! declaration, so a route declared with the wrong access fails.
//!
//! ```rust,ignore
//! let anonymous = app.client();
//! let viewer = app.acting_as(&viewer).await;
//! let operator = app.acting_as(&operator).await;
//! assert_access(&app, &routes(), &[
//!     Case::new(Method::GET, "/docker/containers")
//!         .expect("anonymous", &anonymous, StatusCode::UNAUTHORIZED)
//!         .expect("viewer", &viewer, StatusCode::OK),
//!     Case::new(Method::POST, "/docker/containers/web/restart")
//!         .expect("viewer", &viewer, StatusCode::FORBIDDEN)
//!         .expect("operator", &operator, StatusCode::OK),
//! ]).await;
//! ```

use axum::http::{Method, StatusCode};
use rustcms_auth::{AdminRoutes, AuthState};
use serde_json::json;

use crate::{TestApp, TestClient};

/// One request (a concrete path under `{admin_path}/api/x`) and what each
/// actor must get.
pub struct Case<'c, 'a> {
    method: Method,
    path: String,
    expected: Vec<(&'static str, &'c TestClient<'a>, StatusCode)>,
}

impl<'c, 'a> Case<'c, 'a> {
    /// `method` on `path` (e.g. `/docker/containers/web`), relative to `{admin_path}/api/x`.
    pub fn new(method: Method, path: &str) -> Self {
        Self { method, path: path.to_owned(), expected: Vec::new() }
    }

    /// `actor` (named `label` in failures) must get `status`.
    pub fn expect(mut self, label: &'static str, actor: &'c TestClient<'a>, status: StatusCode) -> Self {
        self.expected.push((label, actor, status));
        self
    }
}

/// Sends every case as every actor (unsafe methods with an empty JSON body;
/// an admitted event stream is checked by its status and dropped) and checks
/// the statuses; also checks that every route of `routes` has at
/// least one case, so a new route cannot go untested.
///
/// # Panics
/// On a wrong status, a case without expectations, or a route without a case.
pub async fn assert_access(app: &TestApp, routes: &AdminRoutes, cases: &[Case<'_, '_>]) {
    let admin = app
        .state()
        .service::<AuthState>()
        .expect("AuthProvider registered")
        .config
        .admin_path
        .trim_end_matches('/')
        .to_owned();
    for (method, pattern, _) in routes.list() {
        assert!(
            cases.iter().any(|c| c.method == method && matches(&pattern, &c.path)),
            "admin route {method} {pattern} has no case in assert_access"
        );
    }
    let body = json!({});
    for case in cases {
        assert!(!case.expected.is_empty(), "{} {} has no expectation", case.method, case.path);
        let uri = format!("{admin}/api/x{}", case.path);
        let unsafe_method = !matches!(case.method, Method::GET | Method::HEAD);
        for (label, actor, status) in &case.expected {
            let response = actor.send(case.method.clone(), &uri, unsafe_method.then_some(&body)).await;
            assert_eq!(
                response.status,
                *status,
                "{} {} as {label}: expected {status}, got {} ({})",
                case.method,
                case.path,
                response.status,
                response.text()
            );
        }
    }
}

/// Whether the concrete `path` matches the route `pattern` (`{id}` is one
/// segment, `{*rest}` the remainder).
fn matches(pattern: &str, path: &str) -> bool {
    let wanted: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let given: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    for (i, segment) in wanted.iter().enumerate() {
        if segment.starts_with("{*") {
            return given.len() > i;
        }
        match given.get(i) {
            Some(value) if segment.starts_with('{') || value == segment => {}
            _ => return false,
        }
    }
    wanted.len() == given.len()
}

#[cfg(test)]
mod tests {
    use super::matches;

    #[test]
    fn concrete_paths_match_route_patterns() {
        assert!(matches("/docker", "/docker"));
        assert!(matches("/docker/{id}/restart", "/docker/web/restart"));
        assert!(matches("/files/{*rest}", "/files/a/b"));
        assert!(!matches("/docker/{id}", "/docker"));
        assert!(!matches("/docker/{id}", "/docker/web/extra"));
        assert!(!matches("/files/{*rest}", "/files"));
    }
}
