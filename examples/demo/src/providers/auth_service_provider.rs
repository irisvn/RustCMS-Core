//! Authentication and permissions: admin sessions under `/admin`, API tokens
//! elsewhere. Declare permissions beyond each resource's own here.

use rustcms::auth::ADMIN_GUARD;
use rustcms::prelude::*;

pub fn provider(api: &ApiRoutes, admin: &AdminRoutes) -> AuthProvider {
    AuthProvider::new()
        // Editors edit their own posts (see `PostPolicy`).
        .permission(ADMIN_GUARD, "posts.update_own", "posts")
        // The API's permissions come from its routes, the panel pages' from theirs.
        .api_permissions(api)
        .admin_permissions(admin)
}
