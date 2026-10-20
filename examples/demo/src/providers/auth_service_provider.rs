//! Authentication and permissions: admin sessions under `/admin`, API tokens
//! elsewhere. Declare permissions beyond each resource's own here.

use rustcms::auth::{ADMIN_GUARD, PermissionArea, PermissionMeta, PermissionPreset};
use rustcms::prelude::*;

pub fn provider(api: &ApiRoutes, admin: &AdminRoutes) -> AuthProvider {
    AuthProvider::new()
        // Editors edit their own posts (see `PostPolicy`).
        .permission(ADMIN_GUARD, "posts.update_own", "posts")
        // The API's permissions come from its routes, the panel pages' from theirs.
        .api_permissions(api)
        .admin_permissions(admin)
        // How the Roles page shows them: an area, labels, and a starting selection.
        .permission_area("live", PermissionArea::new("Live").sort(10))
        .permission_meta(
            ADMIN_GUARD,
            "demo.live",
            PermissionMeta::new().label("Use live demos").description("Ticks, echo socket and hello page").area("live"),
        )
        .permission_meta(
            ADMIN_GUARD,
            "posts.update_own",
            PermissionMeta::new().label("Edit own posts").description("Edit the posts you wrote"),
        )
        .permission_preset(
            "viewer",
            PermissionPreset::new("Viewer").description("Reads posts and categories").permissions([
                "posts.view_any",
                "posts.view",
                "categories.view_any",
                "categories.view",
            ]),
        )
}
