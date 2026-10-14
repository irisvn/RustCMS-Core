//! The public JSON API under `/api/v1`, authenticated with API tokens
//! (`Authorization: Bearer rcms_…`, see `rustcms token:create`).
//! `rustcms make:api <Model>` adds a module per model here.

use rustcms::prelude::*;

pub mod posts;
// rustcms:api-mods

/// Every API route, with the access it requires.
pub fn routes() -> ApiRoutes {
    ApiRoutes::new("/api/v1").get("/ping", Access::public(), ping).nest("/posts", posts::routes())
    // rustcms:api-routes
}

async fn ping() -> Json<serde_json::Value> {
    Json(json!({ "pong": true }))
}
