//! Public paths kept when modules were grouped by role (R1). A compile error
//! here is a broken public path.

#![allow(unused_imports)]

use rustcms_auth::commands::definitions;
use rustcms_auth::credentials::{logout_everywhere, revoke_all};
use rustcms_auth::entities::{
    model_has_permissions, model_has_roles, permission, personal_access_token, role, session,
};
use rustcms_auth::guard::{Auth, Authenticated, CSRF_HEADER, MaybeAuth};
use rustcms_auth::password::hash_blocking;
use rustcms_auth::rate_limit::RateLimiter;
use rustcms_auth::rbac::{Actor, Grants, NewUser, PermissionResolver, RbacService, SyncReport, load_grants, lock};
use rustcms_auth::routes::{confirm_password, routes};
use rustcms_auth::session::{Session, create_for_user, end, lock_lineage, prune_expired};
use rustcms_auth::tokens::issue;
use rustcms_auth::{ADMIN_GUARD, API_GUARD, AuthConfig, AuthProvider, AuthState, BUILT_IN_PERMISSIONS, Gate};

#[test]
fn paths_compile() {}
