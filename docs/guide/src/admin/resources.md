# Resources

A resource exposes one SeaORM entity in the admin panel, like a Filament
resource: its fields (data and permissions), form, table, actions, hooks and
policy. `rustcms make:resource Post --generate` writes one from the model in
`src/admin/resources/post_resource.rs` and registers it in
`src/providers/admin_panel_provider.rs`:

```rust,ignore
pub fn provider() -> AdminProvider {
    AdminProvider::new().brand("Blog").resource::<PostResource>().resource::<CategoryResource>()
}
```

Each resource declares its permissions (`posts.view_any`, `posts.view`,
`posts.create`, `posts.update`, `posts.delete`, plus action abilities) for
`rustcms permission:sync`.

```rust,ignore
use rustcms::admin::*;

pub struct PostResource;

impl Resource for PostResource {
    type Entity = post::Entity;
    const SLUG: &'static str = "posts";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Post")
            .title("title")
            .key(KeyStrategy::AppGenerated(|| Value::Uuid(Some(Uuid::now_v7()))))
    }

    fn fields() -> Vec<Field> {
        vec![
            Field::column(post::Column::Title).required().searchable().sortable(),
            Field::column(post::Column::CategoryId).belongs_to("categories").filterable(),
            Field::column(post::Column::AuthorId).guarded().readable(|_| true),
            Field::column(post::Column::Published).updatable(|ctx| ctx.can("posts.publish")),
        ]
    }
}
```

## Fields

`fields()` is the contract with clients: **columns not listed are never read
or written** by the panel or its API. Each field sets what the column allows:

| Method | Does |
|---|---|
| `.required()` | must be present on create and never set to `null` (`""` is a value: check blank text in a hook) |
| `.max_length(n)` | characters (text), checked on every write |
| `.searchable()`, `.sortable()`, `.filterable()` | allowed in search, sort and filters |
| `.belongs_to("categories")` | a relationship: values must be visible records of that resource |
| `.guarded()` | never written from input (hooks may set it); not readable unless `.readable(…)` |
| `.readable(\|ctx\| …)`, `.creatable(…)`, `.updatable(…)` | field rules per user, checked on the server |
| `.label("…")` | the label (default: the column name in words) |
| `.media()`, `.media_list(n)`, `.accept(…)`, `.max_size(…)` | [media](media.md) fields |
| `.rich_text()` | a [rich text](media.md#rich-text) document |
| `.key_value()`, `.max_items(n)` | a [key/value](forms.md#key-value) object |
| `.audit_redact()` | its values never reach the [audit log](../digging-deeper/audit-log.md) (only that it changed) |

`form()` and `table()` only describe presentation ([Forms](forms.md),
[Tables](tables.md)); leave them out to get one input per field and the first
fields as columns.

## Metadata

| `ResourceMeta` | Does |
|---|---|
| `::new("Post")` | singular label |
| `.plural("Posts")` | plural label (default: label + `s`) |
| `.title("title")` | the field used as a record's title |
| `.group("Content")`, `.icon("file-text")` | [navigation](overview.md#navigation) |
| `.sort(10)`, `.navigation(false)` | [order](overview.md#order), [hidden from the navigation](overview.md#hiding-a-resource) |
| `.key(KeyStrategy::…)` | [primary keys](#primary-keys) |
| `.unique(constraint, &[columns], message)` | maps a unique constraint to a field error |
| `.soft_deletes()`, `.soft_deletes_column("removed_at")` | [soft deletes](#soft-deletes) |
| `.lock_parent("course_id", "courses")` | writes lock the parent record first ([parent locks](#parent-locks)) |
| `.audit(false)` | writes are not recorded in the [audit log](../digging-deeper/audit-log.md) (on by default) |

## Hooks

Hooks are async methods of the resource (`use rustcms::async_trait;` and put
`#[async_trait]` on the `impl`; an attribute path cannot start with `rustc…`). Every write runs in one transaction:

1. reject undeclared or non-writable keys (422),
2. type-check values against the [wire format](../reference/wire-format.md),
3. check relationship keys against the target scope,
4. `mutate_before_create` / `mutate_before_update` hooks,
5. re-check limits (`max_length`) on the final values, so hooks cannot bypass them,
6. write (database constraints are mapped to field errors with `ResourceMeta::unique`),
7. `after_save` hook — an error rolls everything back.


```rust,ignore
async fn mutate_before_create(ctx: &Ctx, _txn: &DatabaseTransaction, mut data: Data) -> AppResult<Data> {
    data.insert("author_id".into(), json!(ctx.principal.id));     // guarded fields may be set here
    Ok(data)
}

async fn after_save(ctx: &Ctx, txn: &DatabaseTransaction, post: &post::Model) -> AppResult<()> {
    // runs in the write's transaction: an error rolls everything back
    Ok(())
}
```

`mutate_before_update` receives the current record too; `after_force_delete`
runs when a trashed record is deleted for good. A message a hook returns is
shown as written: translate it with `ctx.t("…")` ([i18n](i18n.md)).

## Parent locks

A rule over a parent's children (at most `capacity` active enrollments per
course) or a permission read through the parent (the course's instructor)
holds only if writes under one parent do not interleave. Declare the parent:

```rust,ignore
fn meta() -> ResourceMeta {
    ResourceMeta::new("Enrollment").lock_parent("course_id", "courses")
}
```

Every write to an enrollment (create, update, delete, restore, force delete,
actions, creates from a course's page) then locks its course row before the
enrollment, and both courses when it moves. Hooks run under that lock, so a
count in `mutate_before_create` is exact, and an instructor removed from the
course meanwhile no longer passes the scope, checked again under the lock.

- One order everywhere: parent, then child, the order a parent's deletion
  cascades in, so writes and deletions never deadlock. A write takes at most
  one `xact_lock` and takes it first; parent locks come after it.
- The field must be a `belongs_to` the target; one parent per resource, and
  the parent may not lock a parent of its own (checked at boot).
- A hook, an action or a column default that puts a record under a parent
  that was not locked fails (500, nothing written): send the parent field
  (make it `.required()`), and change it through an update. A record moved
  by someone else between reading and locking answers 409 `write_conflict`;
  send the request again.
- Writes under one parent wait for each other; writes under different parents
  do not. SQLite runs one writer at a time anyway.

## Soft deletes

```rust,ignore
fn meta() -> ResourceMeta {
    ResourceMeta::new("Post").soft_deletes()   // or .soft_deletes_column("removed_at")
}
```

The marker is a nullable timestamp column (`Option<DateTimeUtc>` or
`Option<DateTime>`), declared as a field only if `.guarded()` (e.g.
`.guarded().readable(|_| true)` to show it); hooks may not write it.

- **Delete** sets the marker. Trashed records disappear everywhere: lists,
  reads, writes, actions, relationship options and labels (`#key`), and
  references from other records (422). No database cascade runs: related
  records are kept.
- **Restore** (`{slug}.restore`) clears the marker. The record's relationships
  must still point to records the user can see (422 otherwise); `after_save`
  runs.
- **Force delete** (`{slug}.force_delete`) removes a trashed record for good;
  database foreign-key actions apply then.
- The trash (`trashed=with|only`) needs `restore` or `force_delete`, and shows a
  record only if the policy allows `view` and one of them on it. With a custom
  policy, override `admits` for these abilities like the others.
- Unique values stay taken by trashed records (the database still holds them).
- `make:resource --generate` adds `.soft_deletes()` for a nullable `deleted_at`
  timestamp.

## Primary keys

| Strategy                  | Behaviour                                                         |
|---------------------------|-------------------------------------------------------------------|
| `KeyStrategy::DbGenerated`  | default; auto-increment or column default, never written       |
| `KeyStrategy::AppGenerated` | generated by server code before insert (e.g. UUID v7)          |
| `KeyStrategy::Natural`      | supplied on create if its field is creatable; never updated    |

Composite primary keys are not supported yet.
