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

pub mod commands;
pub mod config;
pub mod credentials;
pub mod entities;
pub mod guard;
pub mod password;
mod provider;
pub mod rate_limit;
pub mod rbac;
pub mod routes;
pub mod secret;
pub mod session;
pub mod state;
pub mod tokens;

pub use config::AuthConfig;
pub use guard::{Auth, Authenticated, CSRF_HEADER, MaybeAuth};
pub use provider::{AuthProvider, BUILT_IN_PERMISSIONS};
pub use rbac::{Actor, Grants, NewUser, PermissionResolver, RbacService};
pub use session::Session;
pub use state::{ADMIN_GUARD, API_GUARD, AuthState, Gate, PermissionRegistry};
