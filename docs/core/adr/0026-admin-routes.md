# ADR 0026: Admin routes, declared and permissioned for the panel's session

- Status: accepted (0.4, W1)
- Date: 2026-10-17

## Context

Pages that drive something other than a database row (a service, a process,
a container) need JSON endpoints that the panel's own JavaScript can call.

- The panel authenticates with an `HttpOnly` session cookie, so its scripts
  have no bearer token.
- `ApiRoutes` accepts only API-guard tokens, refuses an admin session (401),
  and must not live under `admin_path`.
- Raw `app.routes(..)` works, but each handler then authorizes on its own,
  after its extractors have run, and the routes escape `route:list` unless
  described by hand.

## Decision

- **`AdminRoutes`** (rustcms-auth) declares routes under
  `{admin_path}/api/x{prefix}{path}`.
  - The `x/` namespace is reserved for applications: nothing built in lives
    there (`tests/paths.rs`), so no application route can shadow one.
  - The API is `ApiRoutes`' shape: `get/post/put/patch/delete(path, access,
    handler)`, `nest`, `list`, `permissions`, `register`.
- **`AdminAccess`** is a separate type with two values:
  - `authenticated()`: any active, signed-in admin session;
  - `can(permission)`: that session's user holds the permission in the
    `admin` guard; `super_admin` passes.

  There is **no public variant**, so an anonymous admin route cannot be
  written. Reusing `Access` would have allowed `Access::Public`.
- **Access is checked in a route layer**, before any extractor reads the
  path, query or body. A refused request is never parsed: 401 without a
  session, 403 without the permission.
- **The admin guard is unchanged** and applies first:
  - unsafe methods need an allowed `Origin` and the session's CSRF token
    (otherwise 403 `csrf_mismatch`);
  - a disabled user, a changed `auth_version` or a frozen session is
    anonymous;
  - responses are private and `no-store`;
  - API tokens and JWTs are not admin credentials, so they get 401.
- `HEAD` follows `GET`; other undeclared methods are 405, as for `ApiRoutes`.
- **Permissions are declared** in the `admin` guard by
  `AuthProvider::admin_permissions(&routes)`, so `permission:sync` creates
  them and the Roles page lists them.
- **`register`** mounts the routes and describes them for `route:list`
  (source `app`). It panics on a duplicate method and path.
- **`rustcms_testing::admin_routes::assert_access`** takes explicit actors and
  expected statuses, so a test states the intended access rather than
  re-deriving it from the declaration.

## Consequences

- Panel extensions (v1 script pages today, React pages later) call
  `{admin}/api/x/...` with the session cookie and `X-CSRF-Token`. They need no
  token and no hand-written authorization.
- Per-record decisions (this container, that site) stay in the handler, after
  the route's access: `AdminAccess` gates the route, not the target.
- Streaming routes (SSE, WebSocket) are not part of this ADR. They need
  tickets, a strict `Origin` check on upgrade and revalidation (plan W2, a
  later ADR).
