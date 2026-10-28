# Authorization

The admin panel is **deny by default**. Every request is checked on the
server, whether it comes from the panel, a script or another client. What
the panel hides is a convenience; it never protects your data. This page
lists the rules every other admin page relies on, then shows how to write a
policy and a scope.

![The demo's posts list seen by an editor who may only edit their own posts](/img/admin/authorization-editor-view.webp)
*The demo application: an editor with only `posts.update_own` sees their own posts and fewer actions.*

## Rules

These hold on **every path**: lists, single records, creates, updates,
deletes, the trash, actions, relation tables and their options, the
links chosen in a form, exports, imports and dashboard widgets.

- **Deny by default.** Without a policy of your own, each ability needs the
  permission `{slug}.{ability}` (`posts.update`). A user without any
  permission sees nothing.
- **Undeclared columns never leave the server.** Clients read and write only
  the columns listed in [`fields()`](resources.md#fields). A column that is
  not a field is never sent, searched, sorted, filtered, exported or accepted
  in input (422 `is not allowed`).
- **Field rules are checked on the server for every request.**
  `.readable(…)`, `.creatable(…)` and `.updatable(…)` decide per user. A
  field the user may not read is removed from records, columns, filters,
  sorts, search, exports and dashboard widgets. A field the user may not write
  is refused in input (422), whatever the form shows.
- **`.guarded()` fields** are never written from input. Hooks may set them.
  They are readable only with an explicit `.readable(…)`.
- **`scope()` decides which records exist for the user.** It applies to
  lists, lookups by key, writes, actions, relation tables, options of
  selects, the titles of related records, exports and dashboard counts. A
  record outside the scope answers 404, like a missing one. A write that
  moves a record out of the user's scope answers with its key only.
- **The policy decides per ability, and per record when it needs to.** Lists
  never show or count records the policy denies `view` (see
  [record-dependent lists](#record-dependent-lists)).
- **No existence probing.** Requests by key first ask
  `Policy::admits(ctx, ability)`. If it refuses, the answer is 403 whether or
  not the key exists. Actions and relation link writes (attach, detach, link
  edits) are admitted before their body is read; creates, updates and relation
  creates parse the JSON body first, so malformed JSON answers 400 or 422
  before an admission refusal.
- **Relationships are checked like records.** A `belongs_to` key a client
  sends must be a record that user may see in the target resource (422
  otherwise). A related record the user may not see is shown as `#<key>`,
  never by its title.
- **Encrypted fields are write-only** for everyone; their values never reach
  a client, a policy, the audit log or an export
  ([encrypted fields](forms.md#encrypted-fields)).

Your own code is trusted: hooks and action handlers may write any column, and
`Ctx::records` / `Ctx::record` read as the user does. Authorization decides
who may *run* that code.

## Permissions

Each resource declares its permissions for `rustcms permission:sync`:

| Declared | When |
|---|---|
| `{slug}.view_any`, `.view`, `.create`, `.update`, `.delete` | always |
| `{slug}.restore`, `{slug}.force_delete` | with [soft deletes](resources.md#soft-deletes) |
| `{slug}.export`, `{slug}.import` | with `.exportable()`, `.importable()` |
| `{slug}.{ability}` of each action | for every [action](actions.md) (its name, or its `.ability(…)`) |
| the `requires_permission(…)` of each action | as written |
| `{slug}.attach_{relation}`, `{slug}.detach_{relation}` | for every [belongs-to-many relation](relationships.md#belongs-to-many-pivot-tables) |
| `{slug}.update_pivot_{relation}` | when that relation has pivot fields |

Declare other permissions (such as `posts.update_own`) on the
`AuthProvider`:

```rust
AuthProvider::new()
    // Editors edit their own posts (see `PostPolicy`).
    .permission(ADMIN_GUARD, "posts.update_own", "posts")
```

The ability names are constants in `rustcms::admin::policy::ability`:
`VIEW_ANY`, `VIEW`, `CREATE`, `UPDATE`, `DELETE`, `RESTORE`, `FORCE_DELETE`,
`EXPORT`, `IMPORT`. Roles are edited on the
[Roles page](../security/authorization.md#the-roles-page-areas-labels-and-presets).

## Policies

A policy decides what a user may do. Write one when a permission per ability
is not enough, for example when editors may change only their own posts.

### Quick start

`rustcms make:policy PostPolicy` writes `src/policies/post_policy.rs`,
registers the module, and prints how to use it:

```rust
// in the resource
fn policy() -> Arc<dyn Policy> {
    Arc::new(crate::policies::post_policy::PostPolicy)
}
```

### The `Policy` trait

| Method | Default | Decides |
|---|---|---|
| `allows(ctx, ability, record)` | (required) | whether the user may do `ability`, on `record` when there is one (`None` for lists and creates) |
| `admits(ctx, ability)` | `true` | whether the user may do `ability` on *some* record; checked before any lookup by key (and, for actions and link writes, before the body is read) |
| `view_depends_on_record()` | `true` | whether `view` can differ between records (then lists check each record) |

The default policy, `PermissionPolicy::new(slug)`, admits and allows by
`{slug}.{ability}` and returns `false` from `view_depends_on_record`. You can
wrap it and delegate to it.

::: warning Override `admits`
The default `admits` lets everyone through to `allows`. Then a missing key
answers 404 and a denied record 403, so a user without any permission can
tell which keys exist. When a policy decides per record, also override
`admits` so that users who could not do the ability to *any* record are
refused first.
:::

### Full example

The demo's `PostPolicy`: editors with `posts.update_own` view, edit, publish,
tag and move their own posts; everything else needs the usual permission.

<<< @/../../../examples/demo/src/policies/post_policy.rs#policy [src/policies/post_policy.rs]

The resource uses it, and its scope keeps editors to their own posts:

<<< @/../../../examples/demo/src/admin/resources/post_resource.rs#policy-scope [src/admin/resources/post_resource.rs]

### What `record` holds

`record` is every column of the stored record, in the
[wire format](../reference/wire-format.md), **not** filtered by field rules,
so a policy can read an owner column that users never see. Encrypted fields
are `null`.

The wire format sends 64-bit integers as strings. `author_id` is an `i64`, so
`r["author_id"]` is `"7"`, and comparing it with `ctx.principal.id` (a
`String`) works. An `i32` column is a JSON number: compare it as a number
instead.

```rust
// i64 column: a string on the wire
record.is_some_and(|r| r["author_id"].as_str() == Some(ctx.principal.id.as_str()))
// i32 column: a number on the wire
record.is_some_and(|r| r["owner_id"].as_i64() == ctx.principal.id.parse::<i64>().ok())
```

### Record-independent policies

When a user's access does not depend on the record (the record's visibility
is all in `scope()`), return `false` from `view_depends_on_record`. Lists are
then paged by the database. The LMS's `EnrollmentPolicy` works this way:

<<< @/../../../examples/lms/src/policies/enrollment_policy.rs#policy [src/policies/enrollment_policy.rs]

### Record-dependent lists

With `view_depends_on_record() == true` (the default for custom policies),
a list reads every matching row and asks the policy about each one, so denied
records are neither shown nor counted. The scan stops at 10,000 matching
rows: beyond that the list answers 422 *Too many matching records to check
individually; narrow the search or filters.* Put large visibility rules in
`scope()` and return `false` to keep SQL pagination.

### Actions

An action is listed in the schema only if the user has its
`requires_permission` (when set) and the policy `admits` its ability. Each
record is then checked with `allows`. See [actions](actions.md).

## Scopes

`scope()` restricts which records exist for a user. It receives the SeaORM
query and returns it narrowed. The demo shows editors only their own posts
(see `scope` in the [example above](#full-example)); the LMS shows
instructors the courses they teach:

<<< @/../../../examples/lms/src/admin/resources/course_resource.rs#scope [src/admin/resources/course_resource.rs]

A scope runs for every request, so keep it a query: no blocking work, no
other requests.

## Policies that query the database

Use `ctx.conn()` in policies. During a write it is the write's own
transaction, so the policy sees the state being written and never waits for a
second pooled connection; elsewhere it is the pool. In hooks, use the `txn`
they receive.

```rust
// A sketch: `membership` is a model of your application.
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};

async fn allows(&self, ctx: &Ctx, ability: &str, record: Option<&Data>) -> bool {
    let Some(team) = record.and_then(|r| r["team_id"].as_str()).and_then(|k| k.parse::<i64>().ok()) else {
        return ctx.can(&format!("projects.{ability}"));
    };
    let user: i64 = ctx.principal.id.parse().unwrap_or(-1);
    membership::Entity::find()
        .filter(membership::Column::TeamId.eq(team))
        .filter(membership::Column::UserId.eq(user))
        .count(&ctx.conn())
        .await
        .is_ok_and(|n| n > 0)
}
```

A policy fails closed: return `false` on a database error.

## `Ctx`

Policies, scopes, field rules, hooks and actions receive a `Ctx`, the
request's context:

| Member | Use |
|---|---|
| `ctx.principal` | the signed-in user (`id`, permissions) |
| `ctx.can(permission)` | whether the user has a permission |
| `ctx.conn()` | the write's transaction, or the pool |
| `ctx.t(text)`, `ctx.t_in(slug, text)` | a message in the user's language ([i18n](i18n.md)) |
| `ctx.dispatch(txn, &job)` | queue a [job](../digging-deeper/queues.md) in the write's transaction |
| `ctx.audit(txn, entry)` | add an entry to the [audit log](../digging-deeper/audit-log.md#recording-your-own-events) |
| `ctx.keyring()` | open or seal [encrypted values](../digging-deeper/encryption.md) |
| `ctx.records(slug, &query)`, `ctx.record(slug, key)` | read another resource as this user may (scope, policy and field rules apply) |
| `ctx.page_path()` | the sub-path of a [custom page](customizing.md#custom-pages) |

## Pitfalls

- A custom policy that does not override `admits` leaks which keys exist.
- Field rules are closures over `Ctx`, not over the record: per-record write
  rules belong in the policy or a hook.
- `requires_permission` and `admits` hide an action; `visible_when` hides it
  per record. None of them restrict what the handler writes.
- Static options of a `SelectInput` or `CheckboxList` bound what a client
  writes, not what hooks and action handlers write: validate those yourself
  ([forms](forms.md#options-are-not-validated)).

## Related pages

- [Roles & permissions](../security/authorization.md)
- [Resources](resources.md), [forms](forms.md), [actions](actions.md), [relationships](relationships.md)
- [Admin API](../reference/admin-api.md)
