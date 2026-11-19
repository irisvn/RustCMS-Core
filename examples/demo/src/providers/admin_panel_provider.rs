//! The admin panel at `/admin` (like a Filament panel provider): its
//! resources (`rustcms make:resource`), pages (`make:settings`) and labels.

use rustcms::admin::Requires;
use rustcms::admin::rich_text::{Attr, RichTextNode};
use rustcms::prelude::*;

use crate::admin::resources::{
    category_resource::CategoryResource, post_resource::PostResource, tag_resource::TagResource,
};

// #region provider
pub fn provider() -> AdminProvider {
    AdminProvider::new()
        // #region branding
        .brand("demo")
        // The site's name, once set in Settings → Site (runtime branding).
        .branding(crate::admin::branding::SiteBranding)
        // #endregion branding
        // #region media
        .media()
        .media_hook(crate::admin::media_hooks::MediaChanges)
        // #region media-collections
        // Collections sort the library: files uploaded through a field that
        // names one go into it (`Field::collection`).
        .media_collection("post-covers", "Post covers")
        .media_collection("post-content", "Post content")
        .media_collection("branding", "Branding")
        // #endregion media-collections
        // #endregion media
        // #region rich-text-nodes
        // Elements a post's content may hold (rich text version 3): a keyboard
        // shortcut inside text, drawn by the React extension below, and a
        // reference to a note as a block, shown as a placeholder.
        .rich_text_node(
            RichTextNode::inline_atom("app.kbd")
                .label("Keyboard shortcut")
                .attr("keys", Attr::text().min(1).max(40))
                .text(|attrs| attrs.text("keys").to_owned()),
        )
        .rich_text_node(
            RichTextNode::block_atom("app.note_ref")
                .label("Note reference")
                .attr("number", Attr::integer().range(1..=999))
                .attr("tone", Attr::one_of(["info", "warn"]).default("info"))
                .text(|attrs| format!("Note {}", attrs.integer("number").unwrap_or_default())),
        )
        // #endregion rich-text-nodes
        .resource::<CategoryResource>()
        .resource::<crate::admin::resources::category_section_resource::CategorySectionResource>()
        .resource::<crate::admin::resources::category_section_resource::SectionLinkResource>()
        .resource::<PostResource>()
        .resource::<TagResource>()
        .resource::<crate::admin::resources::integration_resource::IntegrationResource>()
        // rustcms:resources
        // #region settings
        .settings::<crate::settings::site_settings::SiteSettings>()
        .mail_settings()
        // #endregion settings
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
        // #region color-extension
        // The `demo/color` widget of the category form.
        .extension_script("demo-color", include_str!("../../resources/admin/js/demo/color.js"))
        .extension_style("demo-color", include_str!("../../resources/admin/css/demo/color.css"))
        // #endregion color-extension
        // The live page (`LivePage`, server-sent events).
        .extension_script("demo-live", include_str!("../../resources/admin/js/demo/live.js"))
        // #region react-extension
        // The React demo page and the categories' swatch entry (v2, built by
        // `npm run build` in resources/admin).
        .extension_script_v2(
            "demo-react",
            include_str!("../../resources/admin/js/demo/react.js"),
            Requires { react: "19.3", react_query: "5.104", ui: 4 },
        )
        .extension_style("demo-react", include_str!("../../resources/admin/css/demo/react.css"))
        // #endregion react-extension
        // rustcms:extensions
        // #region translations
        // Vietnamese labels (`cargo run -- lang:missing vi` lists what is missing).
        .translations("vi", include_str!("../../lang/vi.toml"))
    // #endregion translations
}
// #endregion provider
