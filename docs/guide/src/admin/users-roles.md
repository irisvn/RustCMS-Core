# Users, roles & your account

The panel has built-in **System** pages for the admin guard: **Users**, shown
to users with `users.view_any`, and **Roles**, shown with `roles.view_any`.
Every user also has an **Account** page (click your name). Use them to invite
people, give them roles and manage what each role may do, without writing
code.

![The Users list with an Invited badge](/img/admin/users-list.webp)
*A RustCMS demo application: the Users page.*

## Quick start

Create the first super admin from the command line, then sign in and manage
everyone else in the panel:

```sh
rustcms make:user --email admin@example.com --name "Admin" --super-admin   # prints a generated password
rustcms role:assign --email editor@example.com --role editor                # an existing user, an existing role
```

| Command | Options |
|---|---|
| `make:user` | `--email`, `--name` (required), `--password` (default: generated and printed), `--super-admin`, `--guard` (of the super admin role, default `admin`) |
| `role:assign` | `--email`, `--role` (required), `--guard` (default `admin`) |

## Full example

An application decides how its permissions appear on the Roles page: areas,
labels and presets. The demo:

<<< @/../../../examples/demo/src/providers/auth_service_provider.rs#auth-provider [src/providers/auth_service_provider.rs]

See [The Roles page](../security/authorization.md#the-roles-page-areas-labels-and-presets)
for these builders.

## What each action needs

Every change goes through `RbacService`, the same service the CLI and your
own code use, so the same rules apply everywhere:

| Action | Needs | Also |
|---|---|---|
| List / view users | `users.view_any` / `users.view` | your own profile is always visible |
| Create a user | `users.create` | with roles: `roles.assign` too; all or nothing |
| Invite a user | `users.create` + `users.invite` | needs mail; they choose their password from the mail (valid `auth.invitation_days`); pending until then |
| Send an invitation again | `users.invite` + `users.view` | while pending and enabled; the old link stops working when the queued mail job creates the new one |
| Rename a user | `users.update` | you may rename yourself |
| Change an email | someone else's: `users.update`, a manageable account, and super admin of every guard; yours: your current password | the address becomes unverified and the account's other sessions and tokens end (yours continues) |
| Disable / enable, delete | `users.update` / `users.delete` | not yourself; the last active super admin stays |
| Set a password | super admin of every guard (+ `users.update`) | not yourself (use Account); ends their sessions and tokens |
| Assign / remove roles | `roles.assign` (+ `users.view` or `users.view_any` to compare with the current roles) | not your own roles; protected roles by super admins; only roles whose active permissions you hold |
| Create / edit a role | `roles.create` / `roles.update` (+ `roles.view` or `roles.view_any` to compare with the current permissions) | only permissions you hold; never a role you hold; names unique ignoring case, at most 120 characters |
| Delete a role | `roles.delete` | not system roles; not a role you hold; protected roles by super admins; you hold all its permissions |

**Protected accounts.** Another user holding a protected role (system or
privileged) or a super-admin role in *any* guard is disabled, renamed,
re-addressed or deleted only by super admins of every guard. Your own
profile is not affected: you may rename yourself, and change your own email
with your current password.

The panel always sends what the editor started from (`expected`, see
[API](#api)), which needs the read permission above. Code calling
`RbacService` without an expected value skips that comparison and that
permission.

Setting someone's password or email is taking over their account. Equal
permissions do not mean equal access (policies decide on the user, e.g. who
owns a record), and the same credentials also issue API tokens, so only a
super admin of **every** guard (admin and API) may do it. Others can still
rename and disable users.

### Built-in permissions

The framework declares these in the `admin` guard (`BUILT_IN_PERMISSIONS`):

| Group | Permissions |
|---|---|
| `users` | `users.view_any`, `users.view`, `users.create`, `users.update`, `users.delete`, `users.invite` |
| `roles` | `roles.view_any`, `roles.view`, `roles.create`, `roles.update`, `roles.delete`, `roles.assign` |
| `permissions` | `permissions.view_any`, `permissions.assign` |

`permissions.assign` is used only by `RbacService::grant_permission` and
`revoke_permission` (a permission given to one user directly); the panel has
no screen for it.

## Passwords

A password is at least `auth.password_min_length` (8) characters and at most
1,024 bytes. Creating users and setting passwords hash on the server: both
are checked first (input, duplicates, authorization) and limited per caller
(creation within `auth.attempts_per_ip`, resets within
`auth.attempts_per_identifier` per minute).

## Invitations

![The New user dialog with Send an invitation instead](/img/admin/users-invite.webp)
*A RustCMS demo application: inviting a user.*

On **New user**, *Send an invitation instead of setting a password* creates
the user pending (badge *Invited*) and mails them a link; the user page
offers *Send the invitation again* until they accept. Setting their password
as an administrator ends the invitation. To cancel one, disable or delete
the user.

- A link works for `auth.invitation_days` (default 7, at most 30).
- With mail disabled, inviting answers 409 `mail_disabled`.
- Sending again answers 409 `invitation_accepted` once accepted, and
  `user_inactive` for a disabled user.
- At most 20 invitations a minute per administrator, and 5 resends.

See [Authentication](../security/authentication.md) and
[Mail](../digging-deeper/mail.md) for the mails and links.

## The role page

![A role: areas, the resource matrix and presets](/img/admin/roles-matrix.webp)
*A RustCMS demo application: editing a role.*

The role page shows permissions in collapsible **areas** with a
selected/total count, a search box and the presets the application declares.
Inside an area, the permissions of resources come first as a matrix: a row
per resource, a column per ability (list, view, create, update, delete,
restore, delete for good), the resource's other abilities (actions,
`attach_tags`…) in its last column, and a box to switch a whole row or
column. The rest is listed by group, with a box for the whole group. Boxes of
permissions you do not hold are disabled, and saving keeps them as they are.

## Your account

![The Account page: language, password and other sessions](/img/admin/account.webp)
*A RustCMS demo application: the Account page.*

The **Account** page:

- sets your [language](i18n.md);
- changes your password (`POST {admin}/api/auth/password`
  `{current_password, password, password_confirmation}`): your other
  sessions end, and your API tokens are revoked while
  `auth.revoke_tokens_on_password_change` is on (the default);
- signs out your other sessions (`POST {admin}/api/auth/logout-others`
  `{password}`, your current password required), also revoking your API
  tokens.

## API

Under `{admin}/api/system` (same session, CSRF and `no-store` rules as
the rest of the admin API):

| Method | Path | Body | Response |
|---|---|---|---|
| GET | `/users?search=&page=&per_page=` | | `UserPage` |
| POST | `/users` | `{name, email, password, roleIds?}` | 201 `SystemUser`* |
| POST | `/users/invitations` | `{name, email, roleIds?}` (`users.create` + `users.invite`, mail on; 20 per minute) | 201 `SystemUser`* |
| POST | `/users/{id}/invitation` | (`users.invite` + `users.view`; 5 per minute) | `SystemUser`* |
| GET / PATCH / DELETE | `/users/{id}` | PATCH: `{name, email, expected: {name, email}, currentPassword?}` | `SystemUser` / `SystemUser`* / 204 |
| PUT | `/users/{id}/roles` | `{roleIds, expectedRoleIds}` | `SystemUser`* |
| PUT | `/users/{id}/active` | `{active, expected}` | `SystemUser`* |
| PUT | `/users/{id}/password` | `{password}` | 204 |
| GET / POST | `/roles` | POST: `{name, permissions}` | `[SystemRole]` / 201 `SystemRole` |
| GET / DELETE | `/roles/{id}` | | `SystemRole` / 204 |
| PUT | `/roles/{id}/permissions` | `{permissions, expectedPermissions}` | `SystemRole` |
| GET | `/permissions` | | `[PermissionInfo]`: `name`, `group`, `groupLabel`, `grantable`; `resource` and `ability` when it is exactly one resource's `{slug}.{ability}`; `area`, `areaLabel`, `areaSort` always; `label` and `description` when declared |
| GET | `/permission-presets` | | `[PermissionPresetInfo]` `{id, label, description?, permissions}` (declared, not orphaned names, also those the caller may not grant); `roles.view_any` or `permissions.view_any`, as `/permissions` |

\* 204 without a body when the caller may not read the user (no `users.view`), or
when the user was deleted right after the change.

- `GET /users`: `per_page` defaults to 25 (1–100); `search` matches names
  and emails, at most 100 characters. `GET /users/{id}` works on yourself
  without `users.view`.
- `SystemUser` includes `invitationPending`, `invitedAt`, `superAdmin` and
  `isSelf`. Its `abilities` are `update`, `changeEmail`, `delete`,
  `assignRoles`, `activate`, `resetPassword` and `resendInvitation`;
  `SystemRole.abilities` are `update`, `delete` and `assign`. They say what
  the current user may do; the server checks again on every request.
- `GET /roles` answers role names to holders of `roles.assign`; permissions
  and user counts need `roles.view_any`. Lists of ids or names accept at
  most 1,000 items.
- Replacing roles or permissions sends what the editor started from
  (`expected…`): if someone changed them meanwhile, the request is refused
  with a 422 and nothing is overwritten. Profile saves send the values the
  editor started from (`expected`) and are refused the same way. Orphaned
  permissions (no longer declared by the application) are not shown and are
  kept as they are.

## Known limits

- Signing out one session, or revoking one API token, does not abort a
  change that the same credential already started (changes check the
  account's credential version, which sign-out everywhere and password
  changes bump).
- Resource writes in flight are not re-checked against a revocation that
  happens during them.

## Security notes

- Never write role or permission tables directly: use `RbacService`, which
  enforces the rules above, locks consistently and writes the audit log.
- Nobody changes their own access: no assigning your own roles, editing or
  deleting a role you hold, or granting a permission you do not hold.

## Related pages

- [Roles & permissions](../security/authorization.md): permissions, guards, the Roles page catalog
- [Authentication](../security/authentication.md): sessions, invitations, password resets
- [Languages](i18n.md)
- [Configuration](../getting-started/configuration.md): `[auth]` keys
