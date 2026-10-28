# Authorization

The admin panel is **deny by default**. Every request, from the panel or any
other client, is checked on the server; what the panel hides is never what
protects your data.

## Rules

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


## Policies

Without a policy, every ability requires the permission `{slug}.{ability}`.
A custom policy decides per record:

```rust
use rustcms::async_trait;

pub struct PostPolicy;

#[async_trait]
impl Policy for PostPolicy {
    async fn allows(&self, ctx: &Ctx, ability: &str, record: Option<&Data>) -> bool {
        let own = record.and_then(|r| r.get("author_id")) == Some(&json!(ctx.principal.id));
        match ability {
            "update" | "delete" => ctx.can(&format!("posts.{ability}")) || (own && ctx.can("posts.update_own")),
            other => ctx.can(&format!("posts.{other}")),
        }
    }
}

// in the resource
fn policy() -> Arc<dyn Policy> {
    Arc::new(PostPolicy)
}
```

`rustcms make:policy PostPolicy` writes a starting point. Declare extra
permissions such as `posts.update_own` on the `AuthProvider`
(`.permission(ADMIN_GUARD, "posts.update_own", "posts")`).

## Scopes

`scope()` restricts which records exist for a user, everywhere (lists, reads,
writes, actions, relationship options and labels):

```rust
fn scope(query: Select<post::Entity>, ctx: &Ctx) -> Select<post::Entity> {
    if ctx.can("posts.view_any") {
        query
    } else {
        query.filter(post::Column::AuthorId.eq(ctx.principal.id.parse::<i64>().unwrap_or(-1)))
    }
}
```

## Policies that query the database

Use `ctx.conn()` in policies (and hooks): during a write it is the write's own
transaction, so the policy sees the state being written and never waits for a
second pooled connection; elsewhere it is the pool.

```rust
async fn allows(&self, ctx: &Ctx, ability: &str, record: Option<&Data>) -> bool {
    let row = ctx.conn().query_one_raw(/* membership lookup */).await;
    // …
}
```

