# Routing

Your application's JSON API is a list of routes in `src/routes/api.rs`. Each
route names its HTTP method, its path, the access it requires and its handler.
Use it for endpoints called with an API token or a JWT (mobile apps, scripts,
other services). Endpoints for the admin panel's own pages are
[admin routes](../admin/admin-routes.md).

The files follow Laravel's layout:

```text
src/routes/api.rs                        routes(): every API route and its access
src/http/controllers/post_controller.rs  index, show, store, …
src/http/requests/post_request.rs        NewPost + Validate   (≈ form request)
src/http/resources/post_resource.rs      PostJson             (≈ API resource)
```

## Quick start

```rust
// src/routes/api.rs
use rustcms::prelude::*;

use crate::http::controllers; // the routes `make:api` adds use it

pub fn routes() -> ApiRoutes {
    ApiRoutes::new("/api/v1").get("/ping", Access::public(), ping)
    // rustcms:api-routes
}

async fn ping() -> Json<serde_json::Value> {
    Json(json!({ "pong": true }))
}
```

A new application already passes these routes to `AuthProvider` and registers
them in `src/bootstrap/mod.rs`. `rustcms route:list` shows them.

To generate a read-only API for a model, run `rustcms make:api Post`. It writes:

- `src/http/controllers/post_controller.rs` with `index` (a page of records,
  newest first when the model has a non-nullable `created_at` column of a
  `DateTime…` type, else highest key first)
  and `show` (one record, or 404);
- `src/http/resources/post_resource.rs` with `PostJson`, which follows the
  [wire format](../reference/wire-format.md): 64-bit integers, decimals, UUIDs
  and dates as strings, decimals at their declared scale, instants in UTC with
  `Z`;
- two routes in `src/routes/api.rs`:
  `GET /posts` with `Access::can("posts.view_any")` and `GET /posts/{id}` with
  `Access::can("posts.view")`.

`make:api` leaves out every column whose name contains `password`, `secret`,
`token`, `hash`, `api_key` or `salt`, and every column whose type it cannot
write as JSON (it prints a warning for each). It never overwrites a file. Add
write routes yourself (see [below](#write-routes-and-groups)).

## Full example

The demo application's API: a public ping, a paged list and a create route.

<<< @/../../../examples/demo/src/routes/api.rs#api-routes [src/routes/api.rs]

The same `ApiRoutes` value declares the permissions and mounts the routes:

<<< @/../../../examples/demo/src/bootstrap/mod.rs#register-api [src/bootstrap/mod.rs]

`providers::register` passes `api` to `AuthProvider::api_permissions` (in
`src/providers/auth_service_provider.rs`). `api_permissions` declares each
`Access::can` permission on the API guard, in the group named before the first
`.` (`posts.create` → `posts`). A permission you declare yourself with
`.permission(API_GUARD, …, group)` *before* `api_permissions` keeps your group.
Run `rustcms permission:sync` after adding routes, then grant the permissions to
roles.

The handlers:

<<< @/../../../examples/demo/src/http/controllers/post_controller.rs#index [src/http/controllers/post_controller.rs]

<<< @/../../../examples/demo/src/http/controllers/post_controller.rs#store [src/http/controllers/post_controller.rs]

The JSON resource the handlers return:

<<< @/../../../examples/demo/src/http/resources/post_resource.rs#post-json [src/http/resources/post_resource.rs]

`Pagination`, `ApiPage`, `Validated` and `Throttle` are described in
[Validation & pagination](validation.md).

## Access

Every route states its access; there is no default:

| Access | Allows |
|---|---|
| `Access::public()` | anyone, with or without a token |
| `Access::authenticated()` | any valid API token or JWT |
| `Access::can("posts.create")` | a caller whose user holds the permission (and whose token abilities allow it) |

Public routes answer with or without a caller. On the other routes, a caller
without a valid credential gets 401; on `Access::can` routes, an authenticated
caller whose user lacks the permission, or whose token abilities do not allow
it, gets 403.

Access is checked **before the handler runs**, so before any extractor reads
the request: without the permission a client gets 403 whatever its body, path
or query (only the global body limit, 413, comes first). Do not authorize
inside a handler after reading the body.

- Only API credentials count. An admin-panel session is not one (401).
- An invalid bearer token is 401 everywhere, public routes included.
- `HEAD` follows `GET`; other methods of a declared path answer 405.
- No API route may match a path under `auth.admin_path`, captures included
  (`/{section}/report` is refused with the default `/admin`): registration
  panics. The admin guard and CSRF apply there.

The demo's test checks these answers:

<<< @/../../../examples/demo/tests/feature/api.rs#api-access [tests/feature/api.rs]

## Write routes and groups

`ApiRoutes` has one method per HTTP method: `get`, `post`, `put`, `patch` and
`delete`. Each takes a path relative to the prefix, an `Access` and a handler.

`ApiRoutes::scope()` builds routes without a prefix of their own;
`.nest(prefix, routes)` adds them under a prefix. A sketch (the `show`,
`update` and `destroy` handlers are yours to write):

```rust
// src/routes/api.rs
use rustcms::prelude::*;

use crate::http::controllers;
use crate::http::controllers::post_controller;

fn posts() -> ApiRoutes {
    ApiRoutes::scope()
        .get("/", Access::can("posts.view_any"), post_controller::index)    // GET    /api/v1/posts
        .post("/", Access::can("posts.create"), post_controller::store)     // POST   /api/v1/posts
        .get("/{id}", Access::can("posts.view"), post_controller::show)
        .put("/{id}", Access::can("posts.update"), post_controller::update)
        .patch("/{id}", Access::can("posts.update"), post_controller::update)
        .delete("/{id}", Access::can("posts.delete"), post_controller::destroy)
}

pub fn routes() -> ApiRoutes {
    ApiRoutes::new("/api/v1")
        .get("/ping", Access::public(), ping)
        .nest("/posts", posts())
        // rustcms:api-routes
}
```

Paths are joined without doubled or trailing slashes: `"/posts"` + `"/"` is
`/posts`. A nested `ApiRoutes::new("/v2")` keeps its own prefix under the nest
prefix (`/api/v1/x/v2/…`).

Paths use Axum's syntax: `{id}` captures one segment, `{*rest}` the rest of
the path.

## Handlers

Handlers are ordinary Axum handlers returning `AppResult<…>`:

```rust
pub async fn show(State(app): State<AppState>, Path(id): Path<i64>) -> AppResult<Json<PostJson>> {
    let post = post::Entity::find_by_id(id).one(app.db()).await?.ok_or(AppError::NotFound)?;
    Ok(Json(PostJson::from(post)))
}
```

Use `rustcms::Json` (in the prelude) rather than `axum::Json`: rejections become
RustCMS errors (413 too large, 400 malformed, 422 wrong shape) in the standard
[format](errors.md).

The caller is available through two extractors:

| Extractor | Gives | Without a credential |
|---|---|---|
| `Auth` | `auth.0.user` (the `users` row), `auth.principal()` (permissions), `auth.authorize("perm")?` (403 unless allowed) | 401 |
| `MaybeAuth` | `Option<rustcms::auth::Authenticated>` | `MaybeAuth(None)` |

```rust
pub async fn mine(auth: Auth, State(app): State<AppState>) -> AppResult<Json<Vec<PostJson>>> {
    let user = &auth.0.user;
    let can_publish = auth.principal().can("posts.publish");
    // …
}
```

Use `MaybeAuth` on a public route that answers more to a signed-in caller.

The prelude (`use rustcms::prelude::*`) brings in what routes and handlers
need: `ApiRoutes`, `Access`, `Auth`, `MaybeAuth`, `Throttle`, `AppState`,
`AppError`, `AppResult`, `Json`, `Pagination`, `ApiPage`, `Validate`,
`Validated`, `ValidationErrors`, `ClientIp`, Axum's `Path`, `Query`, `State`,
`Router`, `Method`, `StatusCode`, the routing functions `get`, `post`, `put`,
`patch`, `delete`, and `serde_json::json`.

## Plain Axum routes

Routes that are not JSON APIs (webhooks, files) can be added directly:

```rust
app.routes(|router| router.route("/webhooks/stripe", post(stripe_webhook)));
app.describe_routes("app", [(Method::POST, "/webhooks/stripe".to_owned())]);
```

They get no declared access: check credentials yourself (a signature header
for a webhook, or the `Auth` extractor). They are not listed by `route:list`
unless you describe them with `app.describe_routes`.

Other `AppBuilder` methods for routes:

| Method | Does |
|---|---|
| `app.routes(\|router\| …)` | adds Axum routes |
| `app.describe_routes(source, [(method, path)])` | lists routes in `route:list` |
| `app.middleware(\|router, state\| …)` | wraps every application route (not the 404 fallback or `/health`) once all routes are added; runs inside the framework stack (request id, client IP, limits) |
| `app.body_limit(method, path, bytes)` | a body limit of its own for requests with exactly this method and path (e.g. an upload route); every other request keeps `http.body_limit` |

`body_limit` compares the request path literally: it applies to `/api/v1/uploads`,
not to a pattern such as `/api/v1/posts/{id}`.

## Pitfalls

Mistakes in route declarations panic when the application is built, so every
command that boots it (`serve`, `route:list`, your tests) fails at once:

| Mistake | Panics in |
|---|---|
| `Access::can("")`, a name with whitespace, or a leading or trailing `.` | `Access::can` |
| a prefix that is not empty and does not start with `/` | `ApiRoutes::new` |
| a nest prefix that does not start with `/` | `ApiRoutes::nest` |
| a path that does not start with `/` | `get`, `post`, … |
| two routes with the same method and full path | `ApiRoutes::register` |
| a route under `auth.admin_path` | `ApiRoutes::register` |

- `ApiRoutes` does nothing until you call `register(&mut app)`; the compiler
  warns when the value is dropped unused.
- Leaving out `api_permissions` does not make the routes public: `Access`
  still checks the caller's current grants and token abilities. But unless you
  declare the permissions another way (`.permission(API_GUARD, …)`),
  `permission:sync` does not create them, and marks existing ones orphaned:
  an orphaned permission no longer grants anything, so only super admins of
  the API guard pass `Access::can` checks.
- [`Throttle<N>`](validation.md#rate-limiting) needs a token: on a public
  route, a caller without one gets 401.

## Security

- Deny by default: a route without the permission answers 403 before its
  handler runs.
- Expose only what you mean to: return a JSON resource such as `PostJson`, never
  a model, so new columns stay private.
- An `Access::can` check is per route, not per record: filter records the
  caller may not see in the handler (answer 404 for them).

See [Authorization](../security/authorization.md) for roles and permissions and
[Authentication](../security/authentication.md#api-tokens) for API tokens.

## Related pages

- [Validation & pagination](validation.md)
- [Errors](errors.md)
- [Admin routes](../admin/admin-routes.md)
- [Testing](testing.md)
- [Wire format](../reference/wire-format.md)
