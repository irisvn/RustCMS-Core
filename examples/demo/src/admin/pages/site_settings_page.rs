//! The admin page of [`SiteSettings`] (`/admin/settings/site`): which fields
//! the form shows and how they are checked.

use rustcms::admin::{SettingField, SettingsMeta, SettingsPage};
use rustcms::async_trait;

use crate::settings::site_settings::SiteSettings;

// #region settings-page
#[async_trait]
impl SettingsPage for SiteSettings {
    fn meta() -> SettingsMeta {
        SettingsMeta::new("Site").icon("settings")
    }

    fn fields() -> Vec<SettingField> {
        vec![
            SettingField::text("site_name").required().max_length(80),
            SettingField::text("tagline").nullable().max_length(160),
            SettingField::boolean("comments_open"),
            SettingField::integer("posts_per_page").help("Between 1 and 100"),
            SettingField::secret("mail_password").label("Mail password"),
            SettingField::media("logo").accept(&["image/*"]).help("An image of the media library"),
        ]
    }

    async fn validate(_ctx: &rustcms::admin::Ctx, settings: &Self) -> Result<(), rustcms::ValidationErrors> {
        if !(1..=100).contains(&settings.posts_per_page) {
            return Err(rustcms::ValidationErrors::field("posts_per_page", "is out of range"));
        }
        Ok(())
    }
}
// #endregion settings-page
