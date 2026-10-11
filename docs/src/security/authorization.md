# Roles & permissions

Spatie-style tables, scoped by guard: `permissions`, `roles`,
`role_has_permissions`, `model_has_roles`, `model_has_permissions`. A user may
hold roles in both guards; they never mix.

- **Deny by default.** Missing permission → 403.
- **Super admin.** The system role `super_admin` (seeded per guard by the
  migrations) passes every check *in its guard*, still bounded by token abilities.
- **Gate.** Ad-hoc abilities: `AuthProvider::new().define("publish", |p| …)`,
  checked with `gate.allows(principal, "publish")`.
- **Declaring permissions.** `AuthProvider::new().permission("api", "posts.view", "posts")`,
  then `rustcms permission:sync`. Sync never renames or deletes: undeclared
  permissions become *orphaned* and stop granting access.

## Changing access: `RbacService`

Every change goes through `RbacService`, with `Actor::User(&principal)` for
people and `Actor::System` for trusted operators (console). It refuses:

| Attempt                                                          | Result |
|------------------------------------------------------------------|--------|
| Assigning roles with only `users.update` (needs `roles.assign`)  | 403    |
| Assigning a role containing a permission the actor lacks         | 403    |
| Adding **or removing** a permission the actor lacks on a role    | 403    |
| Editing a role the actor holds                                   | 403    |
| Changing one's own roles, direct permissions or active flag      | 403    |
| Managing roles of another guard from a user session              | 403    |
| Editing, assigning or revoking a system/privileged role without being a super admin (a narrow token does not count) | 403 |
| Deleting a system role (even as `System`)                         | 403    |
| Removing, disabling or deleting the last active super admin of a guard | 422 |

Every change runs in one transaction that first takes the RBAC lock
(`rbac::lock`: a transaction-scoped advisory lock on PostgreSQL, the write lock
on SQLite) and then re-reads the actor's own grants. Concurrent changes are
therefore applied one after another, each checked against the state left by
the previous one: an edit cannot restore a permission revoked meanwhile, and a
promotion racing a removal cannot leave a guard without an active super admin.
