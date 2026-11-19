//! The admin page of [`SiteSettings`] (`/admin/settings/site`): which fields
//! the form shows and how they are checked.

use rustcms::admin::{FileUpload, Repeater, SettingField, SettingsMeta, SettingsPage, TextInput, components};
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
            SettingField::media("logo")
                .accept(&["image/*"])
                .collection("branding")
                .help("An image of the media library"),
            // #region repeater
            // Rows of inputs in one value (ADR 0040): each row's rules are on its inputs.
            SettingField::repeater(
                Repeater::make("social_links")
                    .label("Footer links")
                    .schema(components![
                        TextInput::make("label").required().max_length(40),
                        TextInput::make("url").url().required().max_length(200),
                        // A file in a row: its rules are the input's (images, small, in the branding collection).
                        FileUpload::make("icon").accept(&["image/*"]).max_size(512 * 1024).collection("branding"),
                    ])
                    .max_items(5)
                    .reorderable()
                    .item_label("{label}")
                    .add_action_label("Add a link"),
            ),
            // #endregion repeater
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
