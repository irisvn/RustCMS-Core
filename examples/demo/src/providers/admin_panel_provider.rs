//! The admin panel at `/admin` (like a Filament panel provider): its
//! resources (`rustcms make:resource`), pages (`make:settings`) and labels.

use rustcms::prelude::*;

use crate::admin::resources::{category_resource::CategoryResource, post_resource::PostResource};

pub fn provider() -> AdminProvider {
    AdminProvider::new()
        .brand("demo")
        .media()
        .resource::<CategoryResource>()
        .resource::<PostResource>()
        // rustcms:resources
        .settings::<crate::settings::site_settings::SiteSettings>()
        .page::<crate::admin::pages::post_stats_page::PostStatsPage>()
        // rustcms:pages
        .slot::<crate::admin::slots::post_summary_slot::PostSummarySlot>()
        // The `demo/color` widget of the category form.
        .extension_script("demo-color", include_str!("../../resources/admin/js/demo/color.js"))
        .extension_style("demo-color", include_str!("../../resources/admin/css/demo/color.css"))
        // rustcms:extensions
        // Vietnamese labels (`cargo run -- lang:missing vi` lists what is missing).
        .translations("vi", include_str!("../../lang/vi.toml"))
}
