//! Application assembly: providers, services and routes.

use rustcms::prelude::*;

use crate::admin::{CategoryResource, PostResource};
use crate::seed::Seed;

pub fn build(config: Config) -> AppBuilder {
    let mut app = AppBuilder::new(config);
    // Admin sessions under /admin, API tokens elsewhere; declare your permissions here.
    app.provider(AuthProvider::new().permission(rustcms::auth::ADMIN_GUARD, "posts.update_own", "posts"));
    // Resources appear in the admin panel at /admin.
    app.provider(AdminProvider::new().brand("demo").resource::<CategoryResource>().resource::<PostResource>());
    app.command(Seed);
    app.routes(|router| router.route("/api/v1/ping", get(ping)));
    app
}

async fn ping() -> Json<serde_json::Value> {
    Json(json!({ "pong": true }))
}
