//! Tables owned by the auth layer (created by `rustcms-migration`).

pub mod jwt_family;
pub mod jwt_refresh_token;
pub mod model_has_permissions;
pub mod model_has_roles;
pub mod permission;
pub mod personal_access_token;
pub mod role;
pub mod role_has_permissions;
pub mod session;

/// `model_type` used for [`rustcms_core::models::user`] in polymorphic pivots.
pub const USER_MODEL: &str = "user";
