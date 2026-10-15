//! Post stats (`/admin/p/post-stats`): a custom page from a server template.

use rustcms::admin::{Ctx, ListQuery, PageView, PanelPage, PanelPageMeta};
use rustcms::{AppResult, async_trait};
use serde_json::{Value, json};

pub struct PostStatsPage;

#[async_trait]
impl PanelPage for PostStatsPage {
    const SLUG: &'static str = "post-stats";

    fn meta() -> PanelPageMeta {
        PanelPageMeta::new("Post stats").icon("layers").group("Content").permission("posts.view_any")
    }

    fn view() -> PageView {
        PageView::Template(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/resources/views/admin/post_stats.html")))
    }

    /// Posts as the user may list them (editors: their own).
    async fn data(ctx: &Ctx) -> AppResult<Value> {
        let posts = ctx.records("posts", &ListQuery { per_page: Some(100), ..Default::default() }).await?;
        let published = posts.data.iter().filter(|p| p.data.get("published") == Some(&json!(true))).count();
        let latest: Vec<Value> = posts.data.iter().take(5).map(|p| json!({"key": p.key, "title": p.title})).collect();
        Ok(json!({ "total": posts.meta.total, "published": published, "latest": latest }))
    }
}
