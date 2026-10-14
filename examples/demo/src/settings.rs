//! Settings groups edited at `/admin/settings/<group>` (read them with
//! `rustcms::settings::get::<SiteSettings>(&state)`).

use rustcms::admin::{SettingField, SettingsMeta, SettingsPage};
use rustcms::async_trait;
use rustcms::settings::{Secret, Settings};
use serde::{Deserialize, Serialize};

/// The site's name and what visitors may do.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteSettings {
    pub site_name: String,
    pub tagline: Option<String>,
    pub comments_open: bool,
    pub posts_per_page: i32,
    pub mail_password: Secret,
}

impl Default for SiteSettings {
    fn default() -> Self {
        Self {
            site_name: "demo".into(),
            tagline: None,
            comments_open: true,
            posts_per_page: 10,
            mail_password: Secret::default(),
        }
    }
}

impl Settings for SiteSettings {
    const GROUP: &'static str = "site";
}

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
        ]
    }

    async fn validate(_ctx: &rustcms::admin::Ctx, settings: &Self) -> Result<(), rustcms::ValidationErrors> {
        if !(1..=100).contains(&settings.posts_per_page) {
            return Err(rustcms::ValidationErrors::field("posts_per_page", "is out of range"));
        }
        Ok(())
    }
}
