# Credential revocation

What ends when, for every kind of credential. Every row is covered by the
framework's test suite.

| Event                          | Current session  | Other sessions | API tokens (`rcms_…`)          | JWT sign-ins (access + refresh) |
|--------------------------------|------------------|----------------|--------------------------------|---------------------------------|
| Sign out                       | ended            | kept           | kept                           | kept                            |
| Sign out everywhere (`logout-others`) | kept (new id) | ended     | **all revoked**                | **all ended**                   |
| Password change                | kept (new id)    | ended          | all revoked (`revoke_tokens_on_password_change`) | all ended (always) |
| User disabled (`RbacService::set_user_active`) | ended | ended   | all revoked                    | all ended                       |
| Email changed or password reset by an admin | ended | ended     | all revoked                    | all ended                       |
| User deleted                   | ended            | ended          | deleted                        | deleted                         |
| One token revoked              | —                | —              | that token                     | kept                            |
| JWT logout (`POST /api/v1/auth/jwt/logout`) | — | —            | kept                           | that sign-in                    |
| A spent refresh token used again | —              | —              | kept                           | that sign-in                    |
| `APP_KEY` rotated              | ended            | ended          | kept                           | all ended                       |
| Roles or permissions changed   | kept             | kept           | kept; new permissions apply on the next request | kept; new permissions apply on the next request |

Mechanism: account changes bump `users.auth_version` in the same transaction;
sessions and JWT sign-ins store the version they were issued for and are
checked on every request (an access token also checks that its sign-in is
not revoked or expired). `revoke_tokens_on_password_change` applies to API
tokens only: JWT sign-ins behave like sessions. Resolved permissions are cached
for at most `permission_cache_ttl_secs`, keyed by a counter
(`auth_generations.rbac`) that every RBAC change advances in its own
transaction and every request reads: on any number of instances, a committed
change applies from the next request.

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
