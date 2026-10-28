# Authentication

`AuthProvider` (registered by `rustcms new`) decides who makes each request. It
adds two guards, chosen by path, the sign-in, password and token endpoints, and
the commands that manage accounts. What a signed-in user may then do is
[Roles & permissions](authorization.md).

| Guard | Paths | Credential |
|---|---|---|
| `admin` | under `auth.admin_path` (`/admin`) | a server-side session cookie |
| `api` | everything else | `Authorization: Bearer rcms_<id>_<secret>` (API token) or `Bearer <JWT>` (access token) |

On API paths, an `Authorization` header that is present but invalid (unknown,
expired or revoked token or JWT) is a 401 on every route, public ones
included: there is no fallback to another credential. Under the admin path,
an unknown or expired session cookie is not an error: the request is
anonymous, and gets 401 only where a route requires a signed-in user. An
unsafe admin request (POST, …) can get 403 from the origin and CSRF checks
before authentication is looked at. An admin cookie never authenticates API
routes, and a bearer token never authenticates admin routes.

![The admin sign-in page with the Remember me box and the Forgot password link](/img/security/sign-in.webp)
*RBlog, a blog built on RustCMS: the sign-in page.*

## Quick start

A new application registers the provider in `src/bootstrap/providers.rs`, with
the permissions its routes need ([Roles & permissions](authorization.md#declaring-permissions)).
Each API route states the access it requires; it is checked before the request
body is read ([Routing](../basics/routing.md#access)):

<<< @/../../../examples/demo/src/routes/api.rs#api-routes [src/routes/api.rs]

In a handler, `Auth` gives the signed-in user and their permissions:

<<< @/../../../examples/demo/src/http/controllers/post_controller.rs#store [src/http/controllers/post_controller.rs]

Create the first account from the console, then sign in at `/admin`:

```sh
rustcms make:user --email admin@example.com --name Admin --super-admin
```

## Extractors

| Extractor | Gives | Without a signed-in user |
|---|---|---|
| `Auth` | `auth.0`, an `Authenticated` | 401 |
| `MaybeAuth` | `Option<Authenticated>` | `MaybeAuth(None)` |
| `Session` | the admin session (`rustcms::auth::Session`), under the admin path only | 500 outside the admin path |

`Authenticated` has four fields: `principal` (the guard, the permissions and
token abilities: `rustcms::Principal`), `user` (the `users` row, loaded for
this request), `token_id` (the API token used, if any) and `jwt_family` (the
JWT sign-in used, if any).

| Method | Does |
|---|---|
| `auth.principal()` | the `Principal` |
| `auth.authorize("posts.publish")?` | 403 unless the principal has the permission |
| `auth.authorize_guard("api", "posts.publish")?` | the same, and 401 for a principal of another guard |

Checking a permission inside a handler runs after the handler's extractors, so
after the body was read: prefer route access. For an admin page's JSON routes,
see [Admin routes](../admin/admin-routes.md).

`rustcms::auth::routes::confirm_password(&auth_state, &user, password)` checks
the current password of a signed-in user, throttled per user (see
[Throttling](#throttling)): use it before a sensitive change of your own.

## Admin sessions

- The cookie (`auth.cookie_name`, default `rustcms_session`; `HttpOnly`,
  `SameSite=Lax`, `Path=<admin_path>`) holds 43 random characters. It is
  `Secure` when `auth.secure_cookies` is `true`, or, when that key is unset,
  when `APP_URL` starts with `https://`. The database stores only an
  HMAC-SHA256 of the value keyed with `APP_KEY`: rotating the key
  (`key:generate --force`) signs every admin out (API tokens are kept).
- The cookie has no `Max-Age`: it lasts until the browser closes, and expiry is
  enforced by the server. **Remember me** (the sign-in page's box, `remember`
  in `POST auth/login`, shown while `remember_days` > 0) gives the cookie a
  `Max-Age` of `remember_days` (30, at most 400 days are used) from sign-in,
  and the session that idle lifetime instead of `session_lifetime_minutes`; every revocation below still ends it at once. The cookie is sent
  only when its value changes (sign-in, re-issue, sign-out), so a slow
  response can never restore a replaced cookie.
- The session id is regenerated at sign-in and whenever credentials change.
- Sessions without Remember me expire after `session_lifetime_minutes` (120)
  of inactivity.
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
  first, so a sign-out and concurrent re-issues, however many, never leave a
  live session, in any order. Signing in starts a new lineage.
- Every request reloads the user:
  - a disabled or deleted user's session is **ended**;
  - when the user's `auth_version` changed (their password changed, they
    signed out everywhere, an administrator reset their password), the request
    is **anonymous** and the session is **frozen**: not renewed, no cookie
    sent, left to expire. It is not ended at that point, because a concurrent
    request of the same browser may be re-issuing it for the new version (the
    password change that browser just made). For the user, the effect is the
    same: they are signed out on that browser.
- If another request ends the session while one is in flight, the in-flight
  response is kept and the session stays ended (it is never re-created).
- Every admin response carries `x-rustcms-user: <id>` while signed in, so the
  panel notices when the cookie changed accounts in another tab. Admin API
  responses, and every response that sets the cookie, carry
  `Cache-Control: private, no-store`.

### Clean-up

Expired sessions, ended JWT sign-ins, expired reset and invitation links and
audit entries older than `audit.retention_days` are deleted once an hour.
`session:prune` deletes expired sessions and ended JWT sign-ins on demand;
`idempotency:prune` does the same for idempotency receipts of admin writes.

::: warning Where the hourly clean-up runs
The hourly clean-up is the framework schedule `rustcms.auth-prune`
([Scheduling](../digging-deeper/scheduling.md#framework-schedules)): only
`serve` runs it, with `[schedule] enabled` (the default), once per hour across
instances. `queue:work`, `db:seed`, your own commands and tests never prune.
It shows on the Schedules page and in `schedule:list`. Without `serve`, run
`schedule:run rustcms.auth-prune` (or `session:prune` and `audit:prune`) from
cron.
:::

## Password resets and invitations

![The forgot-password page](/img/security/forgot-password.webp)
*RBlog, a blog built on RustCMS: the forgot-password page.*

**Forgot password?** With `auth.password_reset = true`, the sign-in page links
to `<admin>/forgot-password`. Boot fails when it is on without a mail driver
(`rustcms::mail::MailProvider` with a `[mail]` driver). When it is off, the
panel still serves its forgot and reset pages, but
`POST <admin>/api/auth/forgot-password` and `POST <admin>/api/auth/reset-password`
answer 404.

1. `POST auth/forgot-password` `{email}` always answers **202 `{}`**: for an
   account, an unknown address, a disabled or invited one, after the same
   single insert (a queued job), so neither the answer nor its timing tells
   which. Throttled per IP (`reset_requests_per_ip`, 5/min), per address
   (`reset_requests_per_email`, 3/min) and in total (300/min).
2. The job (`auth.password_reset`) finds the account, makes the token **under
   the user's row lock** and mails the link at once: the token never sits in
   the queue. A request older than 15 minutes sends nothing; a newer request
   replaces the link of an older one; a second request within
   `reset_cooldown_secs` (60) sends nothing (a retry of the same request does).
3. The link is `<APP_URL><admin>/reset-password#token=<id>.<secret>`: the
   secret is in the **fragment**, which browsers never send to the server nor
   put in a `Referer`; the page reads it, removes it from the address bar, and
   is served with `Referrer-Policy: no-referrer`.
4. `POST auth/reset-password` `{token, password, password_confirmation}`: the
   password rules first, then the token (one answer, `token: is invalid or has
   expired`, whatever is wrong). Under the user's lock then the token's,
   everything is checked again (the secret, its purpose, expiry
   (`password_reset_minutes`, 60), the same email and credential version, the
   account enabled), and the new password, the end of every token of the user
   and the revocation of their sessions (and API tokens with
   `revoke_tokens_on_password_change`) commit together. 204: sign in with it.

Only an HMAC of the secret is stored (keyed from `APP_KEY`: rotating the key
kills every link). At most one link lives per user and purpose; any credential
change (password, email, disabled, signed out everywhere) kills it.

**Invitations** (see [Users](../admin/users-roles.md)) use the same machinery:
`<admin>/invitation#token=…`, `POST auth/invitation` `{token, password,
password_confirmation, name?}`, valid `invitation_days` (7, at most 30; boot
fails outside 1–30). Once a link is used, or an administrator sets the
password, any older request still waiting in the queue sends nothing. An
invited user cannot sign in until they accept (no password matches theirs),
and does not count as the "last active super admin". Invitations work whether
`password_reset` is on or not; they need a mail driver to be sent.

The mails use the templates `auth.password_reset` and `auth.invitation`
(English and Vietnamese ship with the framework;
[Mail](../digging-deeper/mail.md)).

## CSRF

State-changing admin requests (anything but GET, HEAD and OPTIONS), including
sign-in and sign-out, must:

1. come from an allowed origin: `Origin` (or, without it, `Referer`) must equal
   one of `auth.admin_origins` (default: the origin of `APP_URL`); otherwise
   403;
2. carry `X-CSRF-Token` (`rustcms::auth::CSRF_HEADER`) equal to the session's
   token, from `GET <admin>/api/auth/csrf`; otherwise 403 `csrf_mismatch`. The
   token rotates at sign-in.

| Endpoint (`<admin>/api/auth/…`) | Does |
|---|---|
| `GET csrf` | starts a session if needed; returns `{csrfToken, userId}` |
| `POST login` | `{email, password, remember?}`: signs in, returns what `me` returns |
| `POST logout` | ends the session (204) |
| `GET me` | the user (`id`, `name`, `email`), guard, permissions, `superAdmin`, interface language |
| `POST locale` | `{locale}`: saves the user's interface language (`null`: follow the browser) |
| `POST password` | `{current_password, password, password_confirmation}`: changes the password |
| `POST logout-others` | `{password}`: ends every other session, revokes every token |
| `POST forgot-password` | `{email}`: always 202 (with `password_reset`) |
| `POST reset-password` | `{token, password, password_confirmation}` (with `password_reset`) |
| `POST invitation` | `{token, password, password_confirmation, name?}` |

Any other path under `<admin>/api` answers a JSON 404, inside the session
guard, for every method. The panel's static files (its JavaScript and CSS)
skip the guard: no session is loaded for them.

Passwords are at least `auth.password_min_length` (8) characters and at most
1,024 bytes; emails at most 255 bytes (UTF-8)
(`rustcms::auth::routes::{MAX_EMAIL_LEN, MAX_PASSWORD_LEN}`).

![The Account page with the password form and Sign out everywhere](/img/security/account.webp)
*RBlog, a blog built on RustCMS: the Account page.*

## API tokens

How clients obtain and use tokens is in [API authentication](../api/authentication.md).
`POST /api/v1/auth/token` with `{email, password, token_name, abilities?, expires_in_days?}`
returns the token once. `DELETE /api/v1/auth/token` revokes the token used;
`GET /api/v1/auth/me` describes the caller. Operators use `rustcms token:create`.

A token acts with the **intersection** of its abilities and the user's
permissions in the `api` guard. `["*"]` means all of the user's permissions.
Even a super admin's token is limited to its abilities. Tokens start with
`rcms_`; only a hash is stored.

## JWT sign-ins

`POST /api/v1/auth/jwt` with `{email, password, abilities?}` returns an access
token (a JWT, `auth.jwt.access_ttl_secs`, default 15 minutes) and a refresh
token (`rcmsr_…`). `POST /api/v1/auth/refresh {refreshToken}` returns the next
pair; each refresh token works **once**: using a spent one again (a replay,
two tabs racing, or a retry after a lost response) ends the whole sign-in, so
clients must refresh one at a time and keep the newest token.
`POST /api/v1/auth/jwt/logout {refreshToken}` ends the sign-in (also after the
access token expired). Send refresh and logout **without** an `Authorization`
header: the refresh token in the body authenticates them, and an expired or
revoked access token in the header would make the request a 401 first. With
`auth.jwt.enabled = false`, the three endpoints
answer 404 and bearer JWTs are refused (401).

Access tokens are HS256 with a key derived from `APP_KEY` (rotating it ends
every sign-in); only that exact header is accepted. Each request still checks
the user (active, same `auth_version`) and the sign-in (not ended), so
revocation applies from the next request, as for sessions. Abilities are fixed
per sign-in and narrow the user's API permissions like a token's.

| Key (`[auth.jwt]`) | Default | Bounds (boot fails outside) |
|---|---|---|
| `enabled` | `true` |
| `access_ttl_secs` | `900` | 60–3,600 |
| `refresh_ttl_days` | `14` | 1–90 |
| `refresh_family_days` | `30` | `refresh_ttl_days`–365 |

## Throttling

| What | Limit (per minute) |
|---|---|
| Sign-in: admin `login`, `POST /api/v1/auth/token`, `POST /api/v1/auth/jwt` | `attempts_per_identifier` (5) per account and IP; `attempts_per_ip` (30) per IP |
| Password confirmation (`password`, `logout-others`, `confirm_password`) | `attempts_per_identifier` (5) per user |
| JWT sign-ins | 20 per user, successes included |
| JWT refresh and logout | 60 per IP, then 30 per sign-in |
| New anonymous sessions (`csrf`) | `anonymous_sessions_per_ip` (30) per IP; `anonymous_sessions_per_minute` (1,000) in total |
| `forgot-password` | `reset_requests_per_ip` (5) per IP; `reset_requests_per_email` (3) per address; 300 in total |
| `reset-password`, `invitation` | 10 per IP, 300 in total; 5 per token once its secret matched |

Over a limit: 429 with `Retry-After`. Sign-in attempts are counted *before* the
password is verified, so a burst of concurrent guesses cannot exceed the
limit, and a request cancelled mid-check stays counted; a successful sign-in
clears the account counter and refunds the IP. The IP comes from the
[trusted-proxy rules](../architecture/lifecycle.md#client-ip-and-proxies).
Unknown emails cost the same Argon2 time as wrong passwords and return the
same message. Argon2id hashing runs on the blocking pool, at most one hash per
CPU at a time.

Emails longer than 255 bytes and passwords longer than 1,024 bytes are
rejected before any throttling state or hash is built. Throttling keys are
stored as fixed-size hashes; expired windows are reclaimed and the table is
capped (100,000 keys, oldest dropped first), so unauthenticated traffic cannot
grow memory without bound.

Limits are kept in memory, per instance; with Redis (`[redis] url`, feature
`redis`) every instance counts in Redis, atomically across all the keys of one
check. If Redis does not answer, checks fail closed (500). See
[Several instances](../digging-deeper/instances.md).

Failed sign-ins are recorded in the [audit log](../digging-deeper/audit-log.md#failed-sign-ins)
(`auth.sign_in_failed`), one by one up to `audit.sign_in_failures_per_minute`
(600) per instance, then as one summary entry per minute.

## Configuration

Every key of `[auth]` (`config/auth.toml`, `RUSTCMS_AUTH__*`) has a default; the
full table is in [Configuration](../getting-started/configuration.md). The
ones this page uses:

| Key | Default |
|---|---|
| `admin_path` | `/admin` (not `/`, not under `/api`; boot fails otherwise) |
| `admin_origins` | `[]` (the origin of `APP_URL`) |
| `cookie_name` | `rustcms_session` |
| `secure_cookies` | unset (`true` when `APP_URL` is https) |
| `session_lifetime_minutes` | `120` |
| `remember_days` | `30` (`0`: no Remember me) |
| `anonymous_session_lifetime_minutes` | `15` |
| `password_min_length` | `8` |
| `revoke_tokens_on_password_change` | `true` |
| `password_reset` | `false` |
| `password_reset_minutes` | `60` |
| `invitation_days` | `7` |

## Security notes

- Never set `auth.secure_cookies = false` on a site served over https: the
  session cookie would then also travel over plain http.
- Turn `auth.jwt.enabled` off when no client uses JWT sign-ins.
- What ends when (sign-out, password change, disabled user, key rotation) is
  in [Credential revocation](revocation.md).
- Deploying: [Production checklist](production.md).

## Related pages

- [API authentication](../api/authentication.md)
- [Roles & permissions](authorization.md)
- [Credential revocation](revocation.md)
- [Users, roles & your account](../admin/users-roles.md)
- [Admin API](../reference/admin-api.md)
