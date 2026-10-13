# Credential revocation

The contract below is tested row by row (`credential_revocation_table`; the
`APP_KEY` row by `rotating_the_app_key_signs_everyone_out`).

| Event                          | Current session  | Other sessions | API tokens                     |
|--------------------------------|------------------|----------------|--------------------------------|
| Sign out                       | ended            | kept           | kept                           |
| Sign out everywhere (`logout-others`) | kept (new id) | ended     | **all revoked**                |
| Password change                | kept (new id)    | ended          | all revoked (`revoke_tokens_on_password_change`) |
| User disabled (`RbacService::set_user_active`) | ended | ended   | all revoked                    |
| User deleted                   | ended            | ended          | deleted                        |
| One token revoked              | —                | —              | that token                     |
| `APP_KEY` rotated              | ended            | ended          | kept                           |
| Roles or permissions changed   | kept             | kept           | kept; new permissions apply on the next request |

Mechanism: account changes bump `users.auth_version` in the same transaction;
sessions store the version they were issued for and are checked on every
request. Resolved permissions are cached for at most
`permission_cache_ttl_secs` and evicted immediately on any RBAC change, so on a
single instance the delay is zero.

## Races

- Password changes, sign-out everywhere and token issuance re-check, inside a
  write transaction holding the user row, that the credentials verified earlier
  in the request are unchanged (password hash, `auth_version`, active). A token
  obtained with a password that is changed concurrently is refused, never issued
  after the revocation.
- The permission cache is keyed by a generation advanced on every RBAC change:
  a lookup that started before a change cannot repopulate the cache with the
  old permissions.
- A request whose session was revoked can start a new one (CSRF, sign-in) in
  the same response.
