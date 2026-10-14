# Authentication

`AuthProvider` (registered by `rustcms new`) adds two guards, chosen by path:

| Guard   | Paths                       | Credential                                   |
|---------|-----------------------------|----------------------------------------------|
| `admin` | under `auth.admin_path`     | server-side session cookie                   |
| `api`   | everything else             | `Authorization: Bearer rcms_<id>_<secret>` (API token) or `Bearer <JWT>` (access token) |

A credential that is present but invalid is a 401: there is no fallback to
another driver. An admin cookie never authenticates API routes, and a bearer
token never authenticates admin routes.

For API routes, state the access on the route (`ApiRoutes`, see
[Routing](../basics/routing.md)): it is checked before the request body is read. In a handler,
`Auth` gives the user and its permissions:

```rust,ignore
use rustcms::prelude::*;

// Routed with `.post("/", Access::can("posts.create"), create_post)`.
async fn create_post(auth: Auth, Validated(input): Validated<NewPost>) -> AppResult<StatusCode> {
    // auth.0.user is the user record, auth.principal() the permissions
    Ok(StatusCode::CREATED)
}
```

Checking a permission inside a handler (`auth.authorize(…)`) runs after the
handler's extractors, so after the body was read: prefer route access.

## Admin sessions

- The cookie (`rustcms_session`, `HttpOnly`, `SameSite=Lax`, `Path=<admin_path>`,
  `Secure` when `APP_URL` is https) holds 43 random characters. The database
  stores only their HMAC-SHA256 keyed with `APP_KEY`: rotating the key
  (`key:generate --force`) signs every admin out (API tokens are kept).
- The cookie has no `Max-Age`: it lasts until the browser closes, and expiry is
  enforced by the server. It is sent only when its value changes (sign-in,
  re-issue, sign-out), so a slow response can never restore a replaced cookie.
- The session id is regenerated at sign-in and whenever credentials change.
- Sessions expire after `session_lifetime_minutes` of inactivity; `serve` deletes
  expired rows hourly (`session:prune` does it on demand; `idempotency:prune`
  does the same for idempotency receipts).
- Anonymous sessions (a CSRF token before sign-in) live a **fixed**
  `anonymous_session_lifetime_minutes` (15): activity never extends them.
  Creating one is throttled per IP (`anonymous_sessions_per_ip`, 30/min) and
  globally (`anonymous_sessions_per_minute`, 1,000/min), so at most
  1,000 × 15 anonymous rows are alive; an existing session can always fetch its
  token again.
- Sliding renewal is best effort: if writing it fails, the response the handler
  produced (possibly after committing changes) is returned unchanged.
- A credential change (password, sign-out everywhere) re-issues the session
  within its *lineage* (the first session of the chain), only if it still
  exists. Signing out ends the whole lineage. Both take a per-lineage lock
  first, so a sign-out and concurrent re-issues — however many — never leave a
  live session, in any order. Signing in starts a new lineage.
- Every request reloads the user: a disabled user, or one whose `auth_version`
  changed, is signed out immediately.
- If another request ends the session while one is in flight, the in-flight
  response is kept and the session stays ended (it is never re-created).

### CSRF

State-changing admin requests (anything but GET/HEAD/OPTIONS), including
sign-in and sign-out, must:

1. come from an allowed origin: `Origin` (or, without it, `Referer`) must equal
   one of `auth.admin_origins` (default: the origin of `APP_URL`);
2. carry `X-CSRF-Token` equal to the session's token, from
   `GET <admin>/api/auth/csrf`. The token rotates at sign-in.

| Endpoint (`<admin>/api/auth/…`) | Does                                             |
|----------------------------------|-------------------------------------------------|
| `GET csrf`                       | starts a session, returns `{csrfToken}`         |
| `POST login`                     | `{email, password}`                             |
| `POST logout`                    | ends the session                                |
| `GET me`                         | user, guard, permissions, interface language    |
| `POST locale`                    | `{locale}`: saves the user's interface language |
| `POST password`                  | `{current_password, password, password_confirmation}` |
| `POST logout-others`             | `{password}`: ends every other session, revokes tokens |

## API tokens

How clients obtain and use tokens is in [API authentication](../api/authentication.md).
`POST /api/v1/auth/token` with `{email, password, token_name, abilities?, expires_in_days?}`
returns the token once. `DELETE /api/v1/auth/token` revokes the current one;
`GET /api/v1/auth/me` describes it. Operators use `rustcms token:create`.

A token acts with the **intersection** of its abilities and the user's
permissions in the `api` guard. `["*"]` means all of the user's permissions.
Even a super admin's token is limited to its abilities.

## JWT sign-ins

`POST /api/v1/auth/jwt` with `{email, password, abilities?}` (same limits as
sign-in, plus 20 sign-ins per user per minute) returns an access token (a JWT,
`auth.jwt.access_ttl_secs`, default 15 minutes) and a refresh token.
`POST /api/v1/auth/refresh {refreshToken}` returns the next pair; each refresh
token works **once**: using a spent one again (a replay, two tabs racing, or a
retry after a lost response) ends the whole sign-in, so clients must refresh
one at a time and keep the newest token. `POST /api/v1/auth/jwt/logout
{refreshToken}` ends the sign-in (also after the access token expired).
Refresh and logout are limited per IP (60/min) and per sign-in (30/min).

Access tokens are HS256 with a key derived from `APP_KEY` (rotating it ends
every sign-in); only that exact header is accepted. Each request still checks
the user (active, same `auth_version`) and the sign-in (not ended), so
revocation applies from the next request, as for sessions. Abilities are fixed
per sign-in and narrow the user's API permissions like a token's.

## Throttling

Sign-in attempts (admin login and token issuance) are limited per account+IP
(`attempts_per_identifier`, default 5/min) and per IP (`attempts_per_ip`,
30/min); then 429 with `Retry-After`. Each attempt is counted *before* the
password is verified, so a burst of concurrent guesses cannot exceed the limit,
and a request cancelled mid-check stays counted; a successful sign-in clears
the account counter and refunds the IP. The IP comes from the
[trusted-proxy rules](../architecture/lifecycle.md#client-ip-and-proxies). Unknown emails
cost the same Argon2 time as wrong passwords and return the same message.

Emails longer than 255 characters and passwords longer than 1024 are rejected
before any throttling state or hash is built. Throttling keys are stored as
fixed-size hashes; expired windows are reclaimed and the table is capped
(100,000 keys, oldest dropped first), so unauthenticated traffic cannot grow
memory without bound.

Limits are kept in memory, per instance; with Redis (`[redis] url`, feature
`redis`) every instance counts in Redis, atomically across all the keys of one
check. If Redis does not answer, checks fail closed (500). See
[Several instances](../digging-deeper/instances.md).
