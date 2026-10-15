//! Beside a post: whether readers see it.

use rustcms::admin::{Ctx, Envelope, PageView, PanelSlot, Placement, SlotMeta};
use rustcms::{AppResult, async_trait};
use serde_json::{Value, json};

pub struct PostSummarySlot;

#[async_trait]
impl PanelSlot for PostSummarySlot {
    const NAME: &'static str = "post-summary";

    fn placement() -> Placement {
        Placement::RecordAside("posts")
    }

    fn meta() -> SlotMeta {
        SlotMeta::new().label("Visibility").permission("posts.view_any")
    }

    fn view() -> PageView {
        rustcms::admin::view!("post_summary.html")
    }

    async fn data(_: &Ctx, record: &Envelope) -> AppResult<Value> {
        Ok(json!({ "published": record.data.get("published") == Some(&json!(true)) }))
    }
}
