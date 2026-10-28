//! The application's service providers, in order (like `bootstrap/providers.php`).

use rustcms::prelude::*;

use crate::providers;

// #region register
pub fn register(app: &mut AppBuilder, api: &ApiRoutes, admin: &AdminRoutes) {
    app.provider(providers::auth_service_provider::provider(api, admin));
    // Uploads under `storage/app` (see `[storage]`); the panel's media library uses them.
    app.provider(rustcms::storage::StorageProvider);
    app.provider(rustcms::mail::MailProvider);
    app.provider(providers::admin_panel_provider::provider());
    app.provider(providers::app_service_provider::AppServiceProvider);
}
// #endregion register
