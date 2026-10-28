//! Post stats (`/admin/p/post-stats`): a custom page from a server template.

use rustcms::admin::{Ctx, Filter, FilterOp, ListQuery, PageView, PanelPage, PanelPageMeta};
use rustcms::{AppResult, async_trait};
use serde_json::{Value, json};

// #region page
pub struct PostStatsPage;

#[async_trait]
impl PanelPage for PostStatsPage {
    const SLUG: &'static str = "post-stats";

    fn meta() -> PanelPageMeta {
        PanelPageMeta::new("Post stats").icon("layers").group("Content").permission("posts.view_any")
    }

    fn view() -> PageView {
        rustcms::admin::view!("post_stats.html")
    }

    /// Posts as the user may list them (editors: their own).
    async fn data(ctx: &Ctx) -> AppResult<Value> {
        let posts = ctx.records("posts", &ListQuery { per_page: Some(5), ..Default::default() }).await?;
        // Counted by the list itself: a page of records would undercount.
        let published = Filter { field: "published".into(), op: FilterOp::Eq, value: json!(true) };
        let published = ctx
            .records("posts", &ListQuery { per_page: Some(1), filters: vec![published], ..Default::default() })
            .await?;
        let latest: Vec<Value> = posts.data.iter().map(|p| json!({"key": p.key, "title": p.title})).collect();
        Ok(json!({ "total": posts.meta.total, "published": published.meta.total, "latest": latest }))
    }
}
// #endregion page
