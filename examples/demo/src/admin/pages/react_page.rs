//! React demo (`/admin/p/react-demo`, and its sub-pages `items/{id}`): a v2
//! script page built with the SDK (`resources/admin/src/demo/react.tsx`).

use rustcms::admin::{Ctx, PageView, PanelPage, PanelPageMeta};
use rustcms::{AppResult, async_trait};
use serde_json::{Value, json};

// #region react-page
pub struct ReactPage;

#[async_trait]
impl PanelPage for ReactPage {
    const SLUG: &'static str = "react-demo";

    fn meta() -> PanelPageMeta {
        PanelPageMeta::new("React demo").icon("layers").group("Content").permission("demo.live")
    }

    fn view() -> PageView {
        PageView::Script("demo/react")
    }

    async fn data(ctx: &Ctx) -> AppResult<Value> {
        // The sub-path (`items/2`) is canonical; this page's permission covers it.
        Ok(json!({
            "path": ctx.page_path(),
            "items": [{ "id": 1, "name": "First" }, { "id": 2, "name": "Second" }],
        }))
    }
}
// #endregion react-page
