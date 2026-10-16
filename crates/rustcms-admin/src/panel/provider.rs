//! [`AdminProvider`]: mounts the admin API and declares resource permissions.

use std::sync::Mutex;

use async_trait::async_trait;
use clap::{ArgMatches, Command};
use rustcms_auth::{ADMIN_GUARD, AuthState};
use rustcms_core::i18n::Translator;
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
    translator: Translator,
    settings: crate::settings::SettingsRegistry,
    pages: Vec<std::sync::Arc<dyn crate::custom::DynPage>>,
    slots: Vec<std::sync::Arc<dyn crate::custom::DynSlot>>,
    theme: Option<crate::custom::Theme>,
}

/// Locales the panel's interface is written in (`admin-ui/src/i18n`).
pub const PANEL_LOCALES: &[&str] = &["en", "vi"];

impl Default for AdminProvider {
    fn default() -> Self {
        let mut translator = Translator::default();
        translator
            .add_framework("rustcms-admin", "vi", include_str!("../../lang/vi.toml"))
            .expect("the framework's catalog is valid");
        Self {
            brand: "rustcms".into(),
            registry: Mutex::new(Some(Registry::new())),
            media: false,
            extensions: Vec::new(),
            translator,
            settings: crate::settings::SettingsRegistry::default(),
            pages: Vec::new(),
            slots: Vec::new(),
            theme: None,
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
    /// custom widgets: `extension_script("acme", include_str!("../../resources/admin/js/acme/widget.js"))`.
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

    /// Translations of the application's labels (resources, fields, actions,
    /// options, tabs…) for `locale`: TOML mapping English source strings to
    /// translations, optionally per resource (`[posts] "Title" = "…"`).
    ///
    /// ```rust,ignore
    /// AdminProvider::new().translations("vi", include_str!("../lang/vi.toml"))
    /// ```
    ///
    /// # Panics
    /// On malformed TOML, a translation whose `{placeholders}` differ from its
    /// source, or a locale registered twice.
    pub fn translations(mut self, locale: &str, toml: &str) -> Self {
        if let Err(err) = self.translator.add(locale, toml) {
            panic!("{err:#}");
        }
        self
    }

    /// Adds a settings page for group `T` (permissions `settings.<group>.view`
    /// and `settings.<group>.update`).
    ///
    /// # Panics
    /// On a misconfigured page (see [`crate::SettingsPage`]) or a group added twice.
    pub fn settings<T: crate::SettingsPage>(mut self) -> Self {
        self.settings.register::<T>();
        self
    }

    /// Adds the Mail settings page ([`rustcms_core::mail::MailSettings`]:
    /// the SMTP server, checked against `[mail]`'s policy) and its test mail.
    #[cfg(feature = "mail")]
    pub fn mail_settings(self) -> Self {
        self.settings::<rustcms_core::mail::MailSettings>()
    }

    /// Name shown in the panel.
    /// The panel's look: colors, radius, fonts, logo, favicon, color scheme.
    pub fn theme(mut self, theme: crate::custom::Theme) -> Self {
        self.theme = Some(theme);
        self
    }

    /// Adds a custom page (`/admin/p/{slug}`), drawn from a server template or a script.
    ///
    /// # Panics
    /// At boot, on an invalid page (see [`crate::custom::PanelPage`]).
    pub fn page<P: crate::custom::PanelPage>(mut self) -> Self {
        self.pages.push(crate::custom::PageAdapter::<P>::shared());
        self
    }

    /// Adds a slot beside a resource's records.
    ///
    /// # Panics
    /// At boot, on an invalid slot (see [`crate::custom::PanelSlot`]).
    pub fn slot<S: crate::custom::PanelSlot>(mut self) -> Self {
        self.slots.push(crate::custom::SlotAdapter::<S>::shared());
        self
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
        for permission in crate::audit::permissions() {
            auth.permissions.declare(ADMIN_GUARD, permission, Some("audit"));
        }
        for permission in crate::queue::permissions() {
            auth.permissions.declare(ADMIN_GUARD, permission, Some("queue"));
        }
        for page in self.settings.all() {
            for permission in crate::settings::permissions(page.group()) {
                auth.permissions.declare(ADMIN_GUARD, &permission, Some("settings"));
            }
        }
        let admin_path = auth.config.admin_path.clone();
        let custom = std::sync::Arc::new(crate::custom::CustomPanel::new(
            &admin_path,
            self.pages.clone(),
            self.slots.clone(),
            &registry,
            app.config().app.env == rustcms_core::Environment::Local,
        ));
        for permission in custom.declared_permissions() {
            auth.permissions.declare(ADMIN_GUARD, &permission, Some("pages"));
        }
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
        // Background exports keep their files in the application's storage.
        if app.service::<std::sync::Arc<dyn rustcms_core::storage::Storage>>().is_some() {
            app.job::<crate::exports::ExportJob>();
            app.job::<crate::imports::ImportJob>();
            // Import uploads alone accept files up to their limit.
            for resource in registry.all().into_iter().filter(|r| r.meta().importable) {
                app.body_limit(
                    axum::http::Method::POST,
                    crate::imports::upload_path(&admin_path, resource.slug()),
                    crate::imports::MAX_BYTES + 1024,
                );
            }
        }
        app.describe_routes("admin", http::routes(&admin_path));
        app.describe_routes("admin", spa::routes(&admin_path));
        let locales = &app.config().app.locales;
        if let Some(other) = locales.iter().find(|l| !PANEL_LOCALES.contains(&l.as_str())) {
            panic!("app.locales lists {other:?}, but the admin panel is only written in {PANEL_LOCALES:?}");
        }
        let translator = std::sync::Arc::new(self.translator.clone());
        let settings = std::sync::Arc::new(self.settings.clone());
        app.bind(AdminPanel { brand: self.brand.clone(), registry, translator, settings, custom });
        // The theme's stylesheet loads first, so extension styles may refine it.
        let mut extensions = Vec::new();
        let mut theme = spa::ThemeConfig::default();
        if let Some(look) = &self.theme {
            if let Some(css) = look.stylesheet() {
                extensions.push(spa::Extension::generated_style("theme", css));
            }
            for (name, image, slot) in
                [("logo", &look.logo, &mut theme.logo), ("favicon", &look.favicon, &mut theme.favicon)]
            {
                if let Some(image) = image {
                    let asset = spa::Extension::image(name, image.bytes, image.mime);
                    *slot = Some(asset.file.clone());
                    extensions.push(asset);
                }
            }
            theme.scheme = look.scheme.as_str();
        }
        extensions.extend(self.extensions.iter().cloned());
        let extensions = std::sync::Arc::new(extensions);
        let locales = spa::Locales { default: app.config().app.locale.clone(), all: app.config().app.locales.clone() };
        app.routes(move |router| {
            spa::router(
                crate::custom::router(
                    crate::imports::router(
                        crate::exports::router(
                            crate::queue::router(
                                crate::audit::router(
                                    crate::settings::router(
                                        system::router(http::router(router, &admin_path), &admin_path),
                                        &admin_path,
                                    ),
                                    &admin_path,
                                ),
                                &admin_path,
                            ),
                            &admin_path,
                        ),
                        &admin_path,
                    ),
                    &admin_path,
                ),
                &admin_path,
                extensions,
                &locales,
                &theme,
            )
        });
        app.command(IdempotencyPrune);
        app.command(LangMissing);
    }

    async fn boot(&self, state: &AppState) -> anyhow::Result<()> {
        // With Redis, one instance at a time does each job (a lease per job and,
        // for media, per storage disk); the jobs are correct either way.
        if let Some(media) = state.service::<crate::media::MediaState>() {
            let app = state.clone();
            tokio::spawn(async move {
                let lease = format!("media-prune:{}", media.storage.disk());
                let mut every = tokio::time::interval(MEDIA_PRUNE_EVERY);
                loop {
                    every.tick().await;
                    let (db, media) = (app.db().clone(), media.clone());
                    rustcms_core::jobs::exclusive(&app, &lease, MEDIA_PRUNE_EVERY, || async move {
                        if let Err(err) = crate::media::prune(&db, media.storage.as_ref()).await {
                            tracing::warn!(%err, "media pruning failed");
                        }
                    })
                    .await;
                }
            });
        }
        if let Some(storage) = state.service::<std::sync::Arc<dyn rustcms_core::storage::Storage>>() {
            let app = state.clone();
            tokio::spawn(async move {
                let lease = format!("exports-prune:{}", storage.disk());
                let mut every = tokio::time::interval(EXPORTS_PRUNE_EVERY);
                loop {
                    every.tick().await;
                    let (db, storage) = (app.db().clone(), storage.clone());
                    rustcms_core::jobs::exclusive(&app, &lease, EXPORTS_PRUNE_EVERY, || async move {
                        if let Err(err) = crate::exports::prune(&db, storage.as_ref().as_ref()).await {
                            tracing::warn!(%err, "export pruning failed");
                        }
                        if let Err(err) = crate::imports::prune(&db, storage.as_ref().as_ref()).await {
                            tracing::warn!(%err, "import pruning failed");
                        }
                    })
                    .await;
                }
            });
        }
        let app = state.clone();
        tokio::spawn(async move {
            let mut every = tokio::time::interval_at(tokio::time::Instant::now() + PRUNE_EVERY, PRUNE_EVERY);
            loop {
                every.tick().await;
                let db = app.db().clone();
                rustcms_core::jobs::exclusive(&app, "idempotency-prune", PRUNE_EVERY / 2, || async move {
                    match crate::idempotency::prune(&db).await {
                        Ok(n) if n > 0 => tracing::info!(pruned = n, "expired idempotency keys deleted"),
                        Ok(_) => {}
                        Err(err) => tracing::warn!(%err, "idempotency key pruning failed"),
                    }
                })
                .await;
            }
        });
        Ok(())
    }
}

/// How often expired exports are deleted.
const EXPORTS_PRUNE_EVERY: std::time::Duration = std::time::Duration::from_secs(600);

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

/// `lang:missing <locale>`: the labels of the registered resources that no
/// catalog translates, as TOML to fill in (exit code 1 when any is missing).
struct LangMissing;

#[async_trait]
impl AppCommand for LangMissing {
    fn definition(&self) -> Command {
        Command::new("lang:missing")
            .about("List the admin labels without a translation, as TOML to add to the catalog (lang/vi.toml, …)")
            .arg(clap::Arg::new("locale").required(true).help("Language, e.g. `vi`"))
    }

    async fn run(&self, state: &AppState, args: &ArgMatches) -> anyhow::Result<()> {
        let locale = args.get_one::<String>("locale").expect("required");
        anyhow::ensure!(
            state.config().app.locales.contains(locale),
            "{locale:?} is not in app.locales {:?}",
            state.config().app.locales
        );
        let panel = state.service::<AdminPanel>().ok_or_else(|| anyhow::anyhow!("AdminProvider not registered"))?;
        let groups: Vec<String> = state
            .service::<AuthState>()
            .map(|auth| auth.permissions.all().into_values().flatten().collect())
            .unwrap_or_default();
        let missing = panel.missing_translations(locale, &groups);
        if missing.is_empty() {
            println!("# every admin label has a {locale} translation");
            return Ok(());
        }
        // Messages written by hooks (`ctx.t`) are not listed: they only exist when the hook runs.
        println!("# {} admin labels without a {locale} translation (lang/{locale}.toml)", missing.len());
        for source in &missing {
            // A JSON string is a valid TOML basic string.
            println!("{} = \"\"", serde_json::Value::String(source.clone()));
        }
        anyhow::bail!("{} labels are not translated", missing.len())
    }
}

impl AdminPanel {
    /// Labels that no catalog translates in `locale` (English is the source),
    /// looked up as they are when shown: resources' labels in their table
    /// first, navigation and permission groups globally (`lang:missing`).
    pub fn missing_translations(&self, locale: &str, permission_groups: &[String]) -> Vec<String> {
        missing_translations(self, locale, permission_groups)
    }
}

fn missing_translations(panel: &AdminPanel, locale: &str, permission_groups: &[String]) -> Vec<String> {
    if locale == "en" {
        return Vec::new();
    }
    let translated =
        |scope: Option<&str>, s: &str| s.trim().is_empty() || panel.translator.find(locale, scope, s).is_some();
    let mut missing: Vec<String> = panel
        .registry
        .all()
        .into_iter()
        .flat_map(|resource| {
            let slug = resource.slug();
            resource
                .display_strings()
                .into_iter()
                .filter(move |(scoped, s)| !translated(scoped.then_some(slug), s))
                .map(|(_, s)| s)
        })
        .collect();
    missing.extend(panel.settings.display_strings().into_iter().filter(|s| !translated(None, s)));
    missing.extend(panel.custom.display_strings().into_iter().filter(|s| !translated(None, s)));
    // Permission groups that are not resources are shown in words (`users` → "Users").
    for group in permission_groups {
        let source = crate::system::roles::group_source(group);
        if panel.registry.get(group).is_none() && !translated(None, &source) {
            missing.push(source);
        }
    }
    missing.sort();
    missing.dedup();
    missing
}

/// Definitions of the console commands the panel adds (`media:prune` only
/// with `media()`), for the generated CLI reference.
pub fn command_definitions() -> Vec<Command> {
    vec![IdempotencyPrune.definition(), MediaPrune.definition(), LangMissing.definition()]
}
