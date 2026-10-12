//! [`AdminProvider`]: mounts the admin API and declares resource permissions.

use std::sync::Mutex;

use rustcms_auth::{ADMIN_GUARD, AuthState};
use rustcms_core::{AppBuilder, ServiceProvider};

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
    }
}
