//! The `lms` settings group: stored in the database, edited at
//! `/admin/settings/lms` (`crate::admin::pages::lms_settings_page`), read
//! with `rustcms::settings::get::<LmsSettings>(&state)`.

use rustcms::settings::Settings;
use serde::{Deserialize, Serialize};

/// Largest capacity a course may have.
pub const MAX_CAPACITY: i64 = 10_000;

/// The school, and defaults for new courses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LmsSettings {
    /// Shown in welcomes.
    pub school_name: String,
    /// Capacity of the courses the course seeder creates.
    pub default_capacity: i32,
}

impl Default for LmsSettings {
    fn default() -> Self {
        Self { school_name: "rustcms Academy".into(), default_capacity: 30 }
    }
}

impl Settings for LmsSettings {
    const GROUP: &'static str = "lms";
}
