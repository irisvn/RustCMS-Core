# API (tokens)

Everything outside the admin path uses the **API guard**: requests carry a
personal access token, `Authorization: Bearer rcms_<id>_<secret>`. Tokens are
stored hashed, can expire, and carry **abilities**: a request may do what both
the token's abilities and the user's API-guard permissions allow (`*` = all).

The user needs API-guard permissions first (roles of the `api` guard):

```sh
rustcms role:assign --email you@example.com --role super_admin --guard api   # or any api role
rustcms token:create --email you@example.com --name deploy --abilities posts.view_any --expires-days 30
```

Or over HTTP: `POST /api/v1/auth/token` with `{email, password, token_name,
abilities, expires_in_days}` (rate limited like sign-in); `DELETE
/api/v1/auth/token` revokes the token used; `GET /api/v1/auth/me`.

## JWT sign-ins

Apps that sign users in (mobile, browser) use short access tokens instead
(`auth.jwt.enabled`, on by default):

```text
POST /api/v1/auth/jwt          {email, password, abilities?}   → 201
POST /api/v1/auth/refresh      {refreshToken}                   → 200
POST /api/v1/auth/jwt/logout   {refreshToken}                   → 204
```

Both sign-in and refresh answer `{accessToken, tokenType: "Bearer", expiresIn,
refreshToken, refreshExpiresAt, abilities}`. Send `Authorization: Bearer
<accessToken>` like an API token; abilities (omitted = `*`) are fixed for the
sign-in and narrow the user's API permissions.

- Refresh before `expiresIn` runs out, with the **latest** refresh token. Each
  refresh token works once: a second use ends the sign-in (both tokens stop
  working). Refresh one request at a time (share one in-flight refresh across
  tabs); if a refresh response is lost, sign in again.
- A refresh token lasts `refresh_ttl_days` (14) without use; a sign-in lasts at
  most `refresh_family_days` (30).
- Password change, sign-out everywhere and disabling the user end every sign-in
  from the next request (see [revocation](security/revocation.md)).

## Writing endpoints

An application's API lives in `src/http/api/`, one module per model (Laravel
keeps the same things in `routes/api.php`, controllers, form requests and API
resources):

```text
src/http/api/
├── mod.rs              routes(): every API route
└── posts/
    ├── mod.rs          routes() of posts, with the access each requires
    ├── handlers.rs     list, create, …            (≈ controller)
    ├── requests.rs     NewPost + Validate         (≈ form request)
    └── json.rs         PostJson                   (≈ API resource)
```

`rustcms make:api Post` writes `posts/` with `list` (newest first when the
model has `created_at`, else highest key first) and `show`, and registers it.
`json.rs` follows the wire format (ADR 0003): 64-bit integers, UUIDs and dates
as strings, decimals at their declared scale, instants in UTC with `Z`; secret
looking columns (`password`, `token`, …, also behind `column_name`) are left
out. Models with a composite key are refused; a table name that is not a Rust
identifier (`stock-items`) keeps its URL and gets a module `stock_items`.

### Routes and access

```rust,ignore
// src/http/api/mod.rs
pub fn routes() -> ApiRoutes {
    ApiRoutes::new("/api/v1")
        .get("/ping", Access::public(), ping)
        .nest("/posts", posts::routes())
}

// src/http/api/posts/mod.rs
pub fn routes() -> ApiRoutes {
    ApiRoutes::scope()
        .get("/", Access::can("posts.view_any"), handlers::list)
        .post("/", Access::can("posts.create"), handlers::create)
}

// src/app.rs
let api = crate::http::api::routes();
app.provider(AuthProvider::new().api_permissions(&api)); // declares posts.view_any, posts.create
api.register(&mut app);                                  // mounts and lists them (`route:list`)
```

Every route states its access; there is no default:

| Access | Allows |
|---|---|
| `Access::public()` | anyone |
| `Access::authenticated()` | any valid API token |
| `Access::can("posts.create")` | a token whose user has the permission (and whose abilities allow it) |

Access is checked **before the handler runs**, so before any extractor reads
the request: without the permission a client gets 403 whatever its body, path
or query (only the global body size limit, 413, comes first). Admin-panel
sessions are not API credentials (401), and no API route may be able to match
a path under `auth.admin_path`, captures included (`/{section}/report` is
refused with the default `/admin`): registration panics. A request with an invalid bearer token
is 401 everywhere, public routes included. `HEAD` follows `GET`; other methods
of a path are 405.

`api_permissions` declares each `Access::can` permission on the API guard, in
the group named before the first `.` (`posts.create` → `posts`); a permission
you declare yourself with `.permission(API_GUARD, …, group)` keeps your group.

### Input, pages, limits

```rust,ignore
pub async fn list(State(app): State<AppState>, page: Pagination) -> AppResult<Json<ApiPage<PostJson>>> {
    let select = post::Entity::find().order_by_desc(post::Column::Id); // always order a paged list
    Ok(Json(ApiPage::fetch(select, app.db(), &page, PostJson::from).await?))
}

pub async fn create(
    State(app): State<AppState>,
    auth: Auth,
    _: Throttle<30>,                       // 30 a minute per user, counted before the body is read
    Validated(input): Validated<NewPost>,  // parsed, then NewPost::validate
) -> AppResult<(StatusCode, Json<PostJson>)> { … }
```

- `Pagination`: `?page` (1–10,000) and `?per_page` (1–100, default 20); out of
  range, not a number or given twice → 422 on that field.
- `ApiPage<T>`: `{data: […], meta: {page, perPage, total, lastPage}}`.
- `Validated<T>`: the JSON body (413 too large, 400 not JSON, 422 not a `T` —
  use `#[serde(deny_unknown_fields)]`), then `T::validate`, which may normalize
  (trim) and returns field errors (422).
- `Throttle<N>`: N requests a minute per user and route (429 with
  `Retry-After`). Axum runs it before the body extractor.

A complete example is `examples/demo/src/http/api/posts/` (published posts with
`posts.view_any`, drafts with `posts.create`), tested in
`examples/demo/tests/api.rs`.

Token lifetimes are 1 to 3,650 days (`expires_in_days`, `--expires-days`);
without one, a token lasts until revoked.

Revocation (password change, sign-out everywhere, disabling) applies from the
next request. A request already running is not aborted: your endpoints, like
admin resource writes, finish with the credentials checked when they started.
Account and RBAC changes made through `RbacService` are the exception: they
re-check the credential version under lock and are refused. See
[Credential revocation](security/revocation.md).
