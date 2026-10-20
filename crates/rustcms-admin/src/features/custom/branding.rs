//! Runtime branding (ADR 0033): a name, logos, a favicon and theme tokens the
//! application chooses while running (a settings page), over the static
//! [`Theme`]. Values are checked as the theme's are; only tokens reach CSS.
//! Each instance caches what its [`BrandingSource`] returns for
//! `[admin] branding_ttl_secs`; [`AdminBranding::invalidate`] drops it at once.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use axum::body::Bytes;
use rustcms_core::{AppResult, AppState};
use serde::Deserialize;

use super::theme::{self, Palette, Theme};

/// A branding value that is refused (shown to the application, never to visitors).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrandingError(String);

impl BrandingError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl std::fmt::Display for BrandingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BrandingError {}

#[derive(Debug, Clone)]
struct OwnedImage {
    bytes: Bytes,
    mime: &'static str,
}

/// The branding a [`BrandingSource`] returns. Every setter checks its value,
/// with the rules of the static [`Theme`]: PNG, WebP or ICO images up to
/// 512 KiB (never SVG), the theme's color tokens as hex colors.
///
/// ```rust,ignore
/// Branding::new().name("Acme")?.logo(bytes, "image/png")?.primary("#2563eb")?
/// ```
#[derive(Debug, Clone, Default)]
pub struct Branding {
    name: Option<String>,
    logo: Option<OwnedImage>,
    logo_dark: Option<OwnedImage>,
    favicon: Option<OwnedImage>,
    light: Palette,
    dark: Palette,
    radius: Option<String>,
}

impl Branding {
    /// Nothing set: the static theme shows through.
    pub fn new() -> Self {
        Self::default()
    }

    /// The name shown in the sidebar, the sign-in page and the tab title.
    ///
    /// # Errors
    /// As [`Branding::check_name`].
    pub fn name(mut self, name: &str) -> Result<Self, BrandingError> {
        Self::check_name(name)?;
        self.name = Some(name.to_owned());
        Ok(self)
    }

    /// The logo (light scheme, and dark without [`Branding::logo_dark`]).
    ///
    /// # Errors
    /// As [`Branding::check_image`].
    pub fn logo(mut self, bytes: impl Into<Bytes>, mime: &str) -> Result<Self, BrandingError> {
        self.logo = Some(owned(bytes.into(), mime)?);
        Ok(self)
    }

    /// The logo of the dark scheme.
    ///
    /// # Errors
    /// As [`Branding::check_image`].
    pub fn logo_dark(mut self, bytes: impl Into<Bytes>, mime: &str) -> Result<Self, BrandingError> {
        self.logo_dark = Some(owned(bytes.into(), mime)?);
        Ok(self)
    }

    /// The browser tab icon.
    ///
    /// # Errors
    /// As [`Branding::check_image`].
    pub fn favicon(mut self, bytes: impl Into<Bytes>, mime: &str) -> Result<Self, BrandingError> {
        self.favicon = Some(owned(bytes.into(), mime)?);
        Ok(self)
    }

    /// A color token of both schemes (override dark ones with [`Branding::dark`]).
    ///
    /// # Errors
    /// As [`Branding::check_color`].
    pub fn color(mut self, token: &str, value: &str) -> Result<Self, BrandingError> {
        self.light = std::mem::take(&mut self.light).try_color(token, value)?;
        Ok(self)
    }

    /// `--primary`.
    ///
    /// # Errors
    /// On a value that is not a hex color.
    pub fn primary(self, value: &str) -> Result<Self, BrandingError> {
        self.color("primary", value)
    }

    /// Tokens of the dark scheme (set with [`Palette::try_color`]).
    ///
    /// # Errors
    /// What `palette` returns.
    pub fn dark(
        mut self,
        palette: impl FnOnce(Palette) -> Result<Palette, BrandingError>,
    ) -> Result<Self, BrandingError> {
        self.dark = palette(std::mem::take(&mut self.dark))?;
        Ok(self)
    }

    /// Corner radius: `0.75rem`, `8px` (at most 4rem / 64px).
    ///
    /// # Errors
    /// On any other value.
    pub fn radius(mut self, value: &str) -> Result<Self, BrandingError> {
        if !theme::length(value) {
            return Err(BrandingError::new("radius must be a number of rem (≤ 4) or px (≤ 64)"));
        }
        self.radius = Some(value.to_owned());
        Ok(self)
    }

    /// A brand name: 1–64 characters, not blank, no control characters.
    ///
    /// # Errors
    /// Otherwise.
    pub fn check_name(name: &str) -> Result<(), BrandingError> {
        let count = name.chars().count();
        if !(1..=64).contains(&count) || name.trim().is_empty() {
            return Err(BrandingError::new("the name must be 1–64 characters"));
        }
        if name.chars().any(char::is_control) {
            return Err(BrandingError::new("the name must not contain control characters"));
        }
        Ok(())
    }

    /// An image the panel serves: PNG, WebP or ICO (`image/x-icon`,
    /// `image/vnd.microsoft.icon`) whose bytes match the type, up to 512 KiB.
    ///
    /// # Errors
    /// On another type (SVG included), a mismatch, or a larger or empty file.
    pub fn check_image(bytes: &[u8], mime: &str) -> Result<(), BrandingError> {
        theme::check_image(bytes, mime).map(|_| ())
    }

    /// A color token (one of [`super::TOKENS`]) set to `#rgb`, `#rgba`,
    /// `#rrggbb` or `#rrggbbaa`.
    ///
    /// # Errors
    /// Otherwise.
    pub fn check_color(token: &str, value: &str) -> Result<(), BrandingError> {
        theme::check_color(token, value).map(|_| ())
    }
}

fn owned(bytes: Bytes, mime: &str) -> Result<OwnedImage, BrandingError> {
    let mime = theme::check_image(&bytes, mime)?;
    Ok(OwnedImage { bytes, mime })
}

/// Where the panel's branding comes from while running (a settings row, a
/// file). Called at most once per `[admin] branding_ttl_secs` per instance,
/// and again after [`AdminBranding::invalidate`].
///
/// ```rust,ignore
/// struct SettingsBranding;
///
/// #[async_trait]
/// impl BrandingSource for SettingsBranding {
///     async fn load(&self, app: &AppState) -> AppResult<Option<Branding>> {
///         let Some(row) = branding::Entity::find().one(app.db()).await? else { return Ok(None) };
///         Ok(Some(Branding::new().name(&row.name).map_err(AppError::internal)?))
///     }
/// }
/// ```
#[async_trait]
pub trait BrandingSource: Send + Sync + 'static {
    /// The current branding, or `None` for the static theme. An error (or a
    /// load slower than `[admin] branding_timeout_ms`) shows the static theme.
    async fn load(&self, app: &AppState) -> AppResult<Option<Branding>>;
}

/// A file the panel serves for its look.
#[derive(Debug, Clone)]
pub(crate) struct Asset {
    pub(crate) hash: String,
    pub(crate) mime: &'static str,
    pub(crate) bytes: Bytes,
}

impl Asset {
    fn new(bytes: Bytes, mime: &'static str) -> Self {
        use sha2::Digest;
        let hash = hex::encode(sha2::Sha256::digest(&bytes))[..8].to_owned();
        Self { hash, mime, bytes }
    }
}

/// The look the panel shows: the static theme with the branding over it.
#[derive(Debug, Clone)]
pub(crate) struct Resolved {
    pub(crate) brand: String,
    pub(crate) style: Option<Asset>,
    pub(crate) logo: Option<Asset>,
    pub(crate) logo_dark: Option<Asset>,
    pub(crate) favicon: Option<Asset>,
    pub(crate) scheme: &'static str,
}

impl Resolved {
    pub(crate) fn new(brand: &str, theme: &Theme, branding: Option<&Branding>) -> Self {
        let mut merged = theme.clone();
        if let Some(b) = branding {
            merged.light = theme.light.merged(&b.light);
            merged.dark = theme.dark.merged(&b.dark);
            if b.radius.is_some() {
                merged.radius.clone_from(&b.radius);
            }
        }
        let image = |runtime: Option<&OwnedImage>, fixed: Option<&theme::Image>| {
            runtime
                .map(|i| Asset::new(i.bytes.clone(), i.mime))
                .or_else(|| fixed.map(|i| Asset::new(Bytes::from_static(i.bytes), i.mime)))
        };
        Self {
            brand: branding.and_then(|b| b.name.clone()).unwrap_or_else(|| brand.to_owned()),
            style: merged.stylesheet().map(|css| Asset::new(Bytes::from(css), "text/css; charset=utf-8")),
            logo: image(branding.and_then(|b| b.logo.as_ref()), theme.logo.as_ref()),
            logo_dark: image(branding.and_then(|b| b.logo_dark.as_ref()), None),
            favicon: image(branding.and_then(|b| b.favicon.as_ref()), theme.favicon.as_ref()),
            scheme: theme.scheme.as_str(),
        }
    }

    /// The file of `kind`, if there is one.
    pub(crate) fn asset(&self, kind: &str) -> Option<&Asset> {
        match kind {
            "theme.css" => self.style.as_ref(),
            "logo" => self.logo.as_ref(),
            "logo-dark" => self.logo_dark.as_ref(),
            "favicon" => self.favicon.as_ref(),
            _ => None,
        }
    }

    /// The path of `kind` relative to the panel (`assets/brand/logo?v=…`).
    pub(crate) fn path(&self, kind: &str) -> Option<String> {
        self.asset(kind).map(|a| format!("assets/brand/{kind}?v={}", a.hash))
    }
}

/// `[admin]` settings of branding.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub(crate) struct BrandingConfig {
    /// `"static"` ignores the [`BrandingSource`] (recovery); `"runtime"` (default) uses it.
    branding: String,
    branding_ttl_secs: u64,
    branding_timeout_ms: u64,
}

impl Default for BrandingConfig {
    fn default() -> Self {
        Self { branding: "runtime".into(), branding_ttl_secs: 30, branding_timeout_ms: 2000 }
    }
}

impl BrandingConfig {
    pub(crate) fn of(config: &rustcms_core::Config) -> Self {
        let this: Self = config.section("admin").unwrap_or_else(|e| panic!("invalid [admin] configuration: {e}"));
        assert!(
            matches!(this.branding.as_str(), "runtime" | "static"),
            "admin.branding must be \"runtime\" or \"static\", not {:?}",
            this.branding
        );
        assert!((1..=3600).contains(&this.branding_ttl_secs), "admin.branding_ttl_secs must be 1 to 3600");
        assert!((1..=30_000).contains(&this.branding_timeout_ms), "admin.branding_timeout_ms must be 1 to 30000");
        this
    }
}

struct Entry {
    generation: u64,
    value: Option<(Arc<Resolved>, Instant)>,
}

/// The panel's branding as this instance shows it (bound by the
/// [`crate::AdminProvider`]). Call [`AdminBranding::invalidate`] after the
/// application saves new branding.
pub struct AdminBranding {
    source: Option<Arc<dyn BrandingSource>>,
    configured: bool,
    fallback: Arc<Resolved>,
    brand: String,
    theme: Theme,
    ttl: Duration,
    timeout: Duration,
    entry: Mutex<Entry>,
    loader: tokio::sync::Mutex<()>,
}

impl AdminBranding {
    pub(crate) fn new(
        brand: &str,
        theme: Theme,
        source: Option<Arc<dyn BrandingSource>>,
        config: &BrandingConfig,
    ) -> Self {
        let configured = source.is_some();
        let source = if config.branding == "static" {
            if configured {
                tracing::warn!("admin.branding is \"static\": the branding source is ignored");
            }
            None
        } else {
            source
        };
        Self {
            source,
            configured,
            fallback: Arc::new(Resolved::new(brand, &theme, None)),
            brand: brand.to_owned(),
            theme,
            ttl: Duration::from_secs(config.branding_ttl_secs),
            timeout: Duration::from_millis(config.branding_timeout_ms),
            entry: Mutex::new(Entry { generation: 0, value: None }),
            loader: tokio::sync::Mutex::new(()),
        }
    }

    /// The static theme alone (what static mode always shows).
    pub(crate) fn fixed(&self) -> Arc<Resolved> {
        self.fallback.clone()
    }

    /// The branding is read from the source (else only the static theme).
    pub fn is_runtime(&self) -> bool {
        self.source.is_some()
    }

    /// A source was registered (even when `admin.branding = "static"` ignores it).
    pub fn has_source(&self) -> bool {
        self.configured
    }

    /// Drops the cached branding: the next request on this instance loads it
    /// again. Other instances follow within `[admin] branding_ttl_secs`.
    pub fn invalidate(&self) {
        let mut entry = self.entry.lock().unwrap_or_else(PoisonError::into_inner);
        entry.generation += 1;
        entry.value = None;
    }

    /// Calls the source once and checks what it returns (`admin:branding`):
    /// `Ok(None)` in static mode or when it returns `None`.
    ///
    /// # Errors
    /// The source's error, or a timeout.
    pub async fn probe(&self, app: &AppState) -> anyhow::Result<Option<String>> {
        let Some(source) = &self.source else { return Ok(None) };
        let loaded = tokio::time::timeout(self.timeout, source.load(app))
            .await
            .map_err(|_| anyhow::anyhow!("the branding source took longer than {:?}", self.timeout))??;
        Ok(loaded.map(|b| Resolved::new(&self.brand, &self.theme, Some(&b)).brand))
    }

    fn fresh(&self) -> Option<Arc<Resolved>> {
        let entry = self.entry.lock().unwrap_or_else(PoisonError::into_inner);
        entry.value.as_ref().filter(|(_, until)| Instant::now() < *until).map(|(v, _)| v.clone())
    }

    /// What to show now: cached, else loaded (one load at a time).
    pub(crate) async fn current(&self, app: &AppState) -> Arc<Resolved> {
        let Some(source) = &self.source else { return self.fallback.clone() };
        if let Some(value) = self.fresh() {
            return value;
        }
        let _loading = self.loader.lock().await;
        if let Some(value) = self.fresh() {
            return value;
        }
        let mut retried = false;
        loop {
            let generation = self.entry.lock().unwrap_or_else(PoisonError::into_inner).generation;
            let resolved = self.load(source.as_ref(), app).await;
            let mut entry = self.entry.lock().unwrap_or_else(PoisonError::into_inner);
            // Invalidated meanwhile: what was loaded may predate the change.
            if entry.generation == generation {
                entry.value = Some((resolved.clone(), Instant::now() + self.ttl));
                return resolved;
            }
            if retried {
                return resolved;
            }
            retried = true;
        }
    }

    async fn load(&self, source: &dyn BrandingSource, app: &AppState) -> Arc<Resolved> {
        match tokio::time::timeout(self.timeout, source.load(app)).await {
            Ok(Ok(Some(branding))) => Arc::new(Resolved::new(&self.brand, &self.theme, Some(&branding))),
            Ok(Ok(None)) => self.fallback.clone(),
            Ok(Err(_)) => {
                tracing::warn!("the branding source failed; showing the static theme");
                self.fallback.clone()
            }
            Err(_) => {
                tracing::warn!("the branding source timed out; showing the static theme");
                self.fallback.clone()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";

    #[test]
    fn values_are_checked() {
        assert!(Branding::new().name("Acme").is_ok());
        assert!(Branding::new().name("").is_err());
        assert!(Branding::new().name("  ").is_err());
        assert!(Branding::new().name(&"x".repeat(65)).is_err());
        assert!(Branding::new().name("a\u{7}b").is_err());
        assert!(Branding::new().logo(PNG, "image/png").is_ok());
        assert!(Branding::new().logo(PNG, "image/webp").is_err(), "a mismatch");
        assert!(Branding::new().logo(&b"<svg onload=alert(1)>"[..], "image/svg+xml").is_err());
        assert!(Branding::new().favicon(Vec::new(), "image/png").is_err(), "empty");
        let big = [PNG, &vec![0; 512 * 1024]].concat();
        assert!(Branding::new().logo(big, "image/png").is_err());
        assert!(Branding::new().primary("#2563eb").is_ok());
        assert!(Branding::new().primary("red;}body{display:none").is_err());
        assert!(Branding::new().color("--x", "#fff").is_err());
        assert!(Branding::new().radius("1rem").is_ok());
        assert!(Branding::new().radius("1em").is_err());
        assert!(Branding::check_color("primary", "#abc").is_ok());
        assert!(Branding::check_image(PNG, "image/png").is_ok());
    }

    #[test]
    fn branding_overrides_the_theme_field_by_field() {
        let theme = Theme::new()
            .primary("#111111")
            .radius("4px")
            .dark(|p| p.color("primary", "#222222"))
            .logo(PNG, "image/png");
        let fixed = Resolved::new("Static", &theme, None);
        assert_eq!(fixed.brand, "Static");
        assert!(fixed.logo.is_some() && fixed.logo_dark.is_none() && fixed.favicon.is_none());
        let branding = Branding::new().name("Acme").unwrap().primary("#2563eb").unwrap();
        let resolved = Resolved::new("Static", &theme, Some(&branding));
        assert_eq!(resolved.brand, "Acme");
        let css = String::from_utf8(resolved.style.as_ref().unwrap().bytes.to_vec()).unwrap();
        assert!(css.contains("--primary:#2563eb;") && css.contains("--radius:4px;"), "{css}");
        assert!(css.contains(":root[data-scheme=dark]{--primary:#222222;}"), "{css}");
        assert_eq!(
            resolved.logo.as_ref().unwrap().hash,
            fixed.logo.as_ref().unwrap().hash,
            "the static logo shows through"
        );
        assert!(resolved.path("logo-dark").is_none());
        assert!(fixed.path("logo").unwrap().starts_with("assets/brand/logo?v="));
    }
}
