# Admin routes

Resources cover database rows. When a panel page drives something else (a
service, a process, a remote API), its script needs JSON endpoints of its
own. `AdminRoutes` declares them on the panel's session, each with the access
it requires, checked before the handler reads anything.

```rust,ignore
// src/routes/admin.rs
use rustcms::prelude::*;

pub fn routes() -> AdminRoutes {
    AdminRoutes::new("/docker")
        .get("/containers", AdminAccess::can("docker.view"), controllers::docker::index)
        .post("/containers/{id}/restart", AdminAccess::can("docker.manage"), controllers::docker::restart)
        .get("/me", AdminAccess::authenticated(), controllers::docker::me)
}

// src/bootstrap/mod.rs
let admin = crate::routes::admin::routes();
app.provider(AuthProvider::new().admin_permissions(&admin));   // docker.view, docker.manage (admin guard)
admin.register(&mut app);
```

The routes are served under `{admin_path}/api/x`: `/admin/api/x/docker/containers`.
Nothing built in uses `api/x`, so an application route never shadows the
panel's own.

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
- API tokens and JWTs are not admin credentials (401). For token clients, use
  [`ApiRoutes`](../basics/routing.md).
- `HEAD` follows `GET`; other undeclared methods are 405.

The route's access gates the route, not the target. Decisions about one
record (this container, that site) stay in the handler, after it.

`admin_permissions` declares each permission in the `admin` guard (group: the
first segment, `docker`). `rustcms permission:sync` then creates them, and
they appear on the Roles page. `register` also lists the routes in
`rustcms route:list`, and refuses a method and path declared twice.

## Calling them from a script page

A [script page](customizing.md#script-pages) runs on the panel's origin with
the user's session cookie. Fetch the CSRF token once and send it on writes:

```js
const base = document.baseURI.replace(/\/$/, "");   // the panel's <base>, e.g. https://example.com/admin
async function csrf() {
  const response = await fetch(`${base}/api/auth/csrf`, { credentials: "same-origin" });
  return (await response.json()).csrfToken;
}

window.rustcms.registerPage("docker/containers", {
  apiVersion: 1,
  mount(element, ctx) {
    const restart = async (id) => {
      const response = await fetch(`${base}/api/x/docker/containers/${encodeURIComponent(id)}/restart`, {
        method: "POST",
        credentials: "same-origin",
        headers: { "Content-Type": "application/json", "X-CSRF-Token": await csrf() },
        body: JSON.stringify({ force: false }),
      });
      if (response.status === 401) ctx.reload();   // signed out meanwhile
      return response.json();
    };
    // … build the list from ctx.data, wire buttons to restart(id) …
    return { update() {}, unmount: () => element.replaceChildren() };
  },
});
```

## Testing

`rustcms_testing::admin_routes::assert_access` sends each case as each actor
and checks the status you expect. It also fails when a declared route has no
case, so a new route cannot go untested.

```rust,ignore
use rustcms_testing::admin_routes::{Case, assert_access};

let anonymous = app.client();
let viewer = app.acting_as(&viewer).await;
let operator = app.acting_as(&operator).await;
assert_access(&app, &routes(), &[
    Case::new(Method::GET, "/docker/containers")
        .expect("anonymous", &anonymous, StatusCode::UNAUTHORIZED)
        .expect("viewer", &viewer, StatusCode::OK),
    Case::new(Method::POST, "/docker/containers/web/restart")
        .expect("viewer", &viewer, StatusCode::FORBIDDEN)
        .expect("operator", &operator, StatusCode::UNPROCESSABLE_ENTITY), // allowed; `{}` is not a valid body
    Case::new(Method::GET, "/docker/me").expect("viewer", &viewer, StatusCode::OK),
]).await;
```

Unsafe methods are sent with an empty JSON body, so an allowed write usually
answers 422 there. Test the real bodies separately. An anonymous client with
a CSRF token (`client.fetch_csrf().await`) reaches the access check on writes
(401); without one, the guard refuses first (403).

Streaming (server-sent events, WebSocket) is not available on admin routes
yet.
