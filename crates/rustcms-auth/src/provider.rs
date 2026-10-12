//! [`AuthProvider`]: registers guards, routes, commands and built-in permissions.

use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use axum::middleware::from_fn_with_state;
use rustcms_core::{AppBuilder, AppState, Principal, ServiceProvider};

use crate::commands::{MakeUser, PermissionSync, SessionPrune, TokenCreate};
use crate::config::{AuthConfig, normalize_admin_path};
use crate::state::{ADMIN_GUARD, AuthState, Gate};
use crate::{guard, routes};

/// Permissions of the framework's own management features (admin guard).
pub const BUILT_IN_PERMISSIONS: &[(&str, &str)] = &[
    ("users", "users.view_any"),
    ("users", "users.view"),
    ("users", "users.create"),
    ("users", "users.update"),
    ("users", "users.delete"),
    ("roles", "roles.view_any"),
    ("roles", "roles.view"),
    ("roles", "roles.create"),
    ("roles", "roles.update"),
    ("roles", "roles.delete"),
    ("roles", "roles.assign"),
    ("permissions", "permissions.view_any"),
    ("permissions", "permissions.assign"),
];

const PRUNE_EVERY: Duration = Duration::from_secs(3600);

/// Adds authentication and authorization to an application.
///
/// ```rust,ignore
/// app.provider(AuthProvider::new().permission("api", "posts.view", "posts").define("publish", |p| p.id == "1"));
/// ```
#[derive(Default)]
pub struct AuthProvider {
    gate: Mutex<Option<Gate>>,
    permissions: Vec<(String, String, Option<String>)>,
}

impl AuthProvider {
    /// Provider with only the built-in permissions.
    pub fn new() -> Self {
        Self::default()
    }

    /// Declares a permission for `permission:sync`.
    pub fn permission(mut self, guard: &str, name: &str, group: &str) -> Self {
        self.permissions.push((guard.to_owned(), name.to_owned(), Some(group.to_owned())));
        self
    }

    /// Defines a gate ability (see [`Gate::define`]).
    pub fn define(self, ability: &str, check: impl Fn(&Principal) -> bool + Send + Sync + 'static) -> Self {
        self.gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get_or_insert_with(Gate::default)
            .define(ability, check);
        self
    }
}

#[async_trait]
impl ServiceProvider for AuthProvider {
    fn register(&self, app: &mut AppBuilder) {
        let mut config: AuthConfig =
            app.config().section("auth").unwrap_or_else(|e| panic!("invalid [auth] configuration: {e}"));
        config.admin_path = normalize_admin_path(&config.admin_path).unwrap_or_else(|e| panic!("{e}"));
        let gate = self.gate.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take().unwrap_or_default();
        let state = AuthState::new(config.clone(), &app.config().app.url, gate);
        for (group, name) in BUILT_IN_PERMISSIONS {
            state.permissions.declare(ADMIN_GUARD, name, Some(group));
        }
        for (guard, name, group) in &self.permissions {
            state.permissions.declare(guard, name, group.as_deref());
        }
        app.bind(state);
        let admin_path = config.admin_path.clone();
        app.routes(move |router| routes::router(router, &admin_path));
        app.middleware(|router, state| router.layer(from_fn_with_state(state, guard::authenticate)));
        app.command(MakeUser).command(TokenCreate).command(PermissionSync).command(SessionPrune);
    }

    async fn boot(&self, state: &AppState) -> anyhow::Result<()> {
        let db = state.db().clone();
        tokio::spawn(async move {
            // First run after one period: never competes with startup work.
            let mut every = tokio::time::interval_at(tokio::time::Instant::now() + PRUNE_EVERY, PRUNE_EVERY);
            loop {
                every.tick().await;
                match crate::session::prune_expired(&db).await {
                    Ok(n) if n > 0 => tracing::info!(pruned = n, "expired sessions deleted"),
                    Ok(_) => {}
                    Err(err) => tracing::warn!(%err, "session pruning failed"),
                }
            }
        });
        Ok(())
    }
}
