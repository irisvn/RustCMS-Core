# Roles & permissions

Permissions decide what a user may do; [policies](../admin/authorization.md)
decide it per record in the admin panel, and [route access](../basics/routing.md#access)
per API route.

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

Each change that changes something is recorded in the
[audit log](../digging-deeper/audit-log.md), in the same transaction, with the
actor. To record the request too (IP, user agent, request id), pass the
context: `RbacService::new(db, &auth.resolver).audited(AuditContext::new(AuditActor::user(&principal), request))`.

## Creating roles from code

The panel manages admin-guard roles. API-guard roles (for tokens and JWT), or
roles you seed, are created through `RbacService`, for example in a console
command:

```rust,ignore
use rustcms::auth::{API_GUARD, Actor, AuthState, RbacService};

async fn run(&self, state: &AppState, _: &ArgMatches) -> anyhow::Result<()> {
    let auth = state.service::<AuthState>().expect("AuthProvider");
    let rbac = RbacService::new(state.db(), &auth.resolver);
    rbac.sync_permissions(&auth.permissions.all()).await?;          // the declared permissions exist
    let permissions = ["posts.view_any".to_owned(), "posts.create".to_owned()];
    let role = rbac.create_role(Actor::System, API_GUARD, "publisher", &permissions).await?;
    println!("created role {}", role.id);
    Ok(())
}
```

Then assign it: `rustcms role:assign --email bot@example.com --role publisher --guard api`,
and issue a token with `rustcms token:create`.
