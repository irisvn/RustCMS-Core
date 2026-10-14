# Actions

Actions run on one record (row actions) or on the selected records (bulk
actions), with the same authorization as any write.

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

