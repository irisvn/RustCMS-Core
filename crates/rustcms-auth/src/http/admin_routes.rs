//! JSON routes for the admin panel's own pages, on its session (ADR 0026).
//!
//! ```rust,ignore
//! // src/routes/admin.rs
//! pub fn routes() -> AdminRoutes {
//!     AdminRoutes::new("/docker")
//!         .get("/containers", AdminAccess::can("docker.view"), controllers::docker::index)
//!         .post("/containers/{id}/restart", AdminAccess::can("docker.manage"), controllers::docker::restart)
//! }
//! // src/bootstrap/mod.rs
//! let admin = crate::routes::admin::routes();
//! app.provider(AuthProvider::new().admin_permissions(&admin));
//! admin.register(&mut app);
//! ```
//!
//! The routes live under `{admin_path}/api/x`, a namespace nothing built in
//! uses: `AdminRoutes::new("/docker")` serves `/admin/api/x/docker/containers`.
//! The admin guard applies as everywhere under `admin_path`: a session cookie,
//! and for unsafe methods an allowed `Origin` and the `X-CSRF-Token` header.
//! API tokens and JWTs are not admin credentials (401).
//!
//! Access is checked before the handler runs, so before any of its extractors
//! reads the request: without a session 401, without the permission 403,
//! whatever the body, path or query. There is no public access: every route
//! needs a signed-in admin. `HEAD` follows the access of `GET`; other
//! undeclared methods are 405.

use std::collections::{BTreeMap, BTreeSet};

use axum::extract::Request;
use axum::handler::Handler;
use axum::http::Method;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{MethodRouter, delete, get, patch, post, put};
use rustcms_core::{AppBuilder, AppError, AppState};

use crate::config::{AuthConfig, normalize_admin_path};
use crate::guard::Authenticated;
use crate::http::api::join;
use crate::state::ADMIN_GUARD;

/// The namespace of application routes under `{admin_path}/api`.
pub const NAMESPACE: &str = "/x";

/// What an admin route requires: a signed-in admin, possibly with a
/// permission. Every route states one; there is no public access.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminAccess {
    /// Any signed-in admin session.
    Authenticated,
    /// A signed-in admin whose user holds this permission in the `admin` guard.
    Can(String),
}

impl AdminAccess {
    /// Any signed-in admin session.
    pub fn authenticated() -> Self {
        Self::Authenticated
    }

    /// A signed-in admin allowed `permission` (e.g. `docker.manage`).
    ///
    /// # Panics
    /// On an empty name, whitespace, or a leading or trailing `.`.
    pub fn can(permission: impl Into<String>) -> Self {
        let permission = permission.into();
        assert!(
            !permission.is_empty()
                && !permission.chars().any(char::is_whitespace)
                && !permission.starts_with('.')
                && !permission.ends_with('.'),
            "invalid permission name {permission:?} (e.g. `docker.manage`)"
        );
        Self::Can(permission)
    }

    fn check(&self, caller: Option<&Authenticated>) -> Result<(), AppError> {
        let Some(caller) = caller.filter(|c| c.principal.guard == ADMIN_GUARD) else {
            return Err(AppError::Unauthenticated);
        };
        if self.allows(&caller.principal) { Ok(()) } else { Err(AppError::Forbidden) }
    }

    /// Whether an admin `principal` passes (rechecks of open streams).
    pub(crate) fn allows(&self, principal: &rustcms_core::Principal) -> bool {
        match self {
            Self::Authenticated => principal.guard == ADMIN_GUARD,
            Self::Can(permission) => principal.guard == ADMIN_GUARD && principal.can(permission),
        }
    }
}

struct Route {
    method: Method,
    /// Relative to the owning [`AdminRoutes`]' prefix.
    path: String,
    access: AdminAccess,
    router: MethodRouter<AppState>,
}

/// A set of admin routes under a prefix. See the [module](self) documentation.
#[must_use = "register the routes with `AdminRoutes::register`"]
pub struct AdminRoutes {
    prefix: String,
    routes: Vec<Route>,
}

macro_rules! methods {
    ($($name:ident => $method:ident, $route:ident;)*) => {$(
        #[doc = concat!("A `", stringify!($method), "` route at `path` (relative to the prefix).")]
        pub fn $name<H, T>(self, path: &str, access: AdminAccess, handler: H) -> Self
        where
            H: Handler<T, AppState>,
            T: 'static,
        {
            self.add(Method::$method, path, access, $route(handler))
        }
    )*};
}

impl AdminRoutes {
    /// Routes under `{admin_path}/api/x{prefix}` (e.g. `/docker`).
    ///
    /// # Panics
    /// When `prefix` is not empty and does not start with `/`.
    pub fn new(prefix: &str) -> Self {
        assert!(prefix.is_empty() || prefix.starts_with('/'), "admin route prefixes start with `/`: {prefix:?}");
        Self { prefix: prefix.trim_end_matches('/').to_owned(), routes: Vec::new() }
    }

    /// Routes without a prefix of their own, to [nest](Self::nest) elsewhere.
    pub fn scope() -> Self {
        Self::new("")
    }

    methods! {
        get => GET, get;
        post => POST, post;
        put => PUT, put;
        patch => PATCH, patch;
        delete => DELETE, delete;
    }

    /// A server-sent events route at `path` (GET only; `HEAD` is 405). Its
    /// handler takes a [`LiveSession`](crate::LiveSession) and returns an
    /// [`EventStream`](crate::EventStream). Admission needs no CSRF token but
    /// the panel's own origin; the stream is rechecked while open (ADR 0027).
    pub fn events<H, T>(mut self, path: &str, access: AdminAccess, handler: H) -> Self
    where
        H: Handler<T, AppState>,
        T: 'static,
    {
        assert!(path.starts_with('/'), "admin route paths start with `/`: {path:?}");
        let checked = access.clone();
        let router = get(handler).route_layer(middleware::from_fn(move |req: Request, next: Next| {
            let access = checked.clone();
            async move { admitted_stream(&access, req, next).await }
        }));
        self.routes.push(Route { method: Method::GET, path: join("", path), access, router });
        self
    }

    /// Adds `routes` (with their own prefix, if any) under `prefix`, relative
    /// to this one. Each keeps its own access.
    pub fn nest(mut self, prefix: &str, routes: AdminRoutes) -> Self {
        assert!(prefix.starts_with('/'), "nested prefixes start with `/`: {prefix:?}");
        let base = join(prefix, &routes.prefix);
        for route in routes.routes {
            self.routes.push(Route { path: join(&base, &route.path), ..route });
        }
        self
    }

    fn add(mut self, method: Method, path: &str, access: AdminAccess, router: MethodRouter<AppState>) -> Self {
        assert!(path.starts_with('/'), "admin route paths start with `/`: {path:?}");
        let checked = access.clone();
        let router = router.route_layer(middleware::from_fn(move |req: Request, next: Next| {
            let access = checked.clone();
            async move { guarded(&access, req, next).await }
        }));
        self.routes.push(Route { method, path: join("", path), access, router });
        self
    }

    /// Every route as `(method, path under {admin_path}/api/x, access)`, in
    /// declaration order (`/docker/containers`).
    pub fn list(&self) -> Vec<(Method, String, &AdminAccess)> {
        self.routes.iter().map(|r| (r.method.clone(), join(&self.prefix, &r.path), &r.access)).collect()
    }

    /// The permissions the routes require, without duplicates.
    pub fn permissions(&self) -> BTreeSet<&str> {
        self.routes
            .iter()
            .filter_map(|r| match &r.access {
                AdminAccess::Can(permission) => Some(permission.as_str()),
                AdminAccess::Authenticated => None,
            })
            .collect()
    }

    /// Mounts the routes under `{admin_path}/api/x` and describes them for
    /// `route:list` (source `app`), both from this list.
    ///
    /// # Panics
    /// When two routes have the same method and path, or `[auth]` is invalid.
    pub fn register(self, app: &mut AppBuilder) {
        let config: AuthConfig =
            app.config().section("auth").unwrap_or_else(|e| panic!("invalid [auth] configuration: {e}"));
        let admin = normalize_admin_path(&config.admin_path).unwrap_or_else(|e| panic!("{e}"));
        let base = format!("{admin}/api{NAMESPACE}");
        let mut seen = BTreeSet::new();
        let listed: Vec<(Method, String)> =
            self.list().into_iter().map(|(method, path, _)| (method, join(&base, &path))).collect();
        for (method, path) in &listed {
            assert!(seen.insert((path.clone(), method.to_string())), "admin route {method} {path} is declared twice");
        }
        app.describe_routes("app", listed);
        let mut by_path: BTreeMap<String, MethodRouter<AppState>> = BTreeMap::new();
        for route in self.routes {
            let path = join(&base, &join(&self.prefix, &route.path));
            let merged = match by_path.remove(&path) {
                Some(existing) => existing.merge(route.router),
                None => route.router,
            };
            by_path.insert(path, merged);
        }
        app.routes(move |mut router| {
            for (path, methods) in by_path {
                router = router.route(&path, methods);
            }
            router
        });
    }
}

/// A stream: GET only, the route's access, then its admission, all before any
/// extractor of the handler.
async fn admitted_stream(access: &AdminAccess, mut req: Request, next: Next) -> Response {
    if req.method() != Method::GET {
        return AppError::MethodNotAllowed.into_response();
    }
    if let Err(err) = access.check(req.extensions().get::<Authenticated>()) {
        return err.into_response();
    }
    // Origin, limits and the stream's place, before any extractor of the handler.
    if let Err(refused) = crate::realtime::admit(&mut req, access).await {
        return *refused;
    }
    next.run(req).await
}

async fn guarded(access: &AdminAccess, req: Request, next: Next) -> Response {
    match access.check(req.extensions().get::<Authenticated>()) {
        Ok(()) => next.run(req).await,
        Err(err) => err.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_list_their_paths_and_permissions() {
        let routes = AdminRoutes::new("/docker")
            .get("/", AdminAccess::authenticated(), || async { "list" })
            .post("/{id}/restart", AdminAccess::can("docker.manage"), || async { "ok" })
            .nest("/logs", AdminRoutes::scope().get("/{id}", AdminAccess::can("docker.view"), || async { "log" }));
        let listed: Vec<_> = routes.list().into_iter().map(|(m, p, a)| (m.to_string(), p, a.clone())).collect();
        assert_eq!(
            listed,
            vec![
                ("GET".into(), "/docker".into(), AdminAccess::Authenticated),
                ("POST".into(), "/docker/{id}/restart".into(), AdminAccess::can("docker.manage")),
                ("GET".into(), "/docker/logs/{id}".into(), AdminAccess::can("docker.view")),
            ]
        );
        assert_eq!(routes.permissions().into_iter().collect::<Vec<_>>(), vec!["docker.manage", "docker.view"]);
    }

    #[test]
    #[should_panic(expected = "invalid permission name")]
    fn permission_names_are_checked() {
        let _ = AdminAccess::can("bad name");
    }
}
