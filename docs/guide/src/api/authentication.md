# API authentication

Everything outside the admin path uses the **API guard**. A client sends a
credential in the `Authorization` header: a long-lived **API token** (scripts,
other services, CI) or a short **JWT access token** from a sign-in (mobile and
browser apps). Either way, a request may do what both the credential's
**abilities** and the user's API-guard permissions allow.

## Quick start

The user needs API-guard permissions first (roles of the `api` guard; admin
roles grant nothing here). Then issue a token:

```sh
rustcms role:assign --email you@example.com --role super_admin --guard api   # or a role you created
rustcms token:create --email you@example.com --name deploy --abilities posts.view_any --expires-days 30
```

`token:create` prints the token once. Send it as a bearer token:

```sh
curl -H "Authorization: Bearer rcms_12_…" https://api.example.com/api/v1/posts
```

## Abilities and permissions

A token carries abilities: names of permissions, or `*` for all of them. A
request passes `Access::can("posts.view_any")` when the user holds
`posts.view_any` in the `api` guard (or is its super admin) **and** the token's
abilities list it (or `*`). Even a super admin's token is limited to its
abilities.

The demo's test shows the guards apart and abilities narrowing a token:

<<< @/../../../examples/demo/tests/feature/api.rs#api-access [tests/feature/api.rs]

Roles for the API guard are made in code ([Roles & permissions](../security/authorization.md#changing-access-rbacservice));
the Roles page manages admin-guard roles.

## API tokens

| | Command line | HTTP |
|---|---|---|
| Issue | `rustcms token:create --email --name [--abilities a,b] [--expires-days N]` | `POST /api/v1/auth/token` |
| Revoke | (revoke all: disable the user, or sign out everywhere) | `DELETE /api/v1/auth/token` with the token |
| Describe | | `GET /api/v1/auth/me` |

`token:create --abilities` defaults to `*`.

### `POST /api/v1/auth/token`

```json
{ "email": "you@example.com", "password": "…", "token_name": "deploy",
  "abilities": ["posts.view_any"], "expires_in_days": 30 }
```

| Field | Rules |
|---|---|
| `email`, `password` | the user's credentials; throttled like an admin sign-in |
| `token_name` | 1–255 characters |
| `abilities` | optional: at most 100 names, each 1–255 bytes; omitted or empty means `["*"]` |
| `expires_in_days` | optional: 1–3,650; without it the token lasts until revoked |

The answer is **201**:

```json
{ "token": "rcms_12_…", "abilities": ["posts.view_any"], "expiresAt": "2026-11-27T10:00:00Z" }
```

The token is shown once; only a hash is stored. Wrong credentials, a disabled
user or an invited user who has not accepted yet get the same 422 on `email`.
If the password changes between the check and the insert, the token is not
issued (the same 422).

`DELETE /api/v1/auth/token` revokes the token the request uses (204). With a
JWT bearer it answers 401: end a JWT sign-in with `jwt/logout`.

### `GET /api/v1/auth/me`

Describes the caller (any API token or JWT):

```json
{
  "user": { "id": "12", "name": "Ada", "email": "you@example.com" },
  "guard": "api",
  "permissions": ["posts.create", "posts.view_any"],
  "superAdmin": false,
  "abilities": ["posts.view_any"],
  "locale": "en",
  "preferredLocale": null,
  "locales": ["en", "vi"]
}
```

`permissions` are the user's API-guard permissions; `abilities` the
credential's. The user's id is a string ([Wire format](../reference/wire-format.md)).

## JWT sign-ins

Apps that sign users in use short access tokens with rotating refresh tokens
(`auth.jwt.enabled`, on by default):

```text
POST /api/v1/auth/jwt          {email, password, abilities?}   → 201
POST /api/v1/auth/refresh      {refreshToken}                   → 200
POST /api/v1/auth/jwt/logout   {refreshToken}                   → 204
```

Sign-in and refresh answer:

```json
{
  "accessToken": "eyJ…",
  "tokenType": "Bearer",
  "expiresIn": 900,
  "refreshToken": "rcmsr_34_…",
  "refreshExpiresAt": "2026-11-11T10:00:00Z",
  "abilities": ["*"]
}
```

Send `Authorization: Bearer <accessToken>` like an API token. Do **not**
send it on `refresh` and `jwt/logout`: the refresh token in the body
authenticates them, and any `Authorization` header is checked first, so an
expired or revoked access token there turns the call into a 401.

- **Abilities** are fixed for the sign-in and narrow the user's API
  permissions. Omitted: `*`. Given: 1–100 names, each 1–255 bytes, 2,048 bytes
  in all (they ride in every access token); an empty list is refused (422).
- The request bodies accept no other fields (422 on an unknown one).
- Refresh before `expiresIn` (seconds) runs out, with the **latest** refresh
  token. Each refresh token works once: a second use ends the sign-in (both
  tokens stop working). Refresh one request at a time (share one in-flight
  refresh across tabs); if a refresh response is lost, sign in again.
- `jwt/logout` ends the sign-in, also after the access token expired. It
  answers 204 again for a sign-in that already ended, as long as its records
  still exist; ended sign-ins are deleted by the hourly clean-up, after which
  the same refresh token gets 401.
- With `auth.jwt.enabled = false`, the three endpoints answer 404 and bearer
  JWTs are refused (401).

| Key (`[auth.jwt]`) | Default | Means |
|---|---|---|
| `access_ttl_secs` | `900` | access token lifetime (60–3,600) |
| `refresh_ttl_days` | `14` | a refresh token unused this long stops working (1–90) |
| `refresh_family_days` | `30` | a sign-in ends after this long, however often it is refreshed (`refresh_ttl_days`–365) |

Access tokens are at most 4,096 bytes, refresh tokens at most 80; longer ones
are refused (401).

## Limits

| What | Limit per minute |
|---|---|
| `POST auth/token`, `POST auth/jwt` | `auth.attempts_per_identifier` (5) per account and IP, `auth.attempts_per_ip` (30) per IP, counted before the password is checked |
| `POST auth/jwt` | also 20 per user, successes included |
| `POST auth/refresh`, `POST auth/jwt/logout` | 60 per IP, then 30 per sign-in |

Over a limit: 429 with `Retry-After` (seconds). Your own routes can limit a
user's requests with [`Throttle<N>`](../basics/validation.md#rate-limiting).
Error bodies (401, 403, 422, 429) follow the [error format](../basics/errors.md).

## Revocation

Password change, sign-out everywhere and disabling the user end every JWT
sign-in, and revoke API tokens (for a password change, unless
`auth.revoke_tokens_on_password_change = false`), from the next request. A
request already running is not aborted: your endpoints, like admin resource
writes, finish with the credentials checked when they started. Account and
RBAC changes made through `RbacService` are the exception when the whole
account was revoked (password change, sign-out everywhere, disabled or deleted
user): they re-read the account under lock and are refused. Revoking one token
or one JWT sign-in does not cancel such a change already under way. See
[Credential revocation](../security/revocation.md).

## Security notes

- Give each client its own token with the fewest abilities it needs, and an
  expiry.
- Tokens and refresh tokens are secrets: never log them or put them in URLs.
- An invalid credential is a 401 on every API route, public routes included:
  a client must not send a stale token "just in case".

## Related pages

- [Routing](../basics/routing.md) (declaring the access of your routes)
- [Authentication](../security/authentication.md)
- [Roles & permissions](../security/authorization.md)
- [Credential revocation](../security/revocation.md)
