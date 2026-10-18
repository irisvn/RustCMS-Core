//! The admin panel at `/admin` (like a Filament panel provider): its
//! resources (`rustcms make:resource`), pages (`make:settings`) and labels.

use rustcms::prelude::*;

use crate::admin::resources::{
    category_resource::CategoryResource, post_resource::PostResource, tag_resource::TagResource,
};

pub fn provider() -> AdminProvider {
    AdminProvider::new()
        .brand("demo")
        .media()
        .resource::<CategoryResource>()
        .resource::<PostResource>()
        .resource::<TagResource>()
        .resource::<crate::admin::resources::integration_resource::IntegrationResource>()
        // rustcms:resources
        .settings::<crate::settings::site_settings::SiteSettings>()
        .mail_settings()
        .dashboard(
            rustcms::admin::dashboard::Dashboard::new()
                .widget(rustcms::admin::dashboard::Widget::stat("posts", "posts").label("Posts"))
                .widget(
                    rustcms::admin::dashboard::Widget::chart("posts_per_week", "posts", "published_at")
                        .label("Published per week")
                        .per(rustcms::admin::dashboard::Bucket::Week)
                        .last(8)
                        .span(2),
                ),
        )
        .page::<crate::admin::pages::post_stats_page::PostStatsPage>()
        .page::<crate::admin::pages::live_page::LivePage>()
        // rustcms:pages
        .slot::<crate::admin::slots::post_summary_slot::PostSummarySlot>()
        // The `demo/color` widget of the category form.
        .extension_script("demo-color", include_str!("../../resources/admin/js/demo/color.js"))
        .extension_style("demo-color", include_str!("../../resources/admin/css/demo/color.css"))
        // The live page (`LivePage`, server-sent events).
        .extension_script("demo-live", include_str!("../../resources/admin/js/demo/live.js"))
        // rustcms:extensions
        // Vietnamese labels (`cargo run -- lang:missing vi` lists what is missing).
        .translations("vi", include_str!("../../lang/vi.toml"))
}
