# Users, roles & your account

The panel has built-in **System** pages for the admin guard, shown to users
with `users.view_any` (Users) or `roles.view_any` (Roles), and an **Account**
page for everyone (click your name).

Every change goes through `RbacService`, so the same rules apply as for the
CLI and your own code:

| Action | Needs | Also |
|---|---|---|
| List / view users | `users.view_any` / `users.view` | your own profile is always visible |
| Create a user | `users.create` | with roles: `roles.assign` too; all or nothing |
| Rename a user | `users.update` | you may rename yourself |
| Change an email | someone else's: super admin of every guard; yours: your current password | the address becomes unverified and the account's other sessions and tokens end (yours continues) |
| Disable / enable, delete | `users.update` / `users.delete` | not yourself; the last active super admin stays |
| Set a password | super admin of every guard (+ `users.update`) | not yourself (use Account); ends their sessions and tokens |
| Assign / remove roles | `roles.assign` (+ `users.view` to see current roles) | not your own roles; protected roles by super admins; only roles whose active permissions you hold |
| Create / edit a role | `roles.create` / `roles.update` (+ `roles.view` to edit) | only permissions you hold; never a role you hold; names unique ignoring case, ≤ 120 characters |
| Delete a role | `roles.delete` | not system roles; protected roles by super admins; you hold all its permissions |

**Protected accounts.** Users holding a protected role (system or privileged) or
a super-admin role in *any* guard are disabled, renamed, re-addressed or deleted
only by super admins of every guard.

The **Account** page changes your password (current password required; other
sessions end) and signs out your other sessions (also revoking API tokens).

## API

Under `<admin_path>/api/system` (same session, CSRF and `no-store` rules as
the rest of the admin API):

| Method | Path | Body | Response |
|---|---|---|---|
| GET | `/users?search=&page=&per_page=` | | `UserPage` |
| POST | `/users` | `{name, email, password, roleIds?}` | 201 `SystemUser`* |
| GET / PATCH / DELETE | `/users/{id}` | PATCH: `{name, email, expected: {name, email}, currentPassword?}` | `SystemUser` / `SystemUser`* / 204 |
| PUT | `/users/{id}/roles` | `{roleIds, expectedRoleIds}` | `SystemUser`* |
| PUT | `/users/{id}/active` | `{active, expected}` | `SystemUser`* |
| PUT | `/users/{id}/password` | `{password}` | 204 |
| GET / POST | `/roles` | POST: `{name, permissions}` | `[SystemRole]` / 201 `SystemRole` |
| GET / DELETE | `/roles/{id}` | | `SystemRole` / 204 |
| PUT | `/roles/{id}/permissions` | `{permissions, expectedPermissions}` | `SystemRole` |
| GET | `/permissions` | | `[PermissionInfo]` (with `grantable`) |

Replacing roles or permissions sends what the editor started from
(`expected…`): if someone changed them meanwhile, the request is refused with a
422 and nothing is overwritten. Orphaned permissions (no longer declared by the
application) are not shown and are kept as they are.

Setting someone's password or email is taking over their account. Equal
permissions do not mean equal access (policies decide on the user, e.g. who owns a
record), and the same credentials also issue API tokens, so only a super admin of
**every** guard (admin and API) may do it. Others can still rename and disable users.
Profile saves send the values the editor started from (`expected`) and are refused
if the profile changed meanwhile.

\* 204 without a body when the caller may not read the user (no `users.view`), or
when the user was deleted right after the change.

`GET /roles` answers role names to holders of `roles.assign`; permissions and
user counts need `roles.view_any`. Lists of ids or names accept at most 1,000 items.
Creating users and setting passwords hash on the server: both are checked first
(input, duplicates, authorization) and limited per caller.

**Known limits.** Signing out one session, or revoking one API token, does
not abort a change that the same credential already started (changes check the
account's credential version, which sign-out everywhere and password changes bump).
Resource writes in flight are not re-checked against a revocation that happens
during them.

`SystemUser.abilities` and `SystemRole.abilities` say what the current user may
do; the server checks again on every request.
