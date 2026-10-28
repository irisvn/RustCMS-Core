# Resources

A resource puts one SeaORM entity in the admin panel, like a Filament
resource. It declares the entity's fields (what clients may read and write,
and who), and optionally its form, table, actions, relations, hooks, policy
and scope. Write one for each model your team manages in the panel.

![The demo's posts list, with its filters and the trash selector](/img/admin/resources-posts-list.webp)
*The demo application: the posts resource.*

## Quick start

Generate a resource from a model, then sync the permissions it declares:

```sh
rustcms make:resource Post --generate --belongs-to category_id=categories
rustcms permission:sync
```

This writes `src/admin/resources/post_resource.rs` and registers it in
`src/providers/admin_panel_provider.rs`:

```rust
AdminProvider::new()
    .brand("Blog")
    .resource::<crate::admin::resources::post_resource::PostResource>()
    // rustcms:resources
```

The smallest resource names its entity, a slug, its metadata and its
fields:

```rust
use rustcms::admin::*;

use crate::models::tag;

pub struct TagResource;

impl Resource for TagResource {
    type Entity = tag::Entity;
    const SLUG: &'static str = "tags";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Tag").title("name")
    }

    fn fields() -> Vec<Field> {
        vec![Field::column(tag::Column::Name).required().searchable().sortable().max_length(60)]
    }
}
```

The slug is the URL segment (`/admin/r/tags`) and the permission prefix
(`tags.view_any`, `tags.create`, …). Without `form()` and `table()`, the panel
builds a form and a table from the fields.

## Full example

The demo's posts: soft deletes, CSV export, a relationship, an owner column
set by the server, media, rich text and key/value attributes.

<<< @/../../../examples/demo/src/admin/resources/post_resource.rs#meta [src/admin/resources/post_resource.rs]

<<< @/../../../examples/demo/src/admin/resources/post_resource.rs#fields [src/admin/resources/post_resource.rs]

Its [form](forms.md), [table](tables.md), [actions](actions.md),
[policy and scope](authorization.md) and [tags relation](relationships.md) are
on their own pages.

## Generating a resource

`rustcms make:resource <Model> --generate` reads the model and writes a field,
an input and, where it fits, a column and a filter for each column:

| Column | Becomes |
|---|---|
| `created_at`, `updated_at`, `deleted_at` | skipped; a nullable `deleted_at` timestamp adds `.soft_deletes()` |
| a name containing `password`, `secret`, `token`, `hash`, `api_key` or `salt` | skipped, with a warning (add it by hand, write-only) |
| `author_id`, `user_id`, `owner_id`, `created_by`, `updated_by` | `.guarded().readable(\|_\| true)`: shown, set by your `mutate_before_create` |
| a column named in `--belongs-to FIELD=SLUG` | `.belongs_to(SLUG).filterable()`, a select, a column and a filter |
| another integer column ending in `_id` | a text input with a `TODO` (pass `--belongs-to` to get a select) |
| other numbers | a number input, sortable |
| a `String` primary key | `KeyStrategy::Natural`, typed in on create |
| the first other `String` column | the title, searchable and sortable |
| `body`, `content`, `description`, `notes`, `text`, `summary`, `excerpt`, `bio` | a `Textarea` |
| `bool` | a toggle, a check-mark column and a yes/no filter |
| dates, date-times | pickers, date columns (a date also gets a range filter) |
| JSON | a `Textarea` |

Without `--generate` it writes a resource with no fields to fill in;
`--belongs-to` needs `--generate`. The model must exist
(`rustcms make:model Post -m`). Read the output before you commit it: it is a
starting point.

## Fields

`fields()` is the contract with clients: **columns not listed are never read
or written** by the panel or its API. Fields are readable and writable by
default; each method narrows what the column allows:

| Method | Does |
|---|---|
| `Field::column(post::Column::Title)` | a field for a typed column (`Field::make("title")` takes the name) |
| `.required()` | client input must send it on create and may never set it to `null` (`""` is a value: check blank text in a hook); hooks may still clear a nullable column |
| `.max_length(n)` | characters (text), checked on every write |
| `.searchable()`, `.sortable()`, `.filterable()` | allowed in search, sort and filters |
| `.belongs_to("categories")` | a relationship: values must be visible records of that resource |
| `.guarded()` | never written from input (hooks may set it); not readable unless `.readable(…)` |
| `.readable(\|ctx\| …)`, `.creatable(…)`, `.updatable(…)` | field rules per user, checked on the server |
| `.label("…")` | the label (default: the column name in words) |
| `.media()`, `.media_list(n)`, `.accept(…)`, `.max_size(…)` | [media](media.md#media-fields) fields |
| `.rich_text()` | a [rich text](media.md#rich-text) document |
| `.key_value()`, `.max_items(n)` | a [key/value](forms.md#key-value) object |
| `.encrypted()`, `.encrypted_map()` | a secret, or named secrets, sealed at rest and never shown ([encrypted fields](forms.md#encrypted-fields)) |
| `.audit_redact()` | its values never reach the [audit log](../digging-deeper/audit-log.md) (only that it changed) |

`field.can_read(ctx)` and `field.can_write(Operation::Create | Update, ctx)`
evaluate the rules, for code of your own that needs the same answer.

`form()` and `table()` only describe presentation ([Forms](forms.md),
[Tables](tables.md)). Field rules decide what a user may do whatever the
form shows: see [Authorization](authorization.md).

## Metadata

| `ResourceMeta` | Does |
|---|---|
| `::new("Post")` | singular label |
| `.plural("Posts")` | plural label (default: label + `s`) |
| `.title("title")` | the field used as a record's title (labels, options, the record page) |
| `.group("Content")`, `.icon("file-text")` | [navigation](overview.md#navigation) |
| `.sort(10)`, `.navigation(false)` | [order](overview.md#order), [hidden from the navigation](overview.md#hiding-a-resource) |
| `.key(KeyStrategy::…)` | [primary keys](#primary-keys) |
| `.unique(constraint, &[columns], message)` | maps a unique constraint to a field error ([below](#unique-constraints)) |
| `.soft_deletes()`, `.soft_deletes_column("removed_at")` | [soft deletes](#soft-deletes) |
| `.lock_parent("course_id", "courses")` | writes lock the parent record first ([parent locks](#parent-locks)) |
| `.exportable()`, `.importable()` | CSV [export and import](import-export.md) (abilities `export`, `import`) |
| `.import_key(field)` | imported rows matching a record by `field` update it ([import](import-export.md#importing)); implies `.importable()` |
| `.audit(false)` | writes are not recorded in the [audit log](../digging-deeper/audit-log.md) (on by default) |

### Unique constraints

`.unique(name, columns, message)` turns a database unique violation into a
validation error (422). `name` is the constraint's or unique index's name
(PostgreSQL, MySQL); `columns` identify it on SQLite. With one column the
message goes under that field; with several it is a form error. The message
is translated like other labels. A violation of a constraint you did not map
answers the form error *conflicts with existing data*.

<<< @/../../../examples/demo/src/admin/resources/category_resource.rs#meta [src/admin/resources/category_resource.rs]

## Checked at boot

A resource is checked when the application starts. Each of these stops the
boot with a message naming the resource:

- the primary key is not `i32`, `i64`, `String` or `Uuid`, or is composite;
- the slug is longer than 64 bytes, or is one of `user`, `role`,
  `permission`, `settings`, `session`, `job`, `queue` (reserved);
- a field names an unknown or unsupported column, or is declared twice;
- the title or import key is not a field; `import_key` has no `unique`
  constraint on that column alone; a `unique` column does not exist;
- the primary key is `.audit_redact()` while the resource is audited;
- a rich text field is not a JSON column, a `media()` field not a UUID
  column, a `media_list()` field not a JSON column, or one of them has a
  column default;
- a key/value field is not a JSON column;
- the soft-delete column is unknown, not a nullable timestamp, or a field that
  is not `.guarded()`;
- form, table, action and relation mistakes, described on their pages.

## The record page

`/admin/r/<slug>/records/<key>` shows one record, laid out like its form
(sections, tabs, the editor split) with values read-only: option labels,
related records by title, dates in the user's language. Under it come:

- the [slots](customizing.md#slots-beside-records) registered for the resource;
- one section per [relation](relationships.md) the user may list;
- the record's **History**, for users with `audit.view` when the resource is
  audited ([audit log](../digging-deeper/audit-log.md#viewing-the-log)).

The header shows the record's [actions](actions.md#placement-in-lists), Edit
and Delete, as the server's abilities for that record allow. A trashed record
offers no actions.

![The demo's post page with its actions, tags and history](/img/admin/resources-record-page.webp)
*The demo application: a post's page.*

## Hooks

Hooks are async methods of the resource. They run inside the write's
transaction: whatever they write commits or rolls back with the record. Put
`#[async_trait]` on the `impl` (`use rustcms::async_trait;`, or the
`async-trait` crate as the examples do):

<<< @/../../../examples/demo/src/admin/resources/post_resource.rs#hooks [src/admin/resources/post_resource.rs]

Every write runs in one transaction, in this order:

1. reject undeclared or non-writable keys (422),
2. type-check values against the [wire format](../reference/wire-format.md)
   and check `.required()`,
3. lock the parent record ([parent locks](#parent-locks)),
4. `mutate_before_create` / `mutate_before_update` hooks,
5. check the final values, hooks' changes included: `belongs_to` keys must be
   records the user can see, and media (rich text images included) files the
   user can see (422); limits (`max_length`, …) are re-checked as the values
   are written, so hooks cannot bypass them,
6. write (database constraints are mapped to field errors with `ResourceMeta::unique`),
7. write the links of [belongs-to-many selects](relationships.md#choosing-links-in-the-records-form),
   then `after_relation_change` for each relation whose links changed,
8. `after_save` hook — an error rolls everything back.

Hooks see foreign keys **before** they are checked: do not trust a
`belongs_to` value read in `mutate_before_*` (look the record up under your
own rules, or wait for `after_save`).

### Validating in a hook

Return `ValidationErrors` to refuse a write with messages under the inputs
(422; nothing is written). The LMS checks seats, the enrollment's status and
the progress range, and queues a welcome mail with the enrollment:

<<< @/../../../examples/lms/src/admin/resources/enrollment_resource.rs#hooks [src/admin/resources/enrollment_resource.rs]

- `ValidationErrors::field(name, message)` is one error; build several with
  `ValidationErrors::new()`, `.add(field, message)` and `.add_form(message)`
  (a message for the whole form), then `Err(errors.into())`.
- `ctx.dispatch(txn, &job)` queues a [job](../digging-deeper/queues.md) in
  the write's transaction: it exists only if the write commits.
- A message a hook returns is shown as written: translate it with
  `ctx.t("…")` ([i18n](i18n.md)).

### Hooks after a write

`mutate_before_update` receives the current record too. The other hooks run
in the transaction of their write, after it, with the record; an error rolls
the write back:

| Hook | Runs when | Record |
|---|---|---|
| `after_save` | a create, an update; a restore through the default `after_restore` | as stored |
| `after_delete` | a record of a resource without soft deletes is deleted | as it was |
| `after_trash` | a record is moved to the [trash](#soft-deletes) | as stored (marker set) |
| `after_restore` | a trashed record is restored (default: calls `after_save`) | as stored (marker cleared) |
| `after_force_delete` | a trashed record is deleted for good | as it was |
| `after_relation_change` | links of a [belongs-to-many](relationships.md#reacting-to-link-changes) relation change | the parent, as stored |

```rust
// A sketch: `UnpublishPost` is a job of your application.
async fn after_trash(ctx: &Ctx, txn: &DatabaseTransaction, post: &post::Model) -> AppResult<()> {
    ctx.dispatch(txn, &UnpublishPost { id: post.id }).await?;
    Ok(())
}
```

Dispatch a job there to undo what the record configured outside the database,
or to publish and unpublish it: the job runs only if the write commits.
Trashing does not call `after_delete`. The default `after_restore` calls
`after_save`; overriding `after_restore` replaces that call (call
`Self::after_save(ctx, txn, record).await` from it to keep both).
[Actions](actions.md) (row or bulk) write through their handler: they call
none of these hooks.

## Parent locks

A rule over a parent's children (at most `capacity` active enrollments per
course) or a permission read through the parent (the course's instructor)
holds only if writes under one parent do not interleave. Declare the parent:

<<< @/../../../examples/lms/src/admin/resources/enrollment_resource.rs#meta [src/admin/resources/enrollment_resource.rs]

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

```rust
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
  records are kept. `after_trash` runs.
- **Restore** (`{slug}.restore`) clears the marker. The record's relationships
  must still point to records the user can see (422 otherwise);
  `after_restore` runs (by default, it calls `after_save`).
- **Force delete** (`{slug}.force_delete`) removes a trashed record for good;
  database foreign-key actions apply then; `after_force_delete` runs.
- Unique values stay taken by trashed records (the database still holds them).

### The trash

A list of a resource with soft deletes has a *Trashed records* selector
(*Without trashed*, *With trashed*, *Only trashed*) for users who may restore
or force delete. It sets `trashed=with|only` in the URL and the
[API](../reference/admin-api.md#lists). Trashed rows carry a *Trashed* badge
and offer **Restore** and **Delete permanently**, as the user's abilities on
each record allow; they offer no actions, and the *Only trashed* view has no
bulk actions. The trash shows a record only if the policy allows `view` and
`restore` or `force_delete` on it. With a custom policy, override `admits` for
these abilities like the others.

## Primary keys

| Strategy | Behaviour |
|---|---|
| `KeyStrategy::DbGenerated` | default; auto-increment or column default, never written |
| `KeyStrategy::AppGenerated(f)` | `f()` generates it on the server before insert (e.g. UUID v7) |
| `KeyStrategy::Natural` | supplied on create if its field is creatable; never updated |

```rust
use sea_orm::sea_query::Value;
use uuid::Uuid;

fn meta() -> ResourceMeta {
    ResourceMeta::new("Document").key(KeyStrategy::AppGenerated(|| Value::Uuid(Some(Uuid::now_v7()))))
}
```

Keys are `i32`, `i64`, `String` or `Uuid` columns. Composite primary keys are
not supported.

## Security

Fields, field rules, the policy and the scope apply on every path: see
[Authorization](authorization.md).

## Related pages

- [Forms](forms.md), [tables](tables.md), [actions](actions.md), [relationships](relationships.md)
- [Import and export](import-export.md), [media](media.md)
- [Admin API](../reference/admin-api.md)
