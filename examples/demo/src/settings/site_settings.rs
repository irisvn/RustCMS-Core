//! The `site` settings group: stored in the database, edited at
//! `/admin/settings/site` (`crate::admin::pages::site_settings_page`), read
//! with `rustcms::settings::get::<SiteSettings>(&state)`.

use rustcms::settings::{Secret, Settings};
use serde::{Deserialize, Serialize};

// #region site-settings
/// The site's name and what visitors may do.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteSettings {
    pub site_name: String,
    pub tagline: Option<String>,
    pub comments_open: bool,
    pub posts_per_page: i32,
    pub mail_password: Secret,
    /// The site's logo, a file of the media library (`SettingField::media`).
    #[serde(default)]
    pub logo: Option<sea_orm::prelude::Uuid>,
    /// Links shown in the site's footer (`SettingField::repeater`).
    #[serde(default)]
    pub social_links: Vec<SocialLink>,
}

/// One footer link: a row of the `social_links` repeater.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocialLink {
    pub label: String,
    pub url: String,
    /// An icon of the media library (a file upload in the row).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<sea_orm::prelude::Uuid>,
}

impl Default for SiteSettings {
    fn default() -> Self {
        Self {
            site_name: "demo".into(),
            tagline: None,
            comments_open: true,
            posts_per_page: 10,
            mail_password: Secret::default(),
            logo: None,
            social_links: Vec::new(),
        }
    }
}

impl Settings for SiteSettings {
    const GROUP: &'static str = "site";
}
// #endregion site-settings
