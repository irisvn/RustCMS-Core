//! The admin page of [`LmsSettings`] (`/admin/settings/lms`): which fields
//! the form shows and how they are checked.

use rustcms::admin::{SettingField, SettingsMeta, SettingsPage};
use rustcms::async_trait;

use crate::settings::lms_settings::{LmsSettings, MAX_CAPACITY};

#[async_trait]
impl SettingsPage for LmsSettings {
    fn meta() -> SettingsMeta {
        SettingsMeta::new("School").icon("graduation-cap")
    }

    fn fields() -> Vec<SettingField> {
        vec![
            SettingField::text("school_name").required().max_length(120),
            SettingField::integer("default_capacity").help("Between 1 and 10000"),
        ]
    }

    async fn validate(ctx: &rustcms::admin::Ctx, settings: &Self) -> Result<(), rustcms::ValidationErrors> {
        if !(1..=MAX_CAPACITY).contains(&i64::from(settings.default_capacity)) {
            return Err(rustcms::ValidationErrors::field("default_capacity", ctx.t("must be between 1 and 10000")));
        }
        Ok(())
    }
}
