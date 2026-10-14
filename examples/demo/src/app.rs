//! Application assembly: providers, services and routes.

use rustcms::prelude::*;

use crate::seed::Seed;

pub fn build(config: Config) -> AppBuilder {
    let mut app = AppBuilder::new(config);
    // Admin sessions under /admin, API tokens elsewhere; declare your permissions here.
    let mut auth = AuthProvider::new().permission(rustcms::auth::ADMIN_GUARD, "posts.update_own", "posts");
    for permission in crate::api::PERMISSIONS {
        auth = auth.permission(rustcms::auth::API_GUARD, permission, "posts");
    }
    app.provider(auth);
    // Uploads under `storage/app` (see `[storage]`); the panel's media library uses them.
    app.provider(rustcms::storage::StorageProvider);
    // Resources appear in the admin panel at /admin.
    app.provider(
        AdminProvider::new()
            .brand("demo")
            .media()
            .resource::<crate::admin::category::CategoryResource>()
            .resource::<crate::admin::post::PostResource>()
            // rustcms:resources
            // The `demo/color` widget of the category form.
            .extension_script("demo-color", include_str!("../assets/admin/demo/color.js"))
            .extension_style("demo-color", include_str!("../assets/admin/demo/color.css")),
    );
    app.command(Seed);
    app.routes(|router| router.route("/api/v1/ping", get(ping)));
    app.describe_routes("app", [(Method::GET, "/api/v1/ping".to_owned())]);
    crate::api::register(&mut app);
    app
}

async fn ping() -> Json<serde_json::Value> {
    Json(json!({ "pong": true }))
}
