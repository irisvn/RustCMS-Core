//! [`AuthProvider`]: registers guards, routes, commands and built-in permissions.

use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use axum::middleware::from_fn_with_state;
use rustcms_core::queue::Failure;
use rustcms_core::schedule::Schedule;
use rustcms_core::{AppBuilder, AppState, Principal, ServiceProvider};

use crate::commands::{MakeUser, PermissionSync, RoleAssign, SessionPrune, TokenCreate};
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
    ("users", "users.invite"),
    ("roles", "roles.view_any"),
    ("roles", "roles.view"),
    ("roles", "roles.create"),
    ("roles", "roles.update"),
    ("roles", "roles.delete"),
    ("roles", "roles.assign"),
    ("permissions", "permissions.view_any"),
    ("permissions", "permissions.assign"),
];

/// The framework schedule that deletes expired sessions, ended JWT sign-ins,
/// expired reset and invitation links and audit entries past their retention.
const AUTH_PRUNE: &str = "rustcms.auth-prune";
const PRUNE_EVERY: Duration = Duration::from_secs(3600);
const PRUNE_TIMEOUT: Duration = Duration::from_secs(1800);
/// Time the audit part may spend per run (the next run goes on).
const AUDIT_BUDGET: Duration = Duration::from_secs(60);

/// Adds authentication and authorization to an application.
///
/// ```rust,ignore
/// app.provider(AuthProvider::new().permission("api", "posts.view", "posts").define("publish", |p| p.id == "1"));
/// ```
#[derive(Default)]
pub struct AuthProvider {
    gate: Mutex<Option<Gate>>,
    permissions: Vec<(String, String, Option<String>)>,
    catalog: crate::catalog::Declarations,
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

    /// Declares every permission `routes` require on the API guard (grouped by
    /// the part before the first `.`), so `permission:sync` creates them. A
    /// permission already declared with [`Self::permission`] keeps its group.
    pub fn api_permissions(mut self, routes: &crate::ApiRoutes) -> Self {
        for permission in routes.permissions() {
            if self.permissions.iter().any(|(guard, name, _)| guard == crate::API_GUARD && name == permission) {
                continue;
            }
            let group = crate::http::api::group_of(permission);
            self = self.permission(crate::API_GUARD, permission, group);
        }
        self
    }

    /// Declares, in the `admin` guard, every permission `routes` requires
    /// (group: the name's first segment, `docker.manage` → `docker`), so
    /// `permission:sync` creates them and the Roles page lists them.
    pub fn admin_permissions(mut self, routes: &crate::AdminRoutes) -> Self {
        for permission in routes.permissions() {
            if self.permissions.iter().any(|(guard, name, _)| guard == crate::ADMIN_GUARD && name == permission) {
                continue;
            }
            let group = crate::http::api::group_of(permission);
            self = self.permission(crate::ADMIN_GUARD, permission, group);
        }
        self
    }

    /// Declares a section of the Roles page (or relabels a framework one:
    /// `resources`, `system`, `other`), once per id.
    ///
    /// # Panics
    /// On an invalid id (`[a-z0-9_-]`, 1–64) or an id declared twice.
    pub fn permission_area(mut self, id: &str, area: crate::PermissionArea) -> Self {
        self.catalog.area(id, area);
        self
    }

    /// Declares how `name` of `guard` is shown: label, description, area.
    /// The permission must be declared (checked when the panel registers).
    ///
    /// # Panics
    /// When `name` already has metadata from this provider.
    pub fn permission_meta(mut self, guard: &str, name: &str, meta: crate::PermissionMeta) -> Self {
        self.catalog.meta(guard, name, meta);
        self
    }

    /// Declares a starting selection of admin permissions for the Roles page.
    /// It grants nothing: applying it only changes the unsaved selection.
    ///
    /// # Panics
    /// On an invalid id or an id declared twice.
    pub fn permission_preset(mut self, id: &str, preset: crate::PermissionPreset) -> Self {
        self.catalog.preset(id, preset);
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
        config.jwt.validate().unwrap_or_else(|e| panic!("{e}"));
        assert!(
            (1..=crate::recovery::MAX_INVITATION_DAYS).contains(&config.invitation_days),
            "auth.invitation_days must be between 1 and {}",
            crate::recovery::MAX_INVITATION_DAYS
        );
        let gate = self.gate.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take().unwrap_or_default();
        let mut state = AuthState::new(config.clone(), &app.config().app.url, &app.config().app.key, gate);
        state.catalog = self.catalog.catalog();
        for (group, name) in BUILT_IN_PERMISSIONS {
            state.permissions.declare(ADMIN_GUARD, name, Some(group));
        }
        for (guard, name, group) in &self.permissions {
            state.permissions.declare(guard, name, group.as_deref());
        }
        let realtime: crate::realtime::RealtimeConfig =
            app.config().section("realtime").unwrap_or_else(|e| panic!("invalid [realtime] configuration: {e}"));
        app.bind(crate::realtime::Realtime::new(realtime));
        app.bind(state);
        let admin_path = config.admin_path.clone();
        app.describe_routes("auth", routes::routes(&admin_path));
        // Unknown admin API paths: JSON 404 for every method.
        let any = axum::http::Method::from_bytes(b"ANY").expect("valid method token");
        let base = format!("{}/api", admin_path.trim_end_matches('/'));
        app.describe_routes("auth", [(any.clone(), base.clone()), (any, format!("{base}/{{*rest}}"))]);
        app.routes(move |router| routes::router(router, &admin_path));
        app.middleware(|router, state| router.layer(from_fn_with_state(state, guard::authenticate)));
        // Password resets and invitations: their jobs and mails.
        app.job::<crate::recovery::PasswordResetJob>().job::<crate::recovery::InvitationJob>();
        for template in [
            rustcms_core::mail_template!("auth.password_reset", "en", html),
            rustcms_core::mail_template!("auth.password_reset", "vi", html),
            rustcms_core::mail_template!("auth.invitation", "en", html),
            rustcms_core::mail_template!("auth.invitation", "vi", html),
        ] {
            app.mail_template(template);
        }
        app.command(MakeUser)
            .command(RoleAssign)
            .command(TokenCreate)
            .command(PermissionSync)
            .command(SessionPrune)
            .command(crate::console::commands::AuditPrune);
        // Only `serve` runs it (ADR 0031), once per slot across instances.
        app.schedule(
            Schedule::every(AUTH_PRUNE, PRUNE_EVERY)
                .framework()
                .timeout(PRUNE_TIMEOUT)
                .task(|ctx| async move { prune(&ctx.state).await }),
        );
    }

    async fn boot(&self, state: &AppState) -> anyhow::Result<()> {
        // A bad `[audit]` section stops the boot, not the first pruning.
        rustcms_core::audit::AuditConfig::of(state.config())?;
        if let Some(auth) = state.service::<AuthState>()
            && auth.config.password_reset
        {
            let mails = rustcms_core::mail::mailer(state)
                .is_some_and(|m| m.config().driver != rustcms_core::mail::Driver::Disabled);
            anyhow::ensure!(
                mails,
                "auth.password_reset needs mail: register rustcms::mail::MailProvider with a [mail] driver"
            );
        }
        #[cfg(feature = "redis")]
        if let (Some(auth), Some(redis)) = (state.service::<AuthState>(), state.service::<rustcms_core::redis::Redis>())
        {
            // Every instance counts in the same place.
            auth.limiter.use_redis((*redis).clone());
        }
        Ok(())
    }
}

/// One pass of [`AUTH_PRUNE`]: every part runs even when another fails.
async fn prune(state: &AppState) -> anyhow::Result<Option<String>> {
    let db = state.db();
    let mut failed = false;
    let mut count = |what: &str, result: anyhow::Result<u64>| match result {
        Ok(n) => {
            if n > 0 {
                tracing::info!(pruned = n, "{what} deleted");
            }
            n
        }
        Err(err) => {
            tracing::warn!(err = %format!("{err:#}"), "pruning {what} failed");
            failed = true;
            0
        }
    };
    let sessions = count("expired sessions", crate::session::prune_expired(db).await.map_err(Into::into));
    let jwt = count("ended JWT sign-ins", crate::guards::jwt_session::prune(db).await.map_err(Into::into));
    let links = count("expired reset and invitation links", crate::recovery::prune(db).await.map_err(Into::into));
    let audit = count("old audit entries", crate::console::commands::prune_audit(state, AUDIT_BUDGET).await);
    if failed {
        return Err(Failure::new("prune_failed", "some cleanups failed (see the logs)").into());
    }
    Ok(Some(format!("{sessions} sessions, {jwt} JWT sign-ins, {links} links, {audit} audit entries")))
}
