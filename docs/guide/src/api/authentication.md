# API authentication

Everything outside the admin path uses the **API guard**: requests carry a
personal access token, `Authorization: Bearer rcms_<id>_<secret>`. Tokens are
stored hashed, can expire, and carry **abilities**: a request may do what both
the token's abilities and the user's API-guard permissions allow (`*` = all).

The user needs API-guard permissions first (roles of the `api` guard):

```sh
rustcms role:assign --email you@example.com --role super_admin --guard api   # or a role you created (see Roles & permissions)
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
refreshToken, refreshExpiresAt, abilities}`. Send
`Authorization: Bearer <accessToken>` like an API token; abilities (omitted = `*`) are fixed for the
sign-in and narrow the user's API permissions.

- Refresh before `expiresIn` runs out, with the **latest** refresh token. Each
  refresh token works once: a second use ends the sign-in (both tokens stop
  working). Refresh one request at a time (share one in-flight refresh across
  tabs); if a refresh response is lost, sign in again.
- A refresh token lasts `refresh_ttl_days` (14) without use; a sign-in lasts at
  most `refresh_family_days` (30).
- Password change, sign-out everywhere and disabling the user end every sign-in
  from the next request (see [revocation](../security/revocation.md)).

## Token lifetimes

Token lifetimes are 1 to 3,650 days (`expires_in_days`, `--expires-days`);
without one, a token lasts until revoked.

## Revocation

Revocation (password change, sign-out everywhere, disabling) applies from the
next request. A request already running is not aborted: your endpoints, like
admin resource writes, finish with the credentials checked when they started.
Account and RBAC changes made through `RbacService` are the exception: they
re-check the credential version under lock and are refused. See
[Credential revocation](../security/revocation.md).

Building endpoints that use these credentials is covered in
[Routing](../basics/routing.md).
