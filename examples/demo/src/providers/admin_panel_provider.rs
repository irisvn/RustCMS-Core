//! The admin panel at `/admin` (like a Filament panel provider): its
//! resources (`rustcms make:resource`), pages (`make:settings`) and labels.

use rustcms::admin::Requires;
use rustcms::prelude::*;

use crate::admin::resources::{
    category_resource::CategoryResource, post_resource::PostResource, tag_resource::TagResource,
};

pub fn provider() -> AdminProvider {
    AdminProvider::new()
        .brand("demo")
        // The site's name, once set in Settings → Site (runtime branding).
        .branding(crate::admin::branding::SiteBranding)
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
                )
                // The latest posts, shown as their list shows them (dates formatted).
                .widget(
                    rustcms::admin::dashboard::Widget::recent("latest_posts", "posts")
                        .label("Latest posts")
                        .columns(&["published_at"])
                        .sort("-published_at"),
                ),
        )
        .page::<crate::admin::pages::post_stats_page::PostStatsPage>()
        .page::<crate::admin::pages::live_page::LivePage>()
        .page::<crate::admin::pages::react_page::ReactPage>()
        // rustcms:pages
        .slot::<crate::admin::slots::post_summary_slot::PostSummarySlot>()
        // The `demo/color` widget of the category form.
        .extension_script("demo-color", include_str!("../../resources/admin/js/demo/color.js"))
        .extension_style("demo-color", include_str!("../../resources/admin/css/demo/color.css"))
        // The live page (`LivePage`, server-sent events).
        .extension_script("demo-live", include_str!("../../resources/admin/js/demo/live.js"))
        // The React demo page (v2, built by `npm run build` in resources/admin).
        .extension_script_v2(
            "demo-react",
            include_str!("../../resources/admin/js/demo/react.js"),
            Requires { react: "19.3", react_query: "5.104", ui: 2 },
        )
        .extension_style("demo-react", include_str!("../../resources/admin/css/demo/react.css"))
        // rustcms:extensions
        // Vietnamese labels (`cargo run -- lang:missing vi` lists what is missing).
        .translations("vi", include_str!("../../lang/vi.toml"))
}
