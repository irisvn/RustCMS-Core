//! Authentication and authorization for rustcms.
//!
//! - Two guards: **admin** (server-side session cookie, CSRF synchronizer token,
//!   strict `Origin` check) and **api** (personal access tokens with abilities).
//! - Roles and permissions per guard, resolved with a short cache and changed
//!   only through [`RbacService`], which prevents privilege escalation.
//! - Credential revocation on sign-out everywhere, password change and disabling.
//!
//! Register [`AuthProvider`], then use [`Auth`] in handlers.
#![warn(missing_docs)]

pub mod config;
pub mod console;
pub mod entities;
pub mod guards;
pub mod http;
mod provider;
pub mod rbac;
pub mod recovery;
pub mod state;
pub mod support;

// Module paths kept from before the layout by role (R1).
pub use console::commands;
pub use guards::{credentials, guard, session, tokens};
pub use http::routes;
pub use support::{audit, password, rate_limit, secret};

pub use config::AuthConfig;
pub use guard::{Auth, Authenticated, CSRF_HEADER, MaybeAuth};
pub use http::api::{Access, ApiRoutes, Throttle};
pub use provider::{AuthProvider, BUILT_IN_PERMISSIONS};
pub use rbac::{Actor, Grants, NewUser, PermissionResolver, RbacService};
pub use session::Session;
pub use state::{ADMIN_GUARD, API_GUARD, AuthState, Gate, PermissionRegistry};
