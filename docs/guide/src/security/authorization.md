# Roles & permissions

Permissions decide what a signed-in user may do. Users get them through roles
(or directly), per guard. [Route access](../basics/routing.md#access) checks
them per API route, [admin route access](../admin/admin-routes.md#access) per
panel route, and [policies](../admin/authorization.md) per record in the admin
panel. This page covers how permissions are declared, granted and checked.

![The Roles page: areas, the resource matrix and permission groups](/img/security/roles.webp)
*The RustCMS demo app (examples/demo): System → Roles.*

## Quick start

Declare the permissions your code checks on the `AuthProvider`, sync them to
the database, then grant them to a role in the panel (System → Roles) or from
code:

```rust
AuthProvider::new()
    .permission(ADMIN_GUARD, "posts.publish", "posts")  // (guard, name, group)
    .api_permissions(&api)                               // every Access::can of the API routes
    .admin_permissions(&admin)                           // every AdminAccess::can of the admin routes
```

```sh
rustcms permission:sync     # creates declared permissions; run after each deploy
```

## The model

Spatie-style tables, scoped by guard: `permissions`, `roles`,
`role_has_permissions`, `model_has_roles`, `model_has_permissions` (columns,
keys and how they relate: [Database schema](../reference/database-schema.md)).
A user may hold roles in both guards (`admin`, `api`; the constants
`rustcms::auth::{ADMIN_GUARD, API_GUARD}`); they never mix: an admin-guard role
grants nothing to an API token.

- **Deny by default.** Missing permission: 403.
- **Super admin.** The system role `super_admin` (seeded per guard by the
  migrations) passes every check *in its guard*, still bounded by token
  abilities.
- **Token abilities.** An API token or JWT sign-in carries abilities; a check
  passes only when the user's permissions **and** the abilities allow it
  (`*`: all of the user's permissions).

A request's `Principal` (`rustcms::Principal`, from `auth.principal()`) holds
what the checks use:

| Field | Holds |
|---|---|
| `id` | the user's id, as a string |
| `guard` | `admin` or `api` |
| `permissions` | the effective permissions in that guard (roles and direct grants) |
| `super_admin` | whether the user holds `super_admin` in that guard |
| `token_abilities` | the token's or JWT sign-in's abilities; `None` for a session |
| `auth_version` | the user's credential version when authenticated |
| `locale` | the interface language the user chose |
| `credential` | how the request was authenticated (`Session`, `Token(id)`, `Jwt(family)`), for the audit log |

`principal.can("posts.publish")` applies the rules above: super admin or the
permission, then the token's abilities.

Resolved permissions are cached for at most `auth.permission_cache_ttl_secs`
(30), keyed by a counter that every RBAC change advances: on any number of
instances, a committed change applies from the next request
([Credential revocation](revocation.md)).

## Declaring permissions

A permission exists in the database only once it is declared on the
`AuthProvider` and `rustcms permission:sync` ran. The demo's provider:

<<< @/../../../examples/demo/src/providers/auth_service_provider.rs#auth-provider [src/providers/auth_service_provider.rs]

| Method of `AuthProvider` | Declares |
|---|---|
| `.permission(guard, name, group)` | one permission, shown in `group` |
| `.api_permissions(&api_routes)` | every permission an `ApiRoutes` requires, on the `api` guard, grouped by the part before the first `.` (`posts.create` → `posts`) |
| `.admin_permissions(&admin_routes)` | the same for `AdminRoutes`, on the `admin` guard |
| `.define(ability, check)` | a gate ability (below); not a permission |
| `.permission_area`, `.permission_meta`, `.permission_preset` | how the Roles page shows them ([below](#the-roles-page-areas-labels-and-presets)) |

A permission declared with `.permission` *before* `api_permissions` or
`admin_permissions` keeps its group. Admin resources declare their own
abilities (`posts.view_any`, `posts.create`, …) when the `AdminProvider`
registers them ([Authorization in the panel](../admin/authorization.md)).
`AuthProvider` always declares the framework's own:
`rustcms::auth::BUILT_IN_PERMISSIONS`, on the admin guard:

| Group | Permissions |
|---|---|
| `users` | `users.view_any`, `users.view`, `users.create`, `users.update`, `users.delete`, `users.invite` |
| `roles` | `roles.view_any`, `roles.view`, `roles.create`, `roles.update`, `roles.delete`, `roles.assign` |
| `permissions` | `permissions.view_any`, `permissions.assign` |

`permission:sync` (or `RbacService::sync_permissions`) creates declared
permissions, marks undeclared ones **orphaned**, and never renames or deletes.
An orphaned permission stops granting access but stays on the roles that hold
it; declaring it again restores it (its grants apply again). A change of group
is applied too. Each sync that changes something records a `permission.synced`
entry in the [audit log](../digging-deeper/audit-log.md).

## The gate

The gate checks abilities that are not stored permissions, like Laravel's
`Gate::define`. Define them on the provider:

```rust
AuthProvider::new().define("reports.beta", |principal| BETA_TESTERS.contains(&principal.id.as_str()))
```

Check them through `AuthState.gate` (a sketch):

```rust
use rustcms::auth::AuthState;
use rustcms::prelude::*;

async fn beta_report(State(app): State<AppState>, auth: Auth) -> AppResult<Json<serde_json::Value>> {
    let gate = &app.service::<AuthState>().expect("AuthProvider").gate;
    gate.authorize(auth.principal(), "reports.beta")?; // 403 unless allowed
    Ok(Json(json!({ "ok": true })))
}
```

| Method of `Gate` | Does |
|---|---|
| `gate.allows(principal, ability)` | a defined ability: the token's abilities must list it (or `*`), then a super admin passes, else the closure decides; an undefined ability falls back to `principal.can(ability)` (deny by default) |
| `gate.authorize(principal, ability)` | `allows` or 403 |

A gate check runs inside the handler. For a route that reads a body, check
the gate before any body extractor runs, or declare a permission and use
route access instead.

## The Roles page: areas, labels and presets

The panel's Roles page groups admin permissions by **area**, with a search box
(name, label, description, area and group labels; case and diacritics
ignored, `đ` too). Declare how permissions are shown on the `AuthProvider`
(see the demo's provider above). Nothing here is stored or grants anything.

- **Areas.** Framework defaults: `resources` (sort 0: permissions that are
  exactly one resource's ability, shown as a matrix), `system` (1000: users,
  roles, permissions) and `other` (2000: the rest); the constants are
  `rustcms::auth::catalog::{RESOURCES_AREA, SYSTEM_AREA, OTHER_AREA}`. Declare
  your own with `permission_area(id, PermissionArea::new(label).sort(n))`, or
  relabel or reorder a default once with `permission_area("system", …)`.
  Within an area, the matrix comes first, then one box per permission group.
  An area starts open; with more than three, only the first.
- **Metadata.** `permission_meta(guard, name, PermissionMeta::new().label(..)
  .description(..).area(..))`. `label` is shown instead of the name, which
  stays visible below it, and `description` under it (in the matrix: the label
  names the cell, the description is its tooltip and screen-reader
  description); `area` must be declared. Labels (areas, permissions, presets)
  are 1–80 characters, descriptions 1–300; neither may be blank or contain
  control characters. Labels and descriptions are English sources,
  translated like other labels (`lang/vi.toml`; `rustcms lang:missing vi`
  lists them).
- **Presets** (`permission_preset(id, PermissionPreset::new(label)
  .description(..).permissions([..]))`) are starting selections for admin
  roles. Applying one changes the unsaved selection only: the permissions the
  user may grant become exactly the preset's, the others keep their state, and
  a notice says how many could not be granted. Saving checks everything as
  usual.
- **Bulk toggles** (a matrix row or column, a group) change only the
  permissions shown (matching the search, in an open area) that the user may
  grant. There is no select-all for an area.
- **Checked** with a panic naming the entry. The builders check ids
  (`[a-z0-9_-]`, 1–64), texts and duplicates at once: metadata once per
  `(guard, name)`, each area and preset id once. `AdminProvider::register`
  then checks references against the permissions declared so far (register
  providers that declare permissions before it): metadata must name a
  declared permission of that guard, its area must exist, a preset may only
  name declared admin permissions.

The page reads `AuthState.catalog` (`rustcms::auth::PermissionCatalog`).

## Changing access: `RbacService`

Every change of roles, permissions and user accounts goes through
`rustcms::auth::RbacService`, never through direct writes to the tables. Pass
`Actor::User(&principal)` for people and `Actor::System` for trusted operators
(console commands, seeders):

```rust
let auth = state.service::<AuthState>().expect("AuthProvider");
let rbac = RbacService::new(state.db(), &auth.resolver);
```

| Method | Does |
|---|---|
| `sync_permissions(&auth.permissions.all())` | creates declared permissions, orphans the others; returns a `SyncReport { created, orphaned }` |
| `create_role(actor, guard, name, &permissions)` | a role with these permissions |
| `set_role_permissions(actor, role_id, &permissions)` / `set_role_permissions_if(…, expected)` | replaces a role's permissions (`_if`: only if it still has `expected`) |
| `delete_role(actor, role_id)` | deletes a role (never a system role) |
| `assign_role(actor, user_id, role_id)` / `revoke_role(…)` | one role of one user |
| `set_user_roles(actor, user_id, guard, &role_ids)` / `set_user_roles_if(…)` | replaces a user's roles in a guard |
| `grant_permission(actor, user_id, guard, name)` / `revoke_permission(…)` | a direct permission |
| `set_user_active(actor, user_id, active)` / `set_user_active_if(…)` | enables or disables a user (disabling signs them out everywhere) |
| `delete_user(actor, user_id)` | deletes a user and their assignments |
| `create_user(actor, NewUser { name, email, password_hash, super_admin_guard })` | a user, optionally super admin of one guard |
| `create_user_with_roles(…)` / `invite_user(…)` / `resend_invitation(actor, user_id)` | a user with roles; an invited user, the mail queued in the same transaction |
| `update_profile(actor, user_id, name, email)` / `update_profile_if(…)` | name and email |
| `reset_password(actor, user_id, password_hash)` | sets another user's password, ends their sessions and tokens |
| `check_reset_password`, `check_new_user`, `covers_all_guards` | read-only preflights, before costly hashing |

The `_if` variants take what the editor started from and refuse when it
changed meanwhile (422 with the form error "This was changed by someone else
meanwhile"), so a stale form never undoes a concurrent change.

Roles for the panel are usually made in System → Roles. API-guard roles (for
tokens and JWT sign-ins), or roles an application needs from the start, are
created in a seeder. The LMS creates its roles once, finding each by name
first, so the seeder can run again:

<<< @/../../../examples/lms/src/database/seeders/role_seeder.rs#role-seeder [src/database/seeders/role_seeder.rs]

Then assign roles with `rustcms role:assign --email bot@example.com --role
publisher --guard api` (or `assign_role`), and issue a token with
`rustcms token:create`.

### What `RbacService` refuses

| Attempt | Result |
|---|---|
| Assigning roles with only `users.update` (needs `roles.assign`) | 403 |
| Assigning a role containing a permission the actor lacks | 403 |
| Adding **or removing** a permission the actor lacks on a role | 403 |
| Editing a role the actor holds | 403 |
| Changing one's own roles, direct permissions or active flag | 403 |
| Managing roles of another guard from a user session | 403 (editing such a role's permissions: 404, as for an unknown role) |
| Editing, assigning or revoking a system or privileged role without being a super admin (a narrow token does not count) | 403 |
| Assigning a role that carries orphaned permissions without being a super admin | 403 |
| Replacing another user's credentials (password, email) without being a super admin of **every** guard | 403 |
| Changing the status, profile or existence of an account that holds a system or privileged role without being a super admin of **every** guard | 403 |
| Deleting a system role (even as `System`) | 403 |
| Removing, disabling or deleting the last active super admin of a guard | 422 |

The credential rules ask for every guard because the same password opens the
admin panel and issues API tokens: setting someone's password or email is
signing in as them everywhere. A super admin's token whose abilities are not
`*` never counts as a super admin here. `Actor::System` passes every rule
about the actor, but not the two rules about the state: it cannot delete a
system role, and it cannot leave a guard without an active super admin. That
last check runs, whoever the actor, on every change that can take a super
admin away: `revoke_role` and `set_user_roles` when they remove `super_admin`,
`set_user_active(false)` and `delete_user` for a user who holds it. Editing a
role's permissions never takes it away (super admin status comes from holding
the `super_admin` role, not from its permissions), and the role itself cannot
be deleted. Users whose invitation is still pending do not count as active.

### Transactions and the audit log

Every change runs in one write transaction that first takes the RBAC lock
(`rustcms::auth::rbac::lock`: a transaction-scoped advisory lock on
PostgreSQL, the `rbac` row of `rustcms_locks` on MySQL, the write lock on
SQLite) and then re-reads the actor's own grants and credential version. A
change by a user who was disabled or signed out everywhere meanwhile is
refused (403). Concurrent changes are therefore applied one after another,
each checked against the state left by the previous one: an edit cannot
restore a permission revoked meanwhile, and a promotion racing a removal
cannot leave a guard without an active super admin. A change waits at most
10 s for the lock (`lock_timeout` on PostgreSQL, the row lock wait on MySQL),
then fails with 409 `write_conflict`.

`rustcms::auth::rbac::begin_as(db, &principal)` opens the same locked
transaction for a change of your own that depends on the actor's permissions
(the panel's settings pages use it): it returns the transaction and the
principal as it is now, or 403.

Each change that changes something is recorded in the
[audit log](../digging-deeper/audit-log.md), in the same transaction, with the
actor. To record the request too (IP, user agent, request id), pass the
context:

```rust
RbacService::new(db, &auth.resolver).audited(AuditContext::new(AuditActor::user(&principal), request))
```

## Security notes

- Deny by default holds everywhere: a permission nobody declared cannot be
  granted, and an orphaned one grants nothing.
- Route access and policies are enforced on the server, on every path; the
  Roles page only shows what the server will accept.
- Grant `roles.*` and `users.*` to administrators only: holders can hand out
  every permission they have.

## Related pages

- [Authentication](authentication.md)
- [Credential revocation](revocation.md)
- [Users, roles & your account](../admin/users-roles.md)
- [Authorization in the panel](../admin/authorization.md)
- [Routing: access](../basics/routing.md#access)
