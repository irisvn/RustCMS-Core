//! Who may do what with posts.

use rustcms::admin::*;

/// Editors with `posts.update_own` may view and edit their own posts.
pub struct PostPolicy;

#[async_trait::async_trait]
impl Policy for PostPolicy {
    async fn allows(&self, ctx: &Ctx, ability: &str, record: Option<&Data>) -> bool {
        let owns = record.is_some_and(|r| r["author_id"].as_str() == Some(ctx.principal.id.as_str()));
        let own = ctx.can("posts.update_own");
        ctx.can(&format!("posts.{ability}"))
            // The list itself (its scope keeps only their posts), then their own records.
            || (ability == "view_any" && own)
            || (matches!(ability, "view" | "update" | "publish") && owns && own)
    }
}
