//! Serves the admin SPA (`admin-ui/`, built into `assets/`) under the admin path.
//!
//! Assets are embedded in release builds and read from disk in debug builds
//! (`rust-embed`), so using the framework never needs Node. `index.html` gets
//! the runtime configuration as a `<meta>` tag (no inline script: the CSP
//! allows only `'self'`) and a `<base>` so deep links load the same assets.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::Path;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use rust_embed::RustEmbed;
use rustcms_core::{AppError, AppState};

#[derive(RustEmbed)]
#[folder = "assets/"]
struct Assets;

/// Marker in `admin-ui/index.html` replaced by the runtime configuration.
const CONFIG_MARKER: &str = "<!-- rustcms:config -->";

/// Content Security Policy of the panel.
pub const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; script-src 'self'; style-src 'self'; \
     img-src 'self' data:; font-src 'self'; connect-src 'self'; object-src 'none'; base-uri 'self'; \
     form-action 'self'; frame-ancestors 'none'";

const PLACEHOLDER: &str = "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>Admin</title>\
     </head><body><h1>Admin assets are not built</h1><p>Run <code>just admin-build</code> \
     (Node 20+) to build <code>crates/rustcms-admin/assets</code>.</p></body></html>";

/// Panel pages and assets as `(method, path)` (for `route:list`).
pub fn routes(admin_path: &str) -> Vec<(Method, String)> {
    let base = admin_path.trim_end_matches('/');
    vec![(Method::GET, base.to_owned()), (Method::GET, format!("{base}/")), (Method::GET, format!("{base}/{{*rest}}"))]
}

/// An asset the panel serves for the application: an extension script
/// (registering widgets, pages, cells), a stylesheet, or a theme image, under
/// a content-hashed file name (immutable).
#[derive(Debug, Clone)]
pub struct Extension {
    pub(crate) file: String,
    pub(crate) style: bool,
    pub(crate) content_type: &'static str,
    pub(crate) body: std::borrow::Cow<'static, [u8]>,
}

impl Extension {
    /// `name` (`[a-z0-9-]`, 1–64): a script, or a stylesheet with `style`.
    pub(crate) fn new(name: &str, body: &'static str, style: bool) -> Self {
        let (extension, content_type) =
            if style { ("css", "text/css; charset=utf-8") } else { ("js", "text/javascript; charset=utf-8") };
        let mut asset = Self::bytes(name, body.as_bytes().to_vec().into(), extension, content_type);
        asset.style = style;
        asset
    }

    /// A stylesheet generated at boot (the theme).
    pub(crate) fn generated_style(name: &str, css: String) -> Self {
        let mut asset = Self::bytes(name, css.into_bytes().into(), "css", "text/css; charset=utf-8");
        asset.style = true;
        asset
    }

    /// An image (logo, favicon), neither script nor stylesheet.
    pub(crate) fn image(name: &str, body: &'static [u8], mime: &'static str) -> Self {
        let extension = match mime {
            "image/png" => "png",
            "image/webp" => "webp",
            _ => "ico",
        };
        Self::bytes(name, body.into(), extension, mime)
    }

    fn bytes(name: &str, body: std::borrow::Cow<'static, [u8]>, extension: &str, content_type: &'static str) -> Self {
        assert!(
            (1..=64).contains(&name.len())
                && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
            "invalid extension name `{name}`"
        );
        use sha2::Digest;
        let hash = hex::encode(sha2::Sha256::digest(&body));
        Self { file: format!("{name}.{}.{extension}", &hash[..8]), style: false, content_type, body }
    }

    pub(crate) fn is_script(&self) -> bool {
        self.content_type.starts_with("text/javascript")
    }
}

/// Theme facts the panel needs before it renders.
#[derive(Debug, Clone, Default)]
pub(crate) struct ThemeConfig {
    pub(crate) logo: Option<String>,
    pub(crate) favicon: Option<String>,
    pub(crate) scheme: &'static str,
}

/// Languages of the interface: the default and those users may choose.
#[derive(Debug, Clone)]
pub(crate) struct Locales {
    pub(crate) default: String,
    pub(crate) all: Vec<String>,
}

pub(crate) fn router(
    router: Router<AppState>,
    admin_path: &str,
    extensions: Arc<Vec<Extension>>,
    locales: &Locales,
    theme: &ThemeConfig,
) -> Router<AppState> {
    // `admin_path` is normalized by the auth config (`/admin`, never `/`).
    let base = admin_path.trim_end_matches('/').to_owned();
    let index: Arc<str> =
        render_index(Assets::get("index.html").map(|f| f.data), &base, &extensions, locales, theme).into();
    let (root, slash) = (index.clone(), index.clone());
    router
        .route(
            &format!("{base}/{{*rest}}"),
            any(move |method: Method, Path(rest): Path<String>| page(method, rest, index, extensions)),
        )
        .route(&format!("{base}/"), any(move |method: Method| async move { shell(&method, &slash) }))
        .route(&base, any(move |method: Method| async move { shell(&method, &root) }))
}

async fn page(method: Method, rest: String, index: Arc<str>, extensions: Arc<Vec<Extension>>) -> Response {
    // The admin API has its own JSON 404 (registered by the auth provider); this
    // guards the boundary should routes ever change.
    if rest == "api" || rest.starts_with("api/") {
        return AppError::NotFound.into_response();
    }
    if !matches!(method, Method::GET | Method::HEAD) {
        return AppError::MethodNotAllowed.into_response();
    }
    if let Some(file) = rest.strip_prefix("assets/ext/") {
        return match extensions.iter().find(|e| e.file == file) {
            Some(extension) => extension_asset(extension),
            None => AppError::NotFound.into_response(),
        };
    }
    match rest.strip_prefix("assets/") {
        Some(path) => asset(path),
        None => html(&index),
    }
}

fn shell(method: &Method, index: &str) -> Response {
    if matches!(*method, Method::GET | Method::HEAD) { html(index) } else { AppError::MethodNotAllowed.into_response() }
}

fn asset(path: &str) -> Response {
    let Some(file) = Assets::get(&format!("assets/{path}")) else {
        return AppError::NotFound.into_response();
    };
    let mut headers = security_headers();
    let mime = file.metadata.mimetype();
    let mime = HeaderValue::from_str(mime).unwrap_or(HeaderValue::from_static("application/octet-stream"));
    headers.insert(header::CONTENT_TYPE, mime);
    // Built file names carry a content hash.
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("public, max-age=31536000, immutable"));
    (StatusCode::OK, headers, Body::from(file.data.into_owned())).into_response()
}

fn extension_asset(extension: &Extension) -> Response {
    let mut headers = security_headers();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(extension.content_type));
    // The file name carries the content hash.
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("public, max-age=31536000, immutable"));
    (StatusCode::OK, headers, Body::from(extension.body.clone().into_owned())).into_response()
}

fn html(body: &str) -> Response {
    let mut headers = security_headers();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/html; charset=utf-8"));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    (StatusCode::OK, headers, body.to_owned()).into_response()
}

fn security_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(CONTENT_SECURITY_POLICY));
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(header::REFERRER_POLICY, HeaderValue::from_static("same-origin"));
    headers
}

/// `index.html` with the runtime configuration (before any relative asset
/// reference), or a placeholder when the SPA is not built.
fn render_index(
    template: Option<std::borrow::Cow<'static, [u8]>>,
    base: &str,
    extensions: &[Extension],
    locales: &Locales,
    theme: &ThemeConfig,
) -> String {
    let Some(template) = template else {
        return PLACEHOLDER.to_owned();
    };
    let template = String::from_utf8_lossy(&template).into_owned();
    let scripts: Vec<String> =
        extensions.iter().filter(|e| e.is_script()).map(|e| format!("assets/ext/{}", e.file)).collect();
    let styles: Vec<String> = extensions.iter().filter(|e| e.style).map(|e| format!("assets/ext/{}", e.file)).collect();
    // Before sign-in the panel picks among these from the browser's languages.
    let mut config = serde_json::json!({ "basePath": base, "locale": locales.default, "locales": locales.all });
    if !scripts.is_empty() || !styles.is_empty() {
        config["extensions"] = serde_json::json!({ "scripts": scripts, "styles": styles });
    }
    let mut look = serde_json::json!({ "colorScheme": if theme.scheme.is_empty() { "system" } else { theme.scheme } });
    if let Some(logo) = &theme.logo {
        look["logo"] = serde_json::json!(format!("assets/ext/{logo}"));
    }
    if let Some(favicon) = &theme.favicon {
        look["favicon"] = serde_json::json!(format!("assets/ext/{favicon}"));
    }
    config["theme"] = look;
    let config = config.to_string();
    let tags = format!(
        "<base href=\"{}/\"><meta name=\"rustcms-admin\" content=\"{}\">",
        escape_attr(base),
        escape_attr(&config)
    );
    template.replacen(CONFIG_MARKER, &tags, 1)
}

fn escape_attr(value: &str) -> String {
    value.replace('&', "&amp;").replace('"', "&quot;").replace('\'', "&#39;").replace('<', "&lt;").replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn en() -> Locales {
        Locales { default: "en".into(), all: vec!["en".into(), "vi".into()] }
    }

    #[test]
    fn index_gets_base_and_escaped_config() {
        let template = b"<head><!-- rustcms:config --><script src=\"./assets/a.js\"></script></head>";
        let html = render_index(Some(template.as_slice().into()), "/back-office", &[], &en(), &ThemeConfig::default());
        assert_eq!(
            html,
            "<head><base href=\"/back-office/\"><meta name=\"rustcms-admin\" \
             content=\"{&quot;basePath&quot;:&quot;/back-office&quot;,&quot;locale&quot;:&quot;en&quot;,&quot;locales&quot;:[&quot;en&quot;,&quot;vi&quot;],&quot;theme&quot;:{&quot;colorScheme&quot;:&quot;system&quot;}}\"><script src=\"./assets/a.js\"></script></head>"
        );
        assert!(render_index(None, "/admin", &[], &en(), &ThemeConfig::default()).contains("not built"));
        let ext = [Extension::new("acme", "x", false), Extension::new("acme", "y", true)];
        let html = render_index(Some(template.as_slice().into()), "/admin", &ext, &en(), &ThemeConfig::default());
        assert!(html.contains("&quot;scripts&quot;:[&quot;assets/ext/acme.2d711642.js&quot;]"), "{html}");
        assert_eq!(escape_attr("\"'<&>"), "&quot;&#39;&lt;&amp;&gt;");
    }
}
