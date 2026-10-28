//! Runtime branding: the panel shows the site's name (Settings → Site) instead
//! of the static `brand("demo")`. An application that saves its branding on
//! its own page calls `AdminBranding::invalidate` after saving; here the short
//! `[admin] branding_ttl_secs` (config/admin.toml) picks up the change.

use rustcms::admin::{Branding, BrandingSource};
use rustcms::{AppError, AppResult, AppState, async_trait};

use crate::settings::site_settings::SiteSettings;

// #region site-branding
pub struct SiteBranding;

#[async_trait]
impl BrandingSource for SiteBranding {
    async fn load(&self, app: &AppState) -> AppResult<Option<Branding>> {
        let site = rustcms::settings::get::<SiteSettings>(app).await?;
        if site.site_name == SiteSettings::default().site_name {
            return Ok(None);
        }
        // A name the panel refuses (over 64 characters) shows the static brand.
        let Ok(branding) = Branding::new().name(&site.site_name) else { return Ok(None) };
        Ok(Some(branding.primary("#7c3aed").map_err(AppError::internal)?))
    }
}
// #endregion site-branding
