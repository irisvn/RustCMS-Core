//! The public JSON API under `/api/v1`, authenticated with API tokens
//! (`Authorization: Bearer rcms_…`, see `rustcms token:create`). Every route
//! declares its access; `rustcms make:api <Model>` adds controller routes here.

use rustcms::prelude::*;

use crate::http::controllers;

// #region api-routes
/// Every API route, with the access it requires.
pub fn routes() -> ApiRoutes {
    ApiRoutes::new("/api/v1")
        .get("/ping", Access::public(), ping)
        .get("/posts", Access::can("posts.view_any"), controllers::post_controller::index)
        .post("/posts", Access::can("posts.create"), controllers::post_controller::store)
    // rustcms:api-routes
}
// #endregion api-routes

async fn ping() -> Json<serde_json::Value> {
    Json(json!({ "pong": true }))
}
