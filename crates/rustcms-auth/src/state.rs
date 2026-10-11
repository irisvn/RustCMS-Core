//! Shared auth services, bound by [`crate::AuthProvider`].

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rustcms_core::{AppError, AppResult, Principal};

use crate::config::{AuthConfig, origin_of};
use crate::password::Passwords;
use crate::rate_limit::RateLimiter;
use crate::rbac::PermissionResolver;

/// Guard of the admin panel (session cookie).
pub const ADMIN_GUARD: &str = "admin";
/// Guard of the API (bearer token).
pub const API_GUARD: &str = "api";

type Ability = Arc<dyn Fn(&Principal) -> bool + Send + Sync>;

/// Ad-hoc abilities that are not tied to a resource, like Laravel `Gate::define`.
#[derive(Default)]
pub struct Gate {
    abilities: BTreeMap<String, Ability>,
}

impl Gate {
    /// Defines `ability`. Super admins pass without calling `check`; API tokens
    /// must still list the ability (or `*`).
    pub fn define(&mut self, ability: impl Into<String>, check: impl Fn(&Principal) -> bool + Send + Sync + 'static) {
        self.abilities.insert(ability.into(), Arc::new(check));
    }

    /// Whether `principal` may perform `ability`. Undefined abilities fall back
    /// to the permission of the same name (deny by default).
    pub fn allows(&self, principal: &Principal, ability: &str) -> bool {
        let token_allows = principal.token_abilities.as_ref().is_none_or(|a| a.contains("*") || a.contains(ability));
        match self.abilities.get(ability) {
            Some(check) => token_allows && (principal.super_admin || check(principal)),
            None => principal.can(ability),
        }
    }

    /// `allows` or 403.
    pub fn authorize(&self, principal: &Principal, ability: &str) -> AppResult<()> {
        if self.allows(principal, ability) { Ok(()) } else { Err(AppError::Forbidden) }
    }
}

/// Permissions declared by the framework and the application, synced to the
/// database by `permission:sync`. Keyed by `(guard, name)`, valued by group.
#[derive(Default)]
pub struct PermissionRegistry {
    declared: Mutex<BTreeMap<(String, String), Option<String>>>,
}

impl PermissionRegistry {
    /// Declares `name` in `guard`, grouped under `group` in the UI.
    pub fn declare(&self, guard: &str, name: &str, group: Option<&str>) {
        self.declared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert((guard.to_owned(), name.to_owned()), group.map(str::to_owned));
    }

    /// Snapshot of every declaration.
    pub fn all(&self) -> BTreeMap<(String, String), Option<String>> {
        self.declared.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }
}

/// Everything the guards and auth routes need.
pub struct AuthState {
    /// `[auth]` settings.
    pub config: AuthConfig,
    /// Password hasher.
    pub passwords: Passwords,
    /// Sign-in throttling.
    pub limiter: RateLimiter,
    /// Permission resolution.
    pub resolver: PermissionResolver,
    /// Declared permissions.
    pub permissions: PermissionRegistry,
    /// Ad-hoc abilities.
    pub gate: Gate,
    pub(crate) allowed_origins: Vec<String>,
    pub(crate) secure_cookies: bool,
}

impl AuthState {
    /// Builds the state for an application served at `app_url`.
    pub fn new(config: AuthConfig, app_url: &str, gate: Gate) -> Self {
        let allowed_origins = if config.admin_origins.is_empty() {
            origin_of(app_url).into_iter().collect()
        } else {
            config.admin_origins.iter().filter_map(|o| origin_of(o)).collect()
        };
        let secure_cookies = config.secure_cookies.unwrap_or_else(|| app_url.starts_with("https://"));
        Self {
            resolver: PermissionResolver::new(Duration::from_secs(config.permission_cache_ttl_secs)),
            passwords: Passwords::new(),
            limiter: RateLimiter::default(),
            permissions: PermissionRegistry::default(),
            gate,
            allowed_origins,
            secure_cookies,
            config,
        }
    }

    /// Whether `path` belongs to the admin panel (session guard).
    pub fn is_admin_path(&self, path: &str) -> bool {
        let base = self.config.admin_path.trim_end_matches('/');
        path == base || path.strip_prefix(base).is_some_and(|rest| rest.starts_with('/'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_rules() {
        let mut gate = Gate::default();
        gate.define("publish", |p| p.id == "7");
        let user = |id: &str| Principal { id: id.into(), ..Default::default() };
        assert!(gate.allows(&user("7"), "publish"));
        assert!(!gate.allows(&user("8"), "publish"));
        assert!(gate.allows(&Principal { super_admin: true, ..user("8") }, "publish"));
        let narrow = Principal { token_abilities: Some(["posts.view".into()].into()), ..user("7") };
        assert!(!gate.allows(&narrow, "publish"), "token must list the ability");
        assert!(!gate.allows(&user("7"), "undefined"), "deny by default");
    }

    #[test]
    fn admin_path_matching() {
        let state = AuthState::new(AuthConfig::default(), "https://cms.example.com", Gate::default());
        assert!(state.is_admin_path("/admin"));
        assert!(state.is_admin_path("/admin/api/auth/me"));
        assert!(!state.is_admin_path("/administrator"));
        assert!(!state.is_admin_path("/api/v1/x"));
        assert_eq!(state.allowed_origins, ["https://cms.example.com"]);
        assert!(state.secure_cookies);
    }
}
