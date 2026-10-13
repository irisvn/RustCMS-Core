# Resources

A resource exposes one SeaORM entity in the admin panel. Register resources
with the panel (after `AuthProvider`):

```rust,ignore
app.provider(AdminProvider::new().brand("Blog").resource::<PostResource>().resource::<CategoryResource>());
```

Each resource declares its permissions (`posts.view_any`, `posts.view`,
`posts.create`, `posts.update`, `posts.delete`, plus action abilities) for
`rustcms permission:sync`.

```rust,ignore
use rustcms_admin::{Field, KeyStrategy, Resource, ResourceMeta};

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

## Forms and tables

`fields()` define data and permissions; `form()` and `table()` only describe
presentation. Leave them out to get one input per field and the first six
fields as columns.

```rust,ignore
fn form() -> Form {
    Form::new().columns(2).schema(components![
        Section::make("Content").schema(components![
            TextInput::make("title").placeholder("A catchy title"),
            Textarea::make("excerpt").column_span(2),
        ]),
        SelectInput::make("category_id"),          // options from the `belongs_to` target
        Toggle::make("published"),
        DateTimePicker::make("published_at"),
    ])
}

fn table() -> Table {
    Table::new()
        .columns(vec![TextColumn::make("title"), BooleanColumn::make("published"), DateColumn::make("published_at")])
        .filters(vec![TernaryFilter::make("published"), SelectFilter::make("category_id")])
        .default_sort("-published_at")
}
```

| Inputs | `TextInput` (`.email()`, `.password()`, `.url()`), `Textarea`, `NumberInput`, `SelectInput`, `Toggle`, `Checkbox`, `CheckboxList`, `DatePicker`, `DateTimePicker`, `Hidden` |
|---|---|
| Layouts | `Section::make(label)`, `Grid::make(columns)` |
| Columns | `TextColumn`, `BadgeColumn` (`.color(value, "success")`), `BooleanColumn`, `DateColumn` |
| Filters | `SelectFilter`, `TernaryFilter`, `DateRangeFilter` (field must be `.filterable()`) |

Misconfiguration (an input for an unknown field, a filter on a non-filterable
field, a default sort on a non-sortable field) panics at boot.

`SelectInput` is named so it does not clash with `sea_orm::Select` in `scope()`.

## Actions

```rust,ignore
fn actions() -> Vec<Action<post::Entity>> {
    vec![Action::<post::Entity>::new("publish", |_ctx, txn, posts, _data| {
        Box::pin(async move {
            for post in posts {
                let mut post = post.into_active_model();
                post.published = Set(true);
                post.update(txn).await?;
            }
            Ok(())
        })
    })
    .label("Publish")
    .bulk()
    .requires_confirmation("Publish the selected posts?")]
}
```

Name the entity (`Action::<post::Entity>::new`) so the closure's types are
inferred. Each selected record is checked by the policy with the action's
ability (default: its name, or `.ability("update")`); denied records are
reported, not processed, and envelopes tell the client per record. Add
`.requires_permission("posts.publish")` to refuse the action outright (403, and
hidden from the schema) for users without that permission.

## Security model

- **Deny by default.** Without a custom policy, every ability requires the
  permission `{slug}.{ability}` (`posts.update`).
- **Columns not declared as fields** are never read or written by clients.
- **Field rules** (`readable`, `creatable`, `updatable`) are evaluated on the
  server for every request. Client-side visibility never grants write access.
- **`guarded()` fields** are never written from input; hooks may set them.
- **Lists never show records the policy denies `view`.** With a record-dependent
  policy (the default for custom policies), lists check each matching record
  and count only visible ones (up to 10,000 matching rows, then 422). Put large
  visibility rules in `scope()` and return `false` from
  `Policy::view_depends_on_record` to keep SQL pagination.
- **`scope()`** restricts which records exist for the user. It applies to lists,
  lookups, writes, actions and relationship options and labels — and to the
  response of a write: a record the write moved out of the user's scope comes
  back as its key only.
- **No existence probing.** Requests by key (`find`, schema of a record, update,
  delete, actions, options of a record) first ask `Policy::admits(ctx, ability)`;
  refused, they answer 403 whether or not the key exists. The default policy
  admits holders of `{slug}.{ability}`. Custom policies admit everyone by default
  (then a missing key is 404 and a denied record 403): override `admits` when an
  ability needs a permission regardless of the record.
- **Relationships:** a `belongs_to` key sent by a client must be visible to that
  user in the target resource, otherwise the request fails with 422.

## Policies that query the database

Use `ctx.conn()` in policies (and hooks): during a write it is the write's own
transaction, so the policy sees the state being written and never waits for a
second pooled connection; elsewhere it is the pool.

```rust,ignore
async fn allows(&self, ctx: &Ctx, ability: &str, record: Option<&Data>) -> bool {
    let row = ctx.conn().query_one_raw(/* membership lookup */).await;
    // …
}
```

## Write pipeline

All steps run in one transaction:

1. reject undeclared or non-writable keys (422),
2. type-check values against the [wire format](../admin-protocol.md),
3. check relationship keys against the target scope,
4. `mutate_before_create` / `mutate_before_update` hooks,
5. re-check limits (`max_length`) on the final values, so hooks cannot bypass them,
6. write (database constraints are mapped to field errors with `ResourceMeta::unique`),
7. `after_save` hook — an error rolls everything back.

## Primary keys

| Strategy                  | Behaviour                                                         |
|---------------------------|-------------------------------------------------------------------|
| `KeyStrategy::DbGenerated`  | default; auto-increment or column default, never written       |
| `KeyStrategy::AppGenerated` | generated by server code before insert (e.g. UUID v7)          |
| `KeyStrategy::Natural`      | supplied on create if its field is creatable; never updated    |

Composite primary keys are not supported in 0.1.
