# Relationships

A record's page can list the records of another resource that belong to it,
and link records through a pivot table:

- a **has-many relation** lists the records pointing to this one (a
  category's posts) and creates new ones there. The child keeps a
  `belongs_to` field to the parent.
- a **belongs-to-many relation** links records through a pivot table (posts ↔
  tags), with optional columns on each link.

A single reference to another record (a post's category) is a field with
[`.belongs_to(…)`](resources.md#fields): a select in the form, the related
record's title in tables.

![The demo's category page with its Posts section](/img/admin/relationships-has-many.webp)
*The demo application: a category's posts.*

## Quick start

The demo's categories list their posts; each post points to its category:

<<< @/../../../examples/demo/src/admin/resources/category_resource.rs#relations [src/admin/resources/category_resource.rs]

```rust
// in PostResource::fields()
Field::column(post::Column::CategoryId).belongs_to("categories").filterable(),
```

`Relation::has_many(name, target, field)`: `name` is the relation's id in URLs
(snake_case, unique on the resource), `target` the child resource's slug and
`field` the child's foreign key. `.label(…)` titles its section (default: the
name in words). A resource may have several; the LMS course page shows its
lessons and its enrollments:

<<< @/../../../examples/lms/src/admin/resources/course_resource.rs#relations [src/admin/resources/course_resource.rs]

When the panel starts it checks that the target exists, that `field` is a
field of it declared `belongs_to("<this slug>")`, that its column kind matches
this resource's primary key and that it is not the child's own key; anything
else stops the boot with the fix.

## What users see

On the parent's view page, each relation is a section with a compact table of
the children: the target's table columns (without the foreign key), search and
paging, and a **View** link per row. Rows also offer the child resource's
[actions](actions.md#placement-in-lists) (the first primary one labelled, the
others under **⋯**): the server's abilities for each record decide, and running
one is authorized as from the child's list. Edits and deletes happen on the
child's own pages. **New** opens the child's create form for this parent: the foreign
key input is not shown, the server sets it, and the form returns to the parent
when saved.

## Authorization

Nothing is bypassed: a relation is the target's own list and create, with one
extra condition (`field = parent key`).

- **Listing** needs `view` on the parent resource (`admits`) and the target's
  `view_any`, checked before the parent is looked up: without them the answer is
  403 whatever the key. Then the parent must be visible to the user (scope,
  policy, not trashed), otherwise 404. A policy refusal is a 404 here too,
  unlike `GET …/records/{key}`, which answers 403 for a record it may not view. The
  children are filtered by the target's scope, policy and field rules as on its
  own list; `trashed=with|only` works as there. The foreign key field must be
  readable by the user. Every check that does not depend on a record (the
  abilities, `trashed`, the readable foreign key, paging) runs before the parent
  is looked up, so a refusal never tells whether the parent exists.
- **Creating** needs `view` on the parent resource, the target's `create` and
  write access to the foreign key field on create (all checked before the
  parent is looked up: 403), then a visible parent (else 404). Like any
  `belongs_to` value, the parent must also pass the reference check, which
  needs the parent resource's `view_any` and `view`. The foreign key is set by the server
  **before** the input is validated, so the field must be writable on create by
  the user and all the usual rules run (`belongs_to` included, after hooks). A
  client that sends the foreign key gets a 422 (`is set by the relation`). If a
  hook changes it, the write fails (500) and nothing is stored.
- The parent's view schema lists only relations whose target the user may list
  (`relations[].create` says whether **New** is offered).

A parent trashed while a child is being created keeps the
[write-in-progress rule](../reference/limitations.md#writes-in-flight): the
child's `belongs_to` check sees the parent as it was when the check ran.

## Idempotency

Relation creates accept `Idempotency-Key` like any create. The claim is bound to
the relation and the parent: the same key sent to another parent (or to the
target's own create route) is a separate claim, not a replay. A committed retry
is answered even if the parent was hidden, trashed or deleted since, like any
replay.

## Belongs-to-many (pivot tables)

Records linked through a **pivot table** (posts ↔ tags), with optional columns
on each link (`position`). The demo's posts have tags:

<<< @/../../../examples/demo/src/admin/resources/post_resource.rs#relations [src/admin/resources/post_resource.rs]

`Relation::belongs_to_many(name, target, parent_column, related_column)`
takes the pivot's two key columns: the one holding this resource's key, and
the one holding the target's. `.pivot(fields)` declares the pivot columns
users see and edit with each link. The pivot is a SeaORM model of its own,
with a primary key over both columns:

<<< @/../../../examples/demo/src/models/post_tag.rs#model [src/models/post_tag.rs]

Its migration makes both columns the key and deletes a link with either
record, through the migration helpers so it also runs on MySQL:

<<< @/../../../examples/demo/src/database/migrations/m20261022_000001_create_tags.rs#post-tags [src/database/migrations/m20261022_000001_create_tags.rs]

The record's page lists the linked records (the target's table, scope and
policy, search and pages) with the readable pivot columns, and offers, as
allowed:

- **Attach**: choose records of the target (its options: in the user's scope,
  allowed to view; already linked ones are marked) and the pivot columns set
  on new links. Attaching a record already linked keeps that link as it is.
- **Detach**: the selected links. Links to records the user cannot see are
  never touched.
- **Edit link**: the pivot columns the user may update.

Rows also offer the target resource's [actions](actions.md#placement-in-lists),
as in has-many tables: the server's abilities for each record decide, and
running one is authorized as from the target's list.

![The demo's post page: the Tags section and its Attach dialog](/img/admin/relationships-tags.webp)
*The demo application: a post's tags, with the position of each link.*

### Permissions

Writes need an ability of the **parent** record:
`attach_{relation}`, `detach_{relation}` and `update_pivot_{relation}` (only
with pivot fields). With the default policy that is `posts.attach_tags`,
`posts.detach_tags`, `posts.update_pivot_tags`, declared for
`permission:sync`. A policy of your own receives them like any ability; if it
decides per record (`allows` with the record), also override
[`Policy::admits`](authorization.md) for them, so users without the
permission are refused before the request body is read. The demo's
`PostPolicy` lets owners tag their own posts this way. `LinkOp::Attach`,
`Detach` and `UpdatePivot` name these abilities in code
(`LinkOp::Attach.ability("tags")` is `"attach_tags"`).

Every write checks, in this order: the ability is admitted (403, whatever the
body or the record); the body (422: 1 to 100 keys, pivot columns by their
field rules; undeclared columns are refused); then, in one transaction, the
parent record (in scope: 404; policy: 403) locked, and the targets the user may
view locked and checked again. For attach and detach, targets not found,
outside the user's scope, trashed or not viewable come back in `missing` and
are left alone; the answer is `{processed, missing}`. A link edit answers 404
when its target key is invalid, not viewable or not linked.

### Pivot fields

Pivot columns are [fields](resources.md#fields): `readable`, `creatable`,
`updatable`, `required`, `max_length` apply, values follow the
[wire format](../reference/wire-format.md); columns not declared never leave
the server. Attaching sends only the pivot columns the user filled in (the
others keep their database defaults); editing a link sends only what changed,
and a change to the values already stored writes and records nothing. Users
who may not write a `.required()` pivot field whose column is not nullable and
has no database default cannot attach (403); otherwise the field is left out.

A pivot field may not be:

- one of the two key columns, a column the pivot does not have, or declared
  twice;
- a `belongs_to`, media, rich text or encrypted field.

Every pivot column that is not a key and not declared must be nullable or have
a default; a mandatory one is a `.required()` pivot field.

### Reacting to link changes

`Resource::after_relation_change` runs in the transaction of every write
that changed links of the record, after the pivot rows are written, with the
parent record and what changed (target keys, canonical, sorted):

```rust
// A sketch: the demo's posts do not implement it.
async fn after_relation_change(
    _ctx: &Ctx,
    txn: &DatabaseTransaction,
    post: &post::Model,
    change: &RelationChange,
) -> AppResult<()> {
    if change.relation == "tags" {
        // change.attached, change.detached, change.pivot_updated: Vec<String>
        let mut active = post.clone().into_active_model();
        active.updated_at = Set(chrono::Utc::now()); // links count as an edit
        active.update(txn).await?;
    }
    Ok(())
}
```

It runs once per relation and write: an attach, a detach or a link edit from
the relation manager, or the links chosen in the record's own form. Writes
that change nothing (attaching what is already linked, detaching what is not,
a link edit to the stored values) do not call it. An error rolls the whole
write back. The engine never updates the parent row for a link change (its
`updated_at` included): do it here when your application needs it; the
parent is already locked by the write.

### Rules checked at boot

- The relation's name is unique snake_case on the resource, and its target
  exists.
- The two key columns exist in the pivot with the types of the two keys.
- One relation per pivot table, and not to the same resource; the target may
  not declare `lock_parent`; and no cycle among resources, counting both
  belongs-to-many (parent before target) and `lock_parent` (parent before
  child): posts → tags with tags → posts is refused. These keep one lock order
  for every writer: the parent (and its own `lock_parent`), then the targets in
  key order, then the pivot rows.
- The pivot field rules [above](#pivot-fields).

The record's history lists link changes (`attached`, `detached`,
`pivot_updated`): which records and the pivot values only to readers who may
list the target (pivot values they may read now), otherwise the relation and
how many.

Attach and detach send no `Idempotency-Key`: sending one again gives the same
links. If the answer is lost, the panel pauses writes to the resource until
the page is reloaded, then shows the links as they are.

### Choosing links in the record's form

A `BelongsToManySelect` puts the links in the record's own create and edit
form, next to its fields (tags on a post, in its Publishing tab):

```rust
fn form() -> Form {
    Form::new().schema(components![
        TextInput::make("title"),
        BelongsToManySelect::make("tags"),   // names the relation, not a field
    ])
}
```

The form shows the linked records (from the relation's list: the target's
scope and policy) and a search over the records the user may attach. Saving
sends what changed, never the whole set:

```json
{"title": "Hello", "tags": {"attach": ["1"], "detach": ["2"]}}
```

(the keys of the tags, as text), so links made by someone else meanwhile, and links to records the user
cannot see, are kept. The record's write checks them as the relation manager
does: `attach_{relation}` and `detach_{relation}` are admitted before the keys
are read (403), then decided on the record as the write stores it (403; the
whole save rolls back); each record to attach must be one the user may view
(locked and checked again, 422 on the input otherwise), records to detach the
user cannot see are left alone. Edits also need `update`, like any field.
The pivot rows, `after_relation_change`, then `after_save` run in the
record's transaction, and the history lists `attached` / `detached` after
`created` / `updated`.

- The input appears when the user may list the target (`view_any`): on
  create when the attach ability is admitted and allowed without a record
  (`Policy::admits`, then `Policy::allows(…, None)`), on edit when they may
  attach or detach on the record, and never on the record's page (the
  relation manager is there). The create form searches
  `GET {admin}/api/resources/{slug}/relations/{relation}/options`, which
  serves only relations the form links.
- Not for the media library (link files with a
  [`Field::media_list`](media.md#media-fields) field instead).
- New links take the pivot columns' database defaults: a relation with a
  `.required()` pivot field cannot have a `BelongsToManySelect` (the panel
  refuses to start); edit pivot columns in the relation manager.
- At most 100 records per `attach` or `detach` (and 500 of one target
  resource per save, over all its relations). A record with more than 100
  visible links shows them read-only in the form: change them on its page.
- Detaching on create is refused (422).

## API

Under `/admin/api/resources/{slug}`:

| Method | Path | Does |
|---|---|---|
| GET | `/records/{key}/relations/{relation}` | the related records (a page, like a list; belongs-to-many adds the readable pivot columns per target) |
| POST | `/records/{key}/relations/{relation}` | create a child (has-many) |
| POST | `/records/{key}/relations/{relation}/attach` | `{keys, pivot?}` |
| POST | `/records/{key}/relations/{relation}/detach` | `{keys}` |
| PATCH | `/records/{key}/relations/{relation}/links/{target}` | the pivot columns of one link |
| GET | `/records/{key}/relations/{relation}/options` | targets to attach (`search`, `keys`): `{value, label, attached}` |
| GET | `/relations/{relation}/options` | targets to attach from a create form |

See the [Admin API](../reference/admin-api.md) for the common rules.

## Related pages

- [Resources](resources.md), [forms](forms.md), [authorization](authorization.md)
- [Database and migrations](../basics/database.md#migrations)
