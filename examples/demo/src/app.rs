//! Application assembly: providers, services and routes.

use rustcms::prelude::*;

pub fn build(config: Config) -> AppBuilder {
    let mut app = AppBuilder::new(config);
    // Admin sessions under /admin, API tokens elsewhere; declare your permissions here.
    app.provider(AuthProvider::new());
    // Resources appear in the admin panel at /admin: `.resource::<PostResource>()`.
    app.provider(AdminProvider::new().brand("demo"));
    app.routes(|router| router.route("/api/v1/ping", get(ping)));
    app
}

async fn ping() -> Json<serde_json::Value> {
    Json(json!({ "pong": true }))
}
