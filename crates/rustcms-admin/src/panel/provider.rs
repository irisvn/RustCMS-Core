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
    extension_timeout: std::time::Duration,
    translator: Translator,
    settings: crate::settings::SettingsRegistry,
    pages: Vec<std::sync::Arc<dyn crate::custom::DynPage>>,
    slots: Vec<std::sync::Arc<dyn crate::custom::DynSlot>>,
    theme: Option<crate::custom::Theme>,
    branding: Option<std::sync::Arc<dyn crate::custom::BrandingSource>>,
    dashboard: crate::dashboard::Dashboard,
    home: Option<String>,
    date_format: Option<String>,
    datetime_format: Option<String>,
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
            extension_timeout: std::time::Duration::from_secs(10),
            translator,
            settings: crate::settings::SettingsRegistry::default(),
            pages: Vec::new(),
            slots: Vec::new(),
            theme: None,
            branding: None,
            dashboard: crate::dashboard::Dashboard::default(),
            home: None,
            date_format: None,
            datetime_format: None,
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

    /// A v2 script (React pages, slots and cells sharing the panel's React;
    /// ADR 0032), built with the SDK's preset. `requires` is checked before
    /// it runs: a mismatch leaves its views unavailable.
    pub fn extension_script_v2(mut self, name: &str, js: &'static str, requires: crate::spa::Requires) -> Self {
        let mut extension = crate::spa::Extension::new(name, js, false);
        extension.requires = Some(requires);
        self.add_extension(extension);
        self
    }

    /// How long the panel waits for each extension script (1 to 60 s,
    /// default 10); a script that misses it registers nothing.
    ///
    /// # Panics
    /// Out of range.
    pub fn extension_timeout(mut self, timeout: std::time::Duration) -> Self {
        assert!(
            (std::time::Duration::from_secs(1)..=std::time::Duration::from_secs(60)).contains(&timeout),
            "extension_timeout must be 1 to 60 seconds"
        );
        self.extension_timeout = timeout;
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

    /// The dashboard's widgets (see [`crate::dashboard`]).
    ///
    /// # Panics
    /// At boot, on an invalid widget.
    pub fn dashboard(mut self, dashboard: crate::dashboard::Dashboard) -> Self {
        self.dashboard = dashboard;
        self
    }

    /// Adds the Mail settings page ([`rustcms_core::mail::MailSettings`]:
    /// the SMTP server, checked against `[mail]`'s policy) and its test mail.
    #[cfg(feature = "mail")]
    pub fn mail_settings(self) -> Self {
        self.settings::<rustcms_core::mail::MailSettings>()
    }

    /// The panel's look: colors, radius, fonts, logo, favicon, color scheme.
    pub fn theme(mut self, theme: crate::custom::Theme) -> Self {
        self.theme = Some(theme);
        self
    }

    /// How the panel shows dates (date fields) without a pattern of their own,
    /// in the grammar of [`crate::ui::TableColumn::date_format`] without
    /// times, e.g. `"dd/MM/yyyy"`. Default: the user's language's medium
    /// style.
    ///
    /// # Panics
    /// On a pattern that is not one, or shows a time.
    pub fn date_format(mut self, pattern: impl Into<String>) -> Self {
        let pattern = pattern.into();
        crate::engine::date_format::check("admin panel", &pattern, Some(crate::wire::FieldKind::Date));
        self.date_format = Some(pattern);
        self
    }

    /// How the panel shows date-times (local date-time and instant fields,
    /// history, queue and schedule times) without a pattern of their own, e.g.
    /// `"HH:mm:ss dd/MM/yyyy"` ([`crate::ui::TableColumn::date_format`]).
    /// Default: the user's language's medium style.
    ///
    /// # Panics
    /// On a pattern that is not one, or shows the time zone (`z`): local
    /// date-times have none (a column or entry of an instant may show it).
    pub fn datetime_format(mut self, pattern: impl Into<String>) -> Self {
        let pattern = pattern.into();
        crate::engine::date_format::check("admin panel", &pattern, Some(crate::wire::FieldKind::LocalDateTime));
        self.datetime_format = Some(pattern);
        self
    }

    /// Branding chosen while running (name, logos, favicon, color tokens),
    /// over the [`Self::brand`] and [`Self::theme`]. Cached per instance for
    /// `[admin] branding_ttl_secs`; `admin.branding = "static"` ignores it.
    pub fn branding(mut self, source: impl crate::custom::BrandingSource) -> Self {
        self.branding = Some(std::sync::Arc::new(source));
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

    /// The custom page `/admin` opens instead of the dashboard, for users who
    /// may open it (others see the dashboard).
    ///
    /// # Panics
    /// At boot, when no page has this slug.
    pub fn home(mut self, slug: impl Into<String>) -> Self {
        self.home = Some(slug.into());
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

    /// Runs `hook` in the transaction of every change to a media record
    /// (upload, rename or alternative text, trash, restore, delete): see
    /// [`crate::media::MediaHook`]. Needs [`media`](Self::media); several
    /// hooks run in the order they were added.
    pub fn media_hook(self, hook: impl crate::media::MediaHook) -> Self {
        {
            let mut registry = self.registry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let current = registry.take().unwrap_or_default();
            *registry = Some(current.media_hook(hook));
        }
        self
    }

    /// Declares a collection of the media library: `name` is stored on the
    /// files uploaded through the fields and settings that name it
    /// ([`crate::Field::collection`], [`crate::settings::SettingField::collection`]);
    /// `label` is shown (translated) by the Media page's filter, the media
    /// record and the picker, in declaration order. Collections sort and
    /// filter; they never limit access. Needs [`media`](Self::media).
    ///
    /// # Panics
    /// On a name other than 1 to 64 of `a-z`, `0-9`, `.`, `_`, `-`, an empty
    /// label, or a name declared twice; at boot, without the media library.
    pub fn media_collection(self, name: &str, label: impl Into<String>) -> Self {
        {
            let mut registry = self.registry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let current = registry.take().unwrap_or_default();
            *registry = Some(current.media_collection(name, label));
        }
        self
    }

    /// Declares a node rich text documents may hold (document version 3),
    /// for the fields that accept it ([`crate::Field::nodes`]).
    ///
    /// # Panics
    /// On an invalid declaration or a name declared twice.
    pub fn rich_text_node(self, node: crate::rich_text::RichTextNode) -> Self {
        {
            let mut registry = self.registry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let current = registry.take().unwrap_or_default();
            *registry = Some(current.rich_text_node(node));
        }
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

/// The framework schedule that deletes expired idempotency receipts.
const IDEMPOTENCY_PRUNE: &str = "rustcms.idempotency-prune";
/// How often expired idempotency receipts are deleted while serving.
const PRUNE_EVERY: std::time::Duration = std::time::Duration::from_secs(3_600);
/// Longest run of that pass.
const PRUNE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1_800);

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
            // Encrypted fields rotate with `key:reencrypt`.
            for column in resource.encrypted_columns() {
                app.add_encrypted_column(column);
            }
        }
        for permission in crate::audit::permissions() {
            auth.permissions.declare(ADMIN_GUARD, permission, Some("audit"));
        }
        for permission in crate::queue::permissions() {
            auth.permissions.declare(ADMIN_GUARD, permission, Some("queue"));
        }
        for permission in crate::schedules::permissions() {
            auth.permissions.declare(ADMIN_GUARD, permission, Some("schedules"));
        }
        self.dashboard.check(&registry);
        for permission in self.dashboard.permissions() {
            auth.permissions.declare(ADMIN_GUARD, &permission, Some("dashboard"));
        }
        assert!(
            self.media || registry.media_collections().is_empty(),
            "media collections need the media library (`AdminProvider::media`)"
        );
        for page in self.settings.all() {
            if let Some(field) = page.fields().iter().find(|f| f.media.is_some()) {
                assert!(
                    self.media,
                    "settings `{}`: media setting `{}` needs the media library (`AdminProvider::media`)",
                    page.group(),
                    field.name
                );
            }
            for field in page.fields() {
                if let Some(collection) = &field.collection {
                    assert!(
                        registry.media_collection_named(collection).is_some(),
                        "settings `{}`: setting `{}` uses the media collection `{collection}`, which is not \
                         declared (`AdminProvider::media_collection`)",
                        page.group(),
                        field.name
                    );
                }
            }
            for permission in crate::settings::permissions(page.group()) {
                auth.permissions.declare(ADMIN_GUARD, &permission, Some("settings"));
            }
        }
        let admin_path = auth.config.admin_path.clone();
        // API mode (ADR 0036): everything below is declared and bound as in
        // `full` (permissions, jobs, commands, schedules, the registry for
        // jobs and commands), but no route of the panel is mounted.
        let serves_panel = !app.mode().is_api();
        let custom = std::sync::Arc::new(
            crate::custom::CustomPanel::new(
                &admin_path,
                self.pages.clone(),
                self.slots.clone(),
                &registry,
                app.config().app.env == rustcms_core::Environment::Local,
            )
            .with_home(self.home.clone()),
        );
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
            auth.permissions.declare(ADMIN_GUARD, crate::media::MANAGE_ALL, Some(crate::media::SLUG));
            auth.permissions.declare(ADMIN_GUARD, crate::media::VIEW_ALL, Some(crate::media::SLUG));
            // The upload route alone accepts bodies up to the upload limit (plus multipart framing).
            let limit = usize::try_from(config.max_upload_bytes).unwrap_or(usize::MAX).saturating_add(64 * 1024);
            if serves_panel {
                app.body_limit(
                    axum::http::Method::POST,
                    format!("{}/api/media", admin_path.trim_end_matches('/')),
                    limit,
                );
            }
            let uploads = tokio::sync::Semaphore::new(config.max_concurrent_uploads.max(1));
            // One schedule per disk: instances on another disk have their own slots.
            let name = format!("{MEDIA_PRUNE}.{}", storage.disk());
            app.bind(crate::media::MediaState { storage, config, uploads });
            app.command(MediaPrune);
            // Only `serve` runs it (ADR 0031), once per slot across instances;
            // each instance sweeps its own staged files (`storage::sweep_staging`).
            app.schedule(
                rustcms_core::schedule::Schedule::every(&name, MEDIA_PRUNE_EVERY)
                    .framework()
                    .timeout(STORAGE_PRUNE_TIMEOUT)
                    .task(|ctx| async move {
                        let media = ctx
                            .state
                            .service::<crate::media::MediaState>()
                            .ok_or_else(|| anyhow::anyhow!("media is not enabled"))?;
                        let deleted = crate::media::prune(ctx.state.db(), media.storage.as_ref()).await?;
                        Ok(Some(format!("{deleted} media files deleted")))
                    }),
            );
        }
        // Every permission is declared by now: the Roles page's labels,
        // areas and presets may only name declared ones.
        auth.catalog.check(auth.permissions.all().keys());
        // Background exports keep their files in the application's storage.
        if let Some(storage) = app.service::<std::sync::Arc<dyn rustcms_core::storage::Storage>>() {
            app.job::<crate::exports::ExportJob>();
            app.job::<crate::imports::ImportJob>();
            app.schedule(
                rustcms_core::schedule::Schedule::every(
                    &format!("{EXPORTS_PRUNE}.{}", storage.disk()),
                    EXPORTS_PRUNE_EVERY,
                )
                .framework()
                .timeout(STORAGE_PRUNE_TIMEOUT)
                .task(|ctx| async move { prune_exports(&ctx.state).await }),
            );
            // Import uploads alone accept files up to their limit.
            for resource in registry.all().into_iter().filter(|r| serves_panel && r.meta().importable) {
                app.body_limit(
                    axum::http::Method::POST,
                    crate::imports::upload_path(&admin_path, resource.slug()),
                    crate::imports::MAX_BYTES + 1024,
                );
            }
        }
        if serves_panel {
            app.describe_routes("admin", http::routes(&admin_path));
            app.describe_routes("admin", spa::routes(&admin_path));
        }
        let locales = &app.config().app.locales;
        if let Some(other) = locales.iter().find(|l| !PANEL_LOCALES.contains(&l.as_str())) {
            panic!("app.locales lists {other:?}, but the admin panel is only written in {PANEL_LOCALES:?}");
        }
        let translator = std::sync::Arc::new(self.translator.clone());
        let settings = std::sync::Arc::new(self.settings.clone());
        let dashboard = std::sync::Arc::new(self.dashboard.clone());
        app.bind(AdminPanel { brand: self.brand.clone(), registry, translator, settings, custom, dashboard });
        // The look (static theme, runtime branding over it) is served under
        // `assets/brand/`; its stylesheet loads first, so extension styles may refine it.
        app.bind(crate::custom::AdminBranding::new(
            &self.brand,
            self.theme.clone().unwrap_or_default(),
            self.branding.clone(),
            &crate::custom::branding::BrandingConfig::of(app.config()),
        ));
        let branding = app.service::<crate::custom::AdminBranding>().expect("just bound");
        let mut extensions = Vec::new();
        extensions.extend(self.extensions.iter().cloned());
        let extensions = std::sync::Arc::new(extensions);
        let locales = spa::Locales {
            default: app.config().app.locale.clone(),
            all: app.config().app.locales.clone(),
            password_reset: auth.config.password_reset,
            remember: auth.config.remember_days > 0,
            date_format: self.date_format.clone(),
            datetime_format: self.datetime_format.clone(),
        };
        if serves_panel {
            // Same-origin sockets, also for browsers whose `'self'` misses `ws:`/`wss:`.
            let csp = spa::content_security_policy(auth.allowed_origins());
            let extension_timeout = self.extension_timeout;
            app.routes(move |router| {
                spa::router(
                    crate::custom::router(
                        crate::imports::router(
                            crate::exports::router(
                                crate::schedules::router(
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
                    ),
                    &admin_path,
                    extensions,
                    &locales,
                    branding,
                    &csp,
                    extension_timeout,
                )
            });
        }
        app.command(IdempotencyPrune);
        // Only `serve` runs it (ADR 0031), once per slot across instances.
        app.schedule(
            rustcms_core::schedule::Schedule::every(IDEMPOTENCY_PRUNE, PRUNE_EVERY)
                .framework()
                .timeout(PRUNE_TIMEOUT)
                .task(|ctx| async move {
                    let pruned = crate::idempotency::prune(ctx.state.db()).await?;
                    if pruned > 0 {
                        tracing::info!(pruned, "expired idempotency keys deleted");
                    }
                    Ok(Some(format!("{pruned} expired keys")))
                }),
        );
        app.command(LangMissing);
        app.command(BrandingCheck);
    }
}

/// Expired exports and imports of the storage's disk (`exports::prune`,
/// `imports::prune`); the schedule is named after the disk (`….local`).
const EXPORTS_PRUNE: &str = "rustcms.exports-prune";
/// How often expired exports and imports are deleted.
const EXPORTS_PRUNE_EVERY: std::time::Duration = std::time::Duration::from_secs(600);
/// Queued media deletions and abandoned uploads (`media::prune`), named
/// after the disk (`….local`).
const MEDIA_PRUNE: &str = "rustcms.media-prune";
/// How often queued media deletions and abandoned uploads are processed.
const MEDIA_PRUNE_EVERY: std::time::Duration = std::time::Duration::from_secs(60);
/// Longest run of either: a media pass stops taking work after 5 minutes.
const STORAGE_PRUNE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1_800);
/// Longest pass over exports, then over imports, within one run: one never
/// takes the other's time.
const EXPORTS_PASS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);

/// `rustcms.exports-prune.<disk>`: expired exports, then expired imports, each
/// attempted (and bounded) whatever the other did.
async fn prune_exports(state: &AppState) -> anyhow::Result<Option<String>> {
    let storage = rustcms_core::storage::of(state).ok_or_else(|| anyhow::anyhow!("no storage is bound"))?;
    let mut failed = false;
    let mut count = |what: &str, result: Result<rustcms_core::AppResult<u64>, tokio::time::error::Elapsed>| match result
    {
        Ok(Ok(n)) => n,
        Ok(Err(err)) => {
            tracing::warn!(%err, "pruning {what} failed");
            failed = true;
            0
        }
        Err(_) => {
            tracing::warn!("pruning {what} timed out");
            failed = true;
            0
        }
    };
    let exports = count(
        "expired exports",
        tokio::time::timeout(EXPORTS_PASS_TIMEOUT, crate::exports::prune(state.db(), storage.as_ref())).await,
    );
    let imports = count(
        "expired imports",
        tokio::time::timeout(EXPORTS_PASS_TIMEOUT, crate::imports::prune(state.db(), storage.as_ref())).await,
    );
    if failed {
        return Err(rustcms_core::queue::Failure::new("prune_failed", "some cleanups failed (see the logs)").into());
    }
    Ok(Some(format!("{exports} expired exports, {imports} expired imports")))
}

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
        let mut deleted = crate::media::prune(state.db(), media.storage.as_ref()).await?;
        // And this instance's abandoned staged files.
        deleted += rustcms_core::storage::sweep_staging(state).await?;
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
struct BrandingCheck;

#[async_trait]
impl AppCommand for BrandingCheck {
    fn definition(&self) -> Command {
        Command::new("admin:branding")
            .about("Show where the panel's branding comes from, and load it once to check it (changes nothing)")
    }

    async fn run(&self, state: &AppState, _: &ArgMatches) -> anyhow::Result<()> {
        let branding = state
            .service::<crate::custom::AdminBranding>()
            .ok_or_else(|| anyhow::anyhow!("AdminProvider not registered"))?;
        println!("source: {}", if branding.has_source() { "registered" } else { "none" });
        println!("mode: {}", if branding.is_runtime() { "runtime" } else { "static" });
        if branding.is_runtime() {
            match branding.probe(state).await? {
                Some(name) => println!("loaded: valid (brand {name:?})"),
                None => println!("loaded: none (the static theme shows)"),
            }
        }
        Ok(())
    }
}

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
        let mut missing = panel.missing_translations(locale, &groups);
        // The Roles page's areas, permission labels and presets.
        if let Some(auth) = state.service::<AuthState>().filter(|_| locale != "en") {
            missing.extend(
                auth.catalog.display_strings().into_iter().filter(|s| panel.translator.find(locale, None, s).is_none()),
            );
            missing.sort();
            missing.dedup();
        }
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
    missing.extend(panel.dashboard.display_strings().into_iter().filter(|s| !translated(None, s)));
    // Media collections are shown by their label everywhere (filter, record, picker).
    missing.extend(panel.registry.media_collections().iter().map(|c| c.label.clone()).filter(|s| !translated(None, s)));
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
    vec![IdempotencyPrune.definition(), MediaPrune.definition(), LangMissing.definition(), BrandingCheck.definition()]
}
