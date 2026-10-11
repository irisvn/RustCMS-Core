//! Application assembly: providers, services and routes.

use rustcms::prelude::*;

pub fn build(config: Config) -> AppBuilder {
    let mut app = AppBuilder::new(config);
    // Admin sessions under /admin, API tokens elsewhere; declare your permissions here.
    app.provider(AuthProvider::new());
    app.routes(|router| router.route("/api/v1/ping", get(ping)));
    app
}

async fn ping() -> Json<serde_json::Value> {
    Json(json!({ "pong": true }))
}
