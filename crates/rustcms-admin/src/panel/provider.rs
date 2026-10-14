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
    media: bool,
    extensions: Vec<crate::spa::Extension>,
}

impl Default for AdminProvider {
    fn default() -> Self {
        Self {
            brand: "rustcms".into(),
            registry: Mutex::new(Some(Registry::new())),
            media: false,
            extensions: Vec::new(),
        }
    }
}

impl AdminProvider {
    /// Empty panel.
    pub fn new() -> Self {
        Self::default()
    }

    /// Enables the media library (uploads, `FileUpload` inputs, rich text
    /// images). Register [`rustcms_core::storage::StorageProvider`] first.
    pub fn media(mut self) -> Self {
        self.media = true;
        self.resource::<crate::media::MediaResource>()
    }

    /// A script the panel loads (in order) before it renders, e.g. to register
    /// custom widgets: `extension_script("acme", include_str!("../assets/admin/acme.js"))`.
    /// Extensions are trusted application code with the panel's full privileges.
    ///
    /// # Panics
    /// On a name other than 1–64 of `a-z`, `0-9`, `-`, or a name used twice.
    pub fn extension_script(mut self, name: &str, js: &'static str) -> Self {
        self.add_extension(crate::spa::Extension::new(name, js, false));
        self
    }

    /// A stylesheet the panel loads (for extension widgets; inline styles are not allowed).
    pub fn extension_style(mut self, name: &str, css: &'static str) -> Self {
        self.add_extension(crate::spa::Extension::new(name, css, true));
        self
    }

    fn add_extension(&mut self, extension: crate::spa::Extension) {
        let stem = |e: &crate::spa::Extension| e.file.split('.').next().unwrap_or_default().to_owned();
        assert!(
            !self.extensions.iter().any(|e| stem(e) == stem(&extension) && e.style == extension.style),
            "extension `{}` added twice",
            stem(&extension)
        );
        self.extensions.push(extension);
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
        if self.media {
            let storage = app
                .service::<std::sync::Arc<dyn rustcms_core::storage::Storage>>()
                .expect("register StorageProvider before AdminProvider::media()")
                .as_ref()
                .clone();
            let config = app
                .service::<rustcms_core::storage::StorageConfig>()
                .expect("register StorageProvider before AdminProvider::media()")
                .as_ref()
                .clone();
            auth.permissions.declare(ADMIN_GUARD, "media.manage_all", Some(crate::media::SLUG));
            // The upload route alone accepts bodies up to the upload limit (plus multipart framing).
            let limit = usize::try_from(config.max_upload_bytes).unwrap_or(usize::MAX).saturating_add(64 * 1024);
            app.body_limit(axum::http::Method::POST, format!("{}/api/media", admin_path.trim_end_matches('/')), limit);
            let uploads = tokio::sync::Semaphore::new(config.max_concurrent_uploads.max(1));
            app.bind(crate::media::MediaState { storage, config, uploads });
            app.command(MediaPrune);
        }
        app.describe_routes("admin", http::routes(&admin_path));
        app.describe_routes("admin", spa::routes(&admin_path));
        app.bind(AdminPanel { brand: self.brand.clone(), registry });
        let extensions = std::sync::Arc::new(self.extensions.clone());
        app.routes(move |router| {
            spa::router(system::router(http::router(router, &admin_path), &admin_path), &admin_path, extensions)
        });
        app.command(IdempotencyPrune);
    }

    async fn boot(&self, state: &AppState) -> anyhow::Result<()> {
        if let Some(media) = state.service::<crate::media::MediaState>() {
            let db = state.db().clone();
            tokio::spawn(async move {
                let mut every = tokio::time::interval(MEDIA_PRUNE_EVERY);
                loop {
                    every.tick().await;
                    if let Err(err) = crate::media::prune(&db, media.storage.as_ref()).await {
                        tracing::warn!(%err, "media pruning failed");
                    }
                }
            });
        }
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

/// How often queued media deletions and abandoned uploads are processed.
const MEDIA_PRUNE_EVERY: std::time::Duration = std::time::Duration::from_secs(60);

/// `media:prune`: deletes queued media objects and abandoned uploads.
struct MediaPrune;

#[async_trait]
impl AppCommand for MediaPrune {
    fn definition(&self) -> Command {
        Command::new("media:prune").about("Delete queued media files and abandoned uploads")
    }

    async fn run(&self, state: &AppState, _: &ArgMatches) -> anyhow::Result<()> {
        let media =
            state.service::<crate::media::MediaState>().ok_or_else(|| anyhow::anyhow!("media is not enabled"))?;
        let deleted = crate::media::prune(state.db(), media.storage.as_ref()).await?;
        println!("{deleted} media files deleted");
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
