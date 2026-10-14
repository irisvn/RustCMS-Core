//! `/api/v1/posts`: published posts, and drafts created through the API.

use rustcms::prelude::*;

mod handlers;
mod json;
mod requests;

/// The routes of posts.
pub fn routes() -> ApiRoutes {
    ApiRoutes::scope().get("/", Access::can("posts.view_any"), handlers::list).post(
        "/",
        Access::can("posts.create"),
        handlers::create,
    )
}
