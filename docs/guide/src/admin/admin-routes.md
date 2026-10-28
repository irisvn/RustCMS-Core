# Admin routes

Resources cover database rows. When a panel page drives something else (a
service, a process, a remote API, a computed report), its script needs JSON
endpoints of its own. `AdminRoutes` declares them on the panel's session,
each with the access it requires, checked before the handler reads anything.
For clients with API tokens, use [`ApiRoutes`](../basics/routing.md) instead.

## Quick start

```rust
// src/routes/admin.rs
use rustcms::prelude::*;

pub fn routes() -> AdminRoutes {
    AdminRoutes::new("/reports").get("/summary", AdminAccess::can("reports.view"), summary)
}

async fn summary() -> Json<serde_json::Value> {
    Json(json!({ "posts": 42 }))
}
```

The routes are served under `{admin_path}/api/x`: here
`/admin/api/x/reports/summary`. Nothing built in uses `api/x`, so an
application route never shadows the panel's own.

## Full example

The demo's live page uses an event stream, a POST, a WebSocket and a GET,
all for users with `demo.live`:

<<< @/../../../examples/demo/src/routes/admin.rs#admin-routes [src/routes/admin.rs]

The same `AdminRoutes` value declares the permissions and mounts the routes:

<<< @/../../../examples/demo/src/bootstrap/mod.rs#register-api [src/bootstrap/mod.rs]

`providers::register` passes `admin` to `AuthProvider::admin_permissions`
(in `src/providers/auth_service_provider.rs`), which declares `demo.live` in
the `admin` guard, and labels it for the Roles page:

<<< @/../../../examples/demo/src/providers/auth_service_provider.rs#auth-provider [src/providers/auth_service_provider.rs]

A handler is an ordinary axum handler; this one uses a service bound in a
provider:

<<< @/../../../examples/demo/src/http/controllers/live_controller.rs#inject-reset [src/http/controllers/live_controller.rs]

![The Roles page showing the Live area and the Use live demos permission](/img/admin/admin-routes-roles.webp)
*A RustCMS demo application: a route's permission on the Roles page.*

## Declaring routes

| Method | Effect |
|---|---|
| `AdminRoutes::new("/prefix")` | routes under `{admin}/api/x/prefix`; the prefix is empty or starts with `/` |
| `AdminRoutes::scope()` | routes without a prefix of their own, to nest elsewhere |
| `.get`, `.post`, `.put`, `.patch`, `.delete` `(path, access, handler)` | a route; the path starts with `/` and may hold `{id}` or a final `{*rest}` |
| `.events(path, access, handler)` | a server-sent events stream ([Realtime](realtime.md)) |
| `.socket(path, access, handler)` | a WebSocket ([Realtime](realtime.md#websocket)) |
| `.nest("/prefix", routes)` | adds `routes` under `prefix`; each keeps its own access |
| `.list()` | every route as `(method, path under {admin}/api/x, access)` |
| `.permissions()` | the permissions the routes require, without duplicates |
| `.register(&mut app)` | mounts the routes and lists them in `rustcms route:list` (source `app`) |

```rust
// Sketch: one module per area, nested under a common prefix.
let docker = AdminRoutes::scope()
    .get("/containers", AdminAccess::can("docker.view"), docker::index)
    .post("/containers/{id}/restart", AdminAccess::can("docker.manage"), docker::restart);
let routes = AdminRoutes::new("/ops").nest("/docker", docker);   // /ops/docker/containers
```

The boot stops (a panic with the reason) on:

- a prefix, route path or `nest` prefix that does not start with `/`;
- `AdminAccess::can` with an empty name, whitespace, or a leading or
  trailing `.`;
- the same method and path declared twice (at `register`);
- an invalid `[auth]` section (at `register`).

::: details In a real application: RDockPanel's container routes
[RDockPanel](https://github.com/irisvn/RDockPanel) gives each route its own permission: reading
logs, following them as an event stream, live stats, and a shell over a WebSocket
([`src/routes/admin.rs`](https://github.com/irisvn/RDockPanel/blob/v0.9.3/src/routes/admin.rs#L40-L60),
v0.9.3, trimmed):

```rust
.get("/containers/{id}/logs", AdminAccess::can(permissions::DOCKER_LOGS), docker::logs)
.events("/containers/{id}/logs/follow", AdminAccess::can(permissions::DOCKER_LOGS), docker::follow_logs)
.events("/containers/{id}/stats", view(), docker::live_stats)
.socket("/containers/{id}/exec", AdminAccess::can(permissions::DOCKER_EXEC), docker::exec)
```
:::

## Access

| Access | Who |
|---|---|
| `AdminAccess::authenticated()` | any signed-in, active user of the panel |
| `AdminAccess::can("docker.manage")` | a signed-in user holding that permission in the `admin` guard (`super_admin` passes) |

There is no public access: an admin route always needs a signed-in user.

- Access is checked before any extractor reads the path, query or body. A
  refused request is never parsed: 401 without a session, 403 without the
  permission.
- The panel's guard applies first, as for every admin request:
  - `POST`, `PUT`, `PATCH` and `DELETE` need an allowed `Origin` and the
    session's CSRF token in `X-CSRF-Token` (otherwise 403 `csrf_mismatch`);
  - a disabled user, or a session whose credentials changed (password,
    sign-out everywhere), is signed out (401);
  - responses are `private, no-store`.
- API tokens and JWTs are not admin credentials (401).
- For `.get`, `.post` and the other method routes, `HEAD` follows `GET` and
  other undeclared methods are 405. `.events` and `.socket` routes answer
  405 to anything but `GET`, `HEAD` included.

The route's access gates the route, not the target. Decisions about one
record (this container, that site) stay in the handler, after it.

### Permissions

`AuthProvider::admin_permissions(&routes)` declares each `AdminAccess::can`
permission in the `admin` guard, in the group named before the first `.`
(`docker.manage` → `docker`). A permission you declared yourself with
`.permission(ADMIN_GUARD, …, group)` before it keeps your group.
`rustcms permission:sync` then creates them, and they appear on the Roles
page. `permission_meta` and `permission_area` give them labels, descriptions
and a section there
([The Roles page](../security/authorization.md#the-roles-page-areas-labels-and-presets)).

## Calling them from a page

A [React page](react-pages.md#calling-your-routes) calls them with
`props.api.get/post/put/patch/delete`, `useAdminQuery` and
`useAdminMutation`, which send the session and the CSRF token and check the
path for you:

```tsx
const hello = useAdminQuery<{ message: string }>(props, "/demo/hello");
const reset = useAdminMutation(props, "/demo/ticks/reset", "post");
```

### Calling them from a script page

A v1 [script page](customizing.md#script-pages) runs on the panel's origin
with the user's session cookie. Fetch the CSRF token and send it on writes,
as the demo's live page does:

<<< @/../../../examples/demo/resources/admin/js/demo/live.js#reset [resources/admin/js/demo/live.js]

`base` is the panel's `<base>` (`document.baseURI` without its trailing
slash, e.g. `https://example.com/admin`). On a 401 (signed out meanwhile),
call `ctx.reload()`. Encode each path segment you build from data, and keep
to the characters `A-Z a-z 0-9 . _ ~ -` when the path also goes to
`ctx.realtime` or `props.api`, which refuse others.

## Testing

`rustcms_testing::admin_routes::assert_access` sends each case as each actor
and checks the status you expect. It also fails when a declared route has no
case, so a new route cannot go untested. Cases name concrete paths; they
match route patterns with `{id}` (one segment) and `{*rest}` (the
remainder).

<<< @/../../../examples/demo/tests/feature/live.rs#assert-access [tests/feature/live.rs]

- Unsafe methods are sent with an empty JSON body (`{}`), so an allowed write
  usually answers 422 there, or its own status when it reads no body. Test
  the real bodies separately.
- An admitted event stream is checked by its status (200), then dropped.
- A socket route past its access answers 400 to an allowed actor: in-process
  requests carry no upgrade. Test the socket itself with
  `client.socket(path)` ([Testing](../basics/testing.md)).
- An anonymous client with a CSRF token (`client.fetch_csrf().await`)
  reaches the access check on writes (401); without one, the guard refuses
  first (403).

`rustcms route:list` shows them with the source `app` (the demo application,
trimmed):

```text
METHOD  PATH                            SOURCE
GET     /admin/api/x/demo/echo          app
GET     /admin/api/x/demo/hello         app
GET     /admin/api/x/demo/ticks         app
POST    /admin/api/x/demo/ticks/reset   app
GET     /api/v1/ping                    app
GET     /api/v1/posts                   app
POST    /api/v1/posts                   app
```

## Security notes

- Every route states its access; there is no default and no public access.
- Never authorize inside a handler after reading the body: the route's
  access runs first. Per-record checks (this container) belong in the handler,
  right after it, before any change.
- Event streams are read-only; sockets run commands, and each command calls
  `authorize()` and checks its own permission on the principal it returns.
  Both are rechecked while open. Ordinary data changes go through POST routes
  ([Realtime](realtime.md)).

## Related pages

- [Routing](../basics/routing.md): `ApiRoutes` for token clients
- [Realtime](realtime.md): `.events` and `.socket`
- [React pages](react-pages.md#calling-your-routes)
- [Roles & permissions](../security/authorization.md)
- [Testing](../basics/testing.md)
