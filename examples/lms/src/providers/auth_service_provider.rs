//! Authentication and permissions beyond each resource's own.

use rustcms::auth::ADMIN_GUARD;
use rustcms::prelude::*;

use crate::policies::permissions::{ALL_COURSES, ASSIGN_INSTRUCTOR, UPDATE_PROGRESS};

pub fn provider(api: &ApiRoutes) -> AuthProvider {
    AuthProvider::new()
        .permission(ADMIN_GUARD, ALL_COURSES, "courses")
        .permission(ADMIN_GUARD, ASSIGN_INSTRUCTOR, "courses")
        .permission(ADMIN_GUARD, UPDATE_PROGRESS, "enrollments")
        // The API's permissions come from its routes.
        .api_permissions(api)
}
