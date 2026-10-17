//! Live (`/admin/p/live`): a script page reading server-sent events.

use rustcms::admin::{Ctx, PageView, PanelPage, PanelPageMeta};
use rustcms::{AppResult, async_trait};
use serde_json::{Value, json};

pub struct LivePage;

#[async_trait]
impl PanelPage for LivePage {
    const SLUG: &'static str = "live";

    fn meta() -> PanelPageMeta {
        PanelPageMeta::new("Live").icon("history").group("Content").permission("demo.live")
    }

    fn view() -> PageView {
        PageView::Script("demo/live")
    }

    async fn data(_ctx: &Ctx) -> AppResult<Value> {
        Ok(json!({}))
    }
}
