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

## Not supported

Many-to-many relations (attaching and detaching through a pivot table) are not
supported yet. Use a resource for the pivot table, or an action.
