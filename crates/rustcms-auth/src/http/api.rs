//! Routes of an application's JSON API, each with the access it requires.
//!
//! ```rust,ignore
//! // src/http/api/mod.rs
//! pub fn routes() -> ApiRoutes {
//!     ApiRoutes::new("/api/v1").nest("/posts", posts::routes())
//! }
//! // src/http/api/posts/mod.rs
//! pub fn routes() -> ApiRoutes {
//!     ApiRoutes::scope()
//!         .get("/", Access::can("posts.view_any"), handlers::list)
//!         .post("/", Access::can("posts.create"), handlers::create)
//! }
//! // src/app.rs
//! let api = crate::http::api::routes();
//! app.provider(AuthProvider::new().api_permissions(&api));
//! api.register(&mut app);
//! ```
//!
//! Access is checked before the handler runs, so before any of its extractors
//! reads the request: a caller without the permission gets 403 whatever the
//! body, path or query (only the global body size limit, 413, comes first).
//! Only API-guard tokens are accepted (an admin session is 401), and API routes
//! may not sit under `auth.admin_path` (the admin guard and CSRF apply there).
//! A request with an invalid bearer token is a 401 everywhere, public routes
//! included: the guard checks every credential it is given. `HEAD` follows the
//! access of `GET`; other undeclared methods are 405.
//!
//! Put [`Throttle`] before a body extractor to count attempts before the body
//! is read and validated.

use std::collections::{BTreeMap, BTreeSet};

use axum::extract::{FromRequestParts, MatchedPath, Request};
use axum::handler::Handler;
use axum::http::Method;
use axum::http::request::Parts;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{MethodRouter, delete, get, patch, post, put};
use rustcms_core::{AppBuilder, AppError, AppState};

use crate::config::{AuthConfig, normalize_admin_path};
use crate::guard::Authenticated;
use crate::state::{API_GUARD, AuthState};

/// What a route requires of the caller. Every route states one: there is no default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Access {
    /// Anyone, with or without a token.
    Public,
    /// Any valid API token.
    Authenticated,
    /// An API token whose user has this permission (and whose abilities allow it).
    Can(String),
}

impl Access {
    /// Anyone, with or without a token.
    pub fn public() -> Self {
        Self::Public
    }

    /// Any valid API token.
    pub fn authenticated() -> Self {
        Self::Authenticated
    }

    /// An API token allowed `permission` (e.g. `posts.view_any`).
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
            "invalid permission name {permission:?} (e.g. `posts.view_any`)"
        );
        Self::Can(permission)
    }

    fn check(&self, caller: Option<&Authenticated>) -> Result<(), AppError> {
        let caller = caller.filter(|c| c.principal.guard == API_GUARD);
        match (self, caller) {
            (Self::Public, _) => Ok(()),
            (_, None) => Err(AppError::Unauthenticated),
            (Self::Authenticated, Some(_)) => Ok(()),
            (Self::Can(permission), Some(c)) => {
                if c.principal.can(permission) {
                    Ok(())
                } else {
                    Err(AppError::Forbidden)
                }
            }
        }
    }
}

struct Route {
    method: Method,
    /// Relative to the owning [`ApiRoutes`]' prefix.
    path: String,
    access: Access,
    router: MethodRouter<AppState>,
}

/// A set of API routes under a prefix. See the [module](self) documentation.
#[must_use = "register the routes with `ApiRoutes::register`"]
pub struct ApiRoutes {
    prefix: String,
    routes: Vec<Route>,
}

macro_rules! methods {
    ($($name:ident => $method:ident, $route:ident;)*) => {$(
        #[doc = concat!("A `", stringify!($method), "` route at `path` (relative to the prefix).")]
        pub fn $name<H, T>(self, path: &str, access: Access, handler: H) -> Self
        where
            H: Handler<T, AppState>,
            T: 'static,
        {
            self.add(Method::$method, path, access, $route(handler))
        }
    )*};
}

impl ApiRoutes {
    /// Routes under `prefix` (e.g. `/api/v1`).
    ///
    /// # Panics
    /// When `prefix` is not empty and does not start with `/`.
    pub fn new(prefix: &str) -> Self {
        assert!(prefix.is_empty() || prefix.starts_with('/'), "API prefixes start with `/`: {prefix:?}");
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

    /// Adds `routes` (with their own prefix, if any) under `prefix`, relative
    /// to this one.
    pub fn nest(mut self, prefix: &str, routes: ApiRoutes) -> Self {
        assert!(prefix.starts_with('/'), "nested prefixes start with `/`: {prefix:?}");
        let base = join(prefix, &routes.prefix);
        for route in routes.routes {
            self.routes.push(Route { path: join(&base, &route.path), ..route });
        }
        self
    }

    fn add(mut self, method: Method, path: &str, access: Access, router: MethodRouter<AppState>) -> Self {
        assert!(path.starts_with('/'), "API paths start with `/`: {path:?}");
        let checked = access.clone();
        let router = router.route_layer(middleware::from_fn(move |req: Request, next: Next| {
            let access = checked.clone();
            async move { guarded(&access, req, next).await }
        }));
        self.routes.push(Route { method, path: join("", path), access, router });
        self
    }

    /// Every route as `(method, full path, access)`, in declaration order.
    pub fn list(&self) -> Vec<(Method, String, &Access)> {
        self.routes.iter().map(|r| (r.method.clone(), join(&self.prefix, &r.path), &r.access)).collect()
    }

    /// The permissions the routes require, without duplicates.
    pub fn permissions(&self) -> BTreeSet<&str> {
        self.routes
            .iter()
            .filter_map(|r| match &r.access {
                Access::Can(permission) => Some(permission.as_str()),
                _ => None,
            })
            .collect()
    }

    /// Mounts the routes and describes them for `route:list` (source `app`),
    /// both from this list.
    ///
    /// # Panics
    /// When two routes have the same method and path, or a route is under
    /// `auth.admin_path`.
    pub fn register(self, app: &mut AppBuilder) {
        let config: AuthConfig =
            app.config().section("auth").unwrap_or_else(|e| panic!("invalid [auth] configuration: {e}"));
        let admin = normalize_admin_path(&config.admin_path).unwrap_or_else(|e| panic!("{e}"));
        let mut seen = BTreeSet::new();
        for (method, path, _) in self.list() {
            assert!(
                !may_match_under(&path, &admin),
                "API route {method} {path} is under the admin path `{admin}` (its guard and CSRF apply there)"
            );
            assert!(seen.insert((path.clone(), method.to_string())), "API route {method} {path} is declared twice");
        }
        app.describe_routes("app", self.list().into_iter().map(|(method, path, _)| (method, path)));
        let mut by_path: BTreeMap<String, MethodRouter<AppState>> = BTreeMap::new();
        for route in self.routes {
            let path = join(&self.prefix, &route.path);
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

async fn guarded(access: &Access, req: Request, next: Next) -> Response {
    match access.check(req.extensions().get::<Authenticated>()) {
        Ok(()) => next.run(req).await,
        Err(err) => err.into_response(),
    }
}

/// Whether a request matching the route pattern `path` can fall under `admin`
/// (`/admin`, `/admin/…`): segments are compared one by one, a capture
/// (`{id}`) matches any segment and a catch-all (`{*rest}`) everything after.
fn may_match_under(path: &str, admin: &str) -> bool {
    let route: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let admin: Vec<&str> = admin.split('/').filter(|s| !s.is_empty()).collect();
    for (i, wanted) in admin.iter().enumerate() {
        match route.get(i) {
            None => return false,
            Some(segment) if segment.starts_with("{*") => return true,
            Some(segment) if segment.starts_with('{') || segment == wanted => {}
            Some(_) => return false,
        }
    }
    true
}

/// `"/api" + "/"` → `/api`, `"/api" + "/posts"` → `/api/posts`, `"" + "/"` → `/`.
fn join(prefix: &str, path: &str) -> String {
    let path = path.trim_end_matches('/');
    let joined = format!("{}{path}", prefix.trim_end_matches('/'));
    if joined.is_empty() { "/".to_owned() } else { joined }
}

/// Counts a user's requests to a route, `PER_MINUTE` a minute (429 with
/// `Retry-After` above). Place it before a body extractor so attempts are
/// counted before the body is read and validated:
///
/// ```rust,ignore
/// async fn create(_: Throttle<30>, auth: Auth, Validated(input): Validated<NewPost>) -> AppResult<StatusCode> {
///     todo!()
/// }
/// ```
///
/// Needs a token: use it on routes whose access is not public.
pub struct Throttle<const PER_MINUTE: u32>;

impl<const PER_MINUTE: u32> FromRequestParts<AppState> for Throttle<PER_MINUTE> {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, app: &AppState) -> Result<Self, AppError> {
        let caller = parts.extensions.get::<Authenticated>().ok_or(AppError::Unauthenticated)?;
        let route = parts.extensions.get::<MatchedPath>().map_or_else(|| parts.uri.path(), MatchedPath::as_str);
        let state = app.service::<AuthState>().ok_or_else(|| AppError::internal("AuthProvider not registered"))?;
        state.limiter.reserve(&[(&format!("api:{route}:{}", caller.user.id), PER_MINUTE)]).await?;
        Ok(Self)
    }
}

/// The group of a permission in the permission list (`posts.create` → `posts`).
pub(crate) fn group_of(permission: &str) -> &str {
    permission.split('.').next().unwrap_or(permission)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_join_without_doubled_or_trailing_slashes() {
        assert_eq!(join("/api/v1", "/"), "/api/v1");
        assert_eq!(join("/api/v1", "/posts"), "/api/v1/posts");
        assert_eq!(join("/api/v1/", "/posts/"), "/api/v1/posts");
        assert_eq!(join("", "/"), "/");
        assert_eq!(join("", "/{id}"), "/{id}");
        assert_eq!(group_of("posts.view_any"), "posts");
    }

    #[test]
    fn route_patterns_that_can_reach_the_admin_path_are_found() {
        for path in ["/admin", "/admin/report", "/{section}/report", "/{*rest}", "/{x}"] {
            assert!(may_match_under(path, "/admin"), "{path}");
        }
        for path in ["/", "/api/v1/posts", "/api/{id}", "/administrator", "/adm/{x}"] {
            assert!(!may_match_under(path, "/admin"), "{path}");
        }
        assert!(may_match_under("/{x}/y", "/back-office"));
        assert!(!may_match_under("/admin/y", "/back-office"));
    }
}
