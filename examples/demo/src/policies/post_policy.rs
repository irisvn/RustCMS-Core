//! Who may do what with posts.

use rustcms::admin::*;

/// What editors with `posts.update_own` may do with their own posts: view and
/// edit them, publish, tag them and move them to another category.
const OWN: [&str; 7] = ["view", "update", "publish", "attach_tags", "detach_tags", "update_pivot_tags", "move"];

/// Editors with `posts.update_own` work on their own posts.
pub struct PostPolicy;

#[async_trait::async_trait]
impl Policy for PostPolicy {
    async fn allows(&self, ctx: &Ctx, ability: &str, record: Option<&Data>) -> bool {
        let owns = record.is_some_and(|r| r["author_id"].as_str() == Some(ctx.principal.id.as_str()));
        let own = ctx.can("posts.update_own");
        ctx.can(&format!("posts.{ability}"))
            // The list itself (its scope keeps only their posts), then their own records.
            || (ability == "view_any" && own)
            || (OWN.contains(&ability) && owns && own)
    }

    /// Refused before any record (or request body) is read for users who
    /// could not do it to any post: no permission, and not an owner's ability.
    async fn admits(&self, ctx: &Ctx, ability: &str) -> bool {
        ctx.can(&format!("posts.{ability}"))
            || ((ability == "view_any" || OWN.contains(&ability)) && ctx.can("posts.update_own"))
    }
}
