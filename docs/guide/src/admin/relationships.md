# Relationships

A resource can list the records of another resource that point to it, on its
own view page, and create new ones there: a **has-many relation**. The child
keeps a `belongs_to` field to the parent.

```rust
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

```rust
// src/models/post_tag.rs: post_id, tag_id (primary key over both), position
fn relations() -> Vec<Relation> {
    vec![
        Relation::belongs_to_many("tags", "tags", post_tag::Column::PostId, post_tag::Column::TagId)
            .pivot(vec![Field::column(post_tag::Column::Position)])
            .label("Tags"),
    ]
}
```

```rust
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

Rows also offer the target resource's [actions](actions.md#placement-in-lists),
as in has-many tables: the server's abilities for each record decide, and
running one is authorized as from the target's list.

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
the server. Pivot columns not declared must be nullable or have a default; a
mandatory one is a `.required()` pivot field (the panel refuses to start
otherwise), and users who may not write it cannot attach (403). Attaching sends
only the pivot columns the user filled in (the others keep their database
defaults); editing a link sends only what changed, and a change to the values
already stored writes and records nothing.

### Reacting to link changes

`Resource::after_relation_change` runs in the transaction of every write
that changed links of the record, after the pivot rows are written, with the
parent record and what changed (target keys, canonical, sorted):

```rust
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

- The two key columns exist in the pivot with the types of the two keys.
- One relation per pivot table, and not to the same resource; the target may
  not declare `lock_parent`; and no cycle among resources, counting both
  belongs-to-many (parent before target) and `lock_parent` (parent before
  child): posts → tags with tags → posts is refused. These keep one lock order
  for every writer: the parent (and its own `lock_parent`), then the targets in
  key order, then the pivot rows.

The record's history lists link changes (`attached`, `detached`,
`pivot_updated`): which records and the pivot values only to readers who may
list the target (pivot values they may read now), otherwise the relation and
how many.

Attach and detach send no `Idempotency-Key`: sending one again gives the same
links. If the answer is lost, the panel pauses writes to the resource until
the page is reloaded, then shows the links as they are.

### Choosing links in the record's form

A `BelongsToManySelect` puts the links in the record's own create and edit
form, next to its fields (tags on a post):

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
{"title": "Hello", "tags": {"attach": ["rust"], "detach": ["sql"]}}
```

so links made by someone else meanwhile, and links to records the user
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
