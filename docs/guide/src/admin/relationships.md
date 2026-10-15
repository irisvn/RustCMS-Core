# Relationships

A resource can list the records of another resource that point to it, on its
own view page, and create new ones there: a **has-many relation**. The child
keeps a `belongs_to` field to the parent.

```rust,ignore
impl Resource for CategoryResource {
    // …
    fn relations() -> Vec<Relation> {
        vec![Relation::has_many("posts", "posts", "category_id").label("Posts")]
    }
}

impl Resource for PostResource {
    fn fields() -> Vec<Field> {
        vec![
            // …
            Field::column(post::Column::CategoryId).belongs_to("categories"),
        ]
    }
}
```

`Relation::has_many(name, target, field)`: `name` is the relation's id in URLs
(snake_case, unique on the resource), `target` the child resource's slug and
`field` the child's foreign key. The registry checks, when the panel starts,
that the target exists, that `field` is a field of it declared
`belongs_to("<this slug>")`, that its column kind matches this resource's
primary key and that it is not the child's own key; anything else panics with
the fix.

## What users see

On the parent's view page, each relation is a section with a compact table of
the children: the target's table columns (without the foreign key), search and
paging, and a **View** link per row. Edits and deletes happen on the child's
own pages. **New** opens the child's create form for this parent: the foreign
key input is not shown, the server sets it, and the form returns to the parent
when saved.

## Authorization

Nothing is bypassed: a relation is the target's own list and create, with one
extra condition (`field = parent key`).

- **Listing** needs `view` on the parent resource (`admits`) and the target's
  `view_any`, checked before the parent is looked up: without them the answer is
  403 whatever the key. Then the parent must be visible to the user (scope,
  policy, not trashed), otherwise 404, exactly like `GET …/records/{key}`. The
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
[write-in-progress rule](../reference/limitations.md): the child's `belongs_to`
check sees the parent as it was when the check ran.

## Idempotency

Relation creates accept `Idempotency-Key` like any create. The claim is bound to
the relation and the parent: the same key sent to another parent (or to the
target's own create route) is a separate claim, not a replay. A committed retry
is answered even if the parent was hidden, trashed or deleted since, like any
replay.

## Belongs-to-many (pivot tables)

Records linked through a **pivot table** (posts ↔ tags), with optional columns
on each link (`position`):

```rust,ignore
// src/models/post_tag.rs: post_id, tag_id (primary key over both), position
fn relations() -> Vec<Relation> {
    vec![
        Relation::belongs_to_many("tags", "tags", post_tag::Column::PostId, post_tag::Column::TagId)
            .pivot(vec![Field::column(post_tag::Column::Position)])
            .label("Tags"),
    ]
}
```

```rust,ignore
// The migration: unique over both keys, rows deleted with either record.
Table::create().table(PostTags::Table)
    .col(ColumnDef::new(PostTags::PostId).big_integer().not_null())
    .col(ColumnDef::new(PostTags::TagId).integer().not_null())
    .col(ColumnDef::new(PostTags::Position).integer().not_null().default(0))
    .primary_key(Index::create().col(PostTags::PostId).col(PostTags::TagId))
    .foreign_key(ForeignKey::create().from(PostTags::Table, PostTags::PostId)
        .to(Posts::Table, Posts::Id).on_delete(ForeignKeyAction::Cascade))
    .foreign_key(ForeignKey::create().from(PostTags::Table, PostTags::TagId)
        .to(Tags::Table, Tags::Id).on_delete(ForeignKeyAction::Cascade))
```

The record's page lists the linked records (the target's table, scope and
policy, search and pages) with the readable pivot columns, and offers, as
allowed:

- **Attach**: choose records of the target (its options: in the user's scope,
  allowed to view; already linked ones are marked) and the pivot columns set
  on new links. Attaching a record already linked keeps that link as it is.
- **Detach**: the selected links. Links to records the user cannot see are
  never touched.
- **Edit link**: the pivot columns the user may update.

### Permissions

Writes need an ability of the **parent** record:
`attach_{relation}`, `detach_{relation}` and `update_pivot_{relation}` (only
with pivot fields). With the default policy that is `posts.attach_tags`,
`posts.detach_tags`, `posts.update_pivot_tags`, declared for
`permission:sync`. A policy of your own receives them like any ability; if it
decides per record (`allows` with the record), also override
[`Policy::admits`](authorization.md) for them, so users without the
permission are refused before the request body is read.

Every write checks, in this order: the ability is admitted (403, whatever the
body or the record); the body (422: 1 to 100 keys, pivot columns by their
field rules; undeclared columns are refused); then, in one transaction, the
parent record (in scope: 404; policy: 403) locked, and the targets the user may
view locked and checked again. Targets not found, outside the user's scope,
trashed or not viewable come back in `missing` and are left alone. The answer
is `{processed, missing}`.

Pivot columns are [fields](resources.md#fields): `readable`, `creatable`,
`updatable`, `required`, `max_length` apply, values follow the
[wire format](../reference/wire-format.md); columns not declared never leave
the server. Pivot columns not declared must be nullable or have a default (the
panel refuses to start otherwise).

### Rules checked at boot

- The two key columns exist in the pivot with the types of the two keys.
- One relation per pivot table, and not to the same resource; the target may
  not declare `lock_parent`. These keep one lock order for every writer:
  the parent (and its own `lock_parent`), then the targets in key order, then
  the pivot rows.

Attach and detach send no `Idempotency-Key`: sending one again gives the same
links. If the answer is lost, the panel pauses writes to the resource until
the page is reloaded, then shows the links as they are.

Records are linked only from the relation manager for now; a multi-select of
links in the record's own form is not available yet.
