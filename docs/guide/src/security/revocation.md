# Credential revocation

What stops working when, for every kind of credential: admin sessions, API
tokens and JWT sign-ins. Read it when you decide how to react to a leaked
password or a departing user, or when you build a credential flow of your own.

## What ends when

The columns are the affected user's credentials. "Signed out" means the next
request with that credential is anonymous (sessions) or refused with 401
(tokens, JWT).

| Event | Current session | Other sessions | API tokens (`rcms_…`) | JWT sign-ins (access + refresh) |
|---|---|---|---|---|
| Sign out | ended | kept | kept | kept |
| Sign out everywhere (`logout-others`) | kept (new id) | signed out | **all revoked** | **all ended** |
| Password change | kept (new id) | signed out | all revoked (`revoke_tokens_on_password_change`) | all ended (always) |
| User disabled (`RbacService::set_user_active`) | ended | ended | all revoked | all ended |
| Email changed or password reset by an admin | signed out | signed out | all revoked | all ended |
| Password reset by link / invitation accepted | — | signed out | all revoked (`revoke_tokens_on_password_change`) | all ended |
| User deleted | ended | ended | deleted | deleted |
| One token revoked (`DELETE /api/v1/auth/token`) | — | — | that token | kept |
| JWT logout (`POST /api/v1/auth/jwt/logout`) | — | — | kept | that sign-in |
| A spent refresh token used again | — | — | kept | that sign-in |
| `APP_KEY` rotated | ended | ended | kept | all ended |
| Roles or permissions changed | kept | kept | kept; new permissions apply on the next request | kept; new permissions apply on the next request |

Every row is covered by the framework's tests
(`crates/rustcms-auth/tests/`: `auth_flow.rs`, `jwt.rs`, `recovery.rs`,
`rbac.rs`, `instances.rs`).

## How it works

Account changes bump `users.auth_version` in the same transaction as the
change. Sessions and JWT sign-ins store the version they were issued for, and
every request checks it:

- A session of a **disabled or deleted** user is ended by the request that
  finds it.
- A session whose version is **old** makes the request anonymous and is
  frozen: never renewed, no cookie sent, left to expire. It is not deleted at
  that point, because the same browser may be re-issuing it for the new
  version in a concurrent request (the browser that changed the password keeps
  its session, with a new id).
- An access token (JWT) also checks that its sign-in is not revoked or
  expired; the credential change marks every sign-in of the user revoked.

`revoke_tokens_on_password_change` (default `true`) applies to API tokens
only: JWT sign-ins always end, like sessions.

Resolved permissions are cached for at most `permission_cache_ttl_secs` (30),
keyed by a counter (`auth_generations.rbac`) that every RBAC change advances in
its own transaction and every request reads: on any number of instances, a
committed change applies from the next request.

Session ids and refresh tokens are keyed from `APP_KEY` only, not from
`APP_PREVIOUS_KEYS`: rotating the key ends them whatever the previous keys
hold ([Rotating APP_KEY](../operations/app-key.md)).

Every admin response carries `x-rustcms-user: <id>` while signed in, so the
panel can tell when the cookie changed accounts in another tab and never shows
one user's page to another.

## Races

- Password changes, sign-out everywhere and token issuance re-check, inside a
  write transaction holding the user row, that the credentials verified earlier
  in the request are unchanged (password hash, `auth_version`, active). A token
  obtained with a password that is changed concurrently is refused, never issued
  after the revocation.
- The permission cache is keyed by a generation advanced on every RBAC change:
  a lookup that started before a change cannot repopulate the cache with the
  old permissions.
- JWT refresh tokens rotate under the locks of the user, the sign-in and the
  token, in that order (the same order as issuance and revocation). Two
  refreshes with one token: one wins, the other counts as a replay and ends
  the sign-in.
- A request whose session was revoked can start a new one (CSRF, sign-in) in
  the same response.
- A request already running when a credential is revoked finishes with the
  credentials checked when it started. Changes made through `RbacService`
  re-read the actor under lock and are refused (403) when the account's
  `auth_version` changed (password change, sign-out everywhere, admin reset),
  or the account was disabled or deleted. Revoking only one token, one JWT
  sign-in or one session does not change the account's version, so it does
  not stop an RBAC change that request already authenticated.

## Building your own credential flow

`rustcms::auth::credentials` holds the primitives the built-in endpoints use.
Use them when you add a flow of your own (an SSO callback that must end other
sessions, an admin tool that revokes a user's tokens):

| Function | Does |
|---|---|
| `lock_unchanged(&txn, &snapshot)` | reloads the user under a row lock; 401 unless the password hash, `auth_version` and active flag still match `snapshot` |
| `revoke_all(&txn, user_id, tokens)` | bumps `auth_version`, ends every JWT sign-in and, with `tokens`, revokes every API token; returns the new version |
| `revoke_tokens(&txn, user_id)` | revokes every API token of the user |
| `change_password(db, &snapshot, new_hash, revoke_api_tokens, &audit)` | sets the password and revokes the other credentials, in one transaction; returns the new version |
| `logout_everywhere(db, &snapshot, &audit)` | ends every session, sign-in and token; returns the new version |
| `issue_token(db, &snapshot, name, &abilities, expires_at, &request)` | issues an API token, unless the credentials changed since they were verified |

Call `revoke_all` and `revoke_tokens` inside the transaction that changes the
account, so the revocation commits or rolls back with it. After
`change_password` or `logout_everywhere` in an admin request, keep the
caller's session signed in with `session.restamp(new_version)` (the `Session`
extractor); otherwise that request's own session is signed out too.

Revocations are recorded in the [audit log](../digging-deeper/audit-log.md):
`auth.password_changed`, `auth.signed_out_everywhere`, `token.revoked`,
`token.created`, and the `user.*` entries of `RbacService`.

## Related pages

- [Authentication](authentication.md)
- [API authentication](../api/authentication.md)
- [Roles & permissions](authorization.md)
- [Rotating APP_KEY](../operations/app-key.md)
