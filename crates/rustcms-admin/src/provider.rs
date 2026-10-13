//! [`AdminProvider`]: mounts the admin API and declares resource permissions.

use std::sync::Mutex;

use async_trait::async_trait;
use clap::{ArgMatches, Command};
use rustcms_auth::{ADMIN_GUARD, AuthState};
use rustcms_core::{AppBuilder, AppCommand, AppState, ServiceProvider};

use crate::http::{self, AdminPanel};
use crate::registry::Registry;
use crate::resource::Resource;
use crate::{spa, system};

/// The admin panel of an application.
///
/// ```rust,ignore
/// app.provider(AuthProvider::new());
/// app.provider(AdminProvider::new().brand("Blog").resource::<PostResource>().resource::<CategoryResource>());
/// ```
///
/// Register [`rustcms_auth::AuthProvider`] first: the panel uses its admin guard,
/// and every resource's permissions (`posts.view_any`, …, action abilities)
/// are declared for `permission:sync`.
pub struct AdminProvider {
    brand: String,
    registry: Mutex<Option<Registry>>,
}

impl Default for AdminProvider {
    fn default() -> Self {
        Self { brand: "rustcms".into(), registry: Mutex::new(Some(Registry::new())) }
    }
}

impl AdminProvider {
    /// Empty panel.
    pub fn new() -> Self {
        Self::default()
    }

    /// Name shown in the panel.
    pub fn brand(mut self, brand: impl Into<String>) -> Self {
        self.brand = brand.into();
        self
    }

    /// Adds a resource.
    ///
    /// # Panics
    /// On a misconfigured resource (see [`Registry::register`]).
    pub fn resource<R: Resource>(self) -> Self {
        {
            let mut registry = self.registry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let current = registry.take().unwrap_or_default();
            *registry = Some(current.register::<R>());
        }
        self
    }
}

/// How often expired idempotency receipts are deleted while serving.
const PRUNE_EVERY: std::time::Duration = std::time::Duration::from_secs(3_600);

#[async_trait]
impl ServiceProvider for AdminProvider {
    fn register(&self, app: &mut AppBuilder) {
        let registry =
            self.registry.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take().unwrap_or_default().finish();
        let auth = app.service::<AuthState>().expect("register AuthProvider before AdminProvider");
        for resource in registry.all() {
            for permission in resource.permissions() {
                auth.permissions.declare(ADMIN_GUARD, &permission, Some(resource.slug()));
            }
        }
        let admin_path = auth.config.admin_path.clone();
        app.describe_routes("admin", http::routes(&admin_path));
        app.describe_routes("admin", spa::routes(&admin_path));
        app.bind(AdminPanel { brand: self.brand.clone(), registry });
        app.routes(move |router| {
            spa::router(system::router(http::router(router, &admin_path), &admin_path), &admin_path)
        });
        app.command(IdempotencyPrune);
    }

    async fn boot(&self, state: &AppState) -> anyhow::Result<()> {
        let db = state.db().clone();
        tokio::spawn(async move {
            let mut every = tokio::time::interval_at(tokio::time::Instant::now() + PRUNE_EVERY, PRUNE_EVERY);
            loop {
                every.tick().await;
                match crate::idempotency::prune(&db).await {
                    Ok(n) if n > 0 => tracing::info!(pruned = n, "expired idempotency keys deleted"),
                    Ok(_) => {}
                    Err(err) => tracing::warn!(%err, "idempotency key pruning failed"),
                }
            }
        });
        Ok(())
    }
}

/// `idempotency:prune`: deletes expired idempotency receipts.
struct IdempotencyPrune;

#[async_trait]
impl AppCommand for IdempotencyPrune {
    fn definition(&self) -> Command {
        Command::new("idempotency:prune").about("Delete expired idempotency keys of admin writes")
    }

    async fn run(&self, state: &AppState, _: &ArgMatches) -> anyhow::Result<()> {
        let pruned = crate::idempotency::prune(state.db()).await?;
        println!("{pruned} expired idempotency keys deleted");
        Ok(())
    }
}
