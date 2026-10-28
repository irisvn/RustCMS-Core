# Routing

Your JSON API is laid out as in Laravel:

```text
src/routes/api.rs                        routes(): every API route and its access
src/http/controllers/post_controller.rs  index, show, store, …
src/http/requests/post_request.rs        NewPost + Validate   (≈ form request)
src/http/resources/post_resource.rs      PostJson             (≈ API resource)
```

`rustcms make:api Post` writes the controller (`index`, newest first when the
model has `created_at`, else highest key first, and `show`), the JSON
resource, and their two routes in `src/routes/api.rs`. The JSON resource
follows the [wire format](../reference/wire-format.md): 64-bit
integers, UUIDs and dates as strings, decimals at their declared scale,
instants in UTC with `Z`; secret-looking columns (`password`, `token`, …) are
left out.

## Declaring routes

```rust
// src/routes/api.rs
use crate::http::controllers::post_controller;

pub fn routes() -> ApiRoutes {
    ApiRoutes::new("/api/v1")
        .get("/ping", Access::public(), ping)
        .get("/posts", Access::can("posts.view_any"), post_controller::index)
        .post("/posts", Access::can("posts.create"), post_controller::store)
        .get("/posts/{id}", Access::can("posts.view_any"), post_controller::show)
        // rustcms:api-routes
}

// src/bootstrap/mod.rs (and providers/auth_service_provider.rs)
let api = crate::routes::api::routes();
app.provider(AuthProvider::new().api_permissions(&api)); // declares posts.view_any, posts.create
api.register(&mut app);                                  // mounts them and lists them in `route:list`
```

## Access

Every route states its access; there is no default:

| Access | Allows |
|---|---|
| `Access::public()` | anyone |
| `Access::authenticated()` | any valid API token or JWT |
| `Access::can("posts.create")` | a caller whose user holds the permission (and whose token abilities allow it) |

Access is checked **before the handler runs**, so before any extractor reads
the request: without the permission a client gets 403 whatever its body, path
or query (only the global body limit, 413, comes first). Do not authorize
inside a handler after reading the body.

- Admin-panel sessions are not API credentials (401).
- An invalid bearer token is 401 everywhere, public routes included.
- `HEAD` follows `GET`; other methods of a path answer 405.
- No API route may match a path under `auth.admin_path`, captures included
  (`/{section}/report` is refused with the default `/admin`): registration panics.
  Endpoints for the panel's own pages, on its session, are
  [admin routes](../admin/admin-routes.md).

`api_permissions` declares each `Access::can` permission on the API guard, in
the group named before the first `.` (`posts.create` → `posts`). A permission
you declare yourself with `.permission(API_GUARD, …, group)` keeps your group.
Run `rustcms permission:sync` after adding routes.

## Handlers

Handlers are ordinary Axum handlers returning `AppResult<…>`:

```rust
pub async fn show(State(app): State<AppState>, Path(id): Path<i64>) -> AppResult<Json<PostJson>> {
    let post = post::Entity::find_by_id(id).one(app.db()).await?.ok_or(AppError::NotFound)?;
    Ok(Json(PostJson::from(post)))
}
```

Use `rustcms::Json` rather than `axum::Json`: rejections become RustCMS
errors (413 too large, 400 malformed, 422 wrong shape) in the standard
[format](errors.md). `Auth` gives the authenticated user:

```rust
pub async fn mine(auth: Auth, State(app): State<AppState>) -> AppResult<Json<Vec<PostJson>>> {
    let user = &auth.0.user;                 // the users row
    let can_publish = auth.principal().can("posts.publish");
    // …
}
```

## Plain Axum routes

Routes that are not JSON APIs (webhooks, files) can be added directly:

```rust
app.routes(|router| router.route("/webhooks/stripe", post(stripe_webhook)));
```

They are not listed by `route:list` unless you describe them
(`app.describe_routes(…)`), and they get no declared access: check
credentials yourself.

