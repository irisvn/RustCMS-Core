//! Assembles the application (like Laravel's `bootstrap/app.php`).

mod providers;

use rustcms::prelude::*;

// #region build
/// The application for `config`: its providers, then its API routes.
pub fn build(config: Config) -> AppBuilder {
    let mut app = AppBuilder::new(config);
    // #region register-api
    let api = crate::routes::api::routes();
    let admin = crate::routes::admin::routes();
    providers::register(&mut app, &api, &admin);
    crate::jobs::post_digest::schedule(&mut app);
    // Mounted and listed by `rustcms route:list` from the same declarations.
    api.register(&mut app);
    admin.register(&mut app);
    // #endregion register-api
    app
}
// #endregion build
