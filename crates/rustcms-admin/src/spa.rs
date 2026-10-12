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

pub(crate) fn router(router: Router<AppState>, admin_path: &str) -> Router<AppState> {
    // `admin_path` is normalized by the auth config (`/admin`, never `/`).
    let base = admin_path.trim_end_matches('/').to_owned();
    let index: Arc<str> = render_index(Assets::get("index.html").map(|f| f.data), &base).into();
    let (root, slash) = (index.clone(), index.clone());
    router
        .route(
            &format!("{base}/{{*rest}}"),
            any(move |method: Method, Path(rest): Path<String>| page(method, rest, index)),
        )
        .route(&format!("{base}/"), any(move |method: Method| async move { shell(&method, &slash) }))
        .route(&base, any(move |method: Method| async move { shell(&method, &root) }))
}

async fn page(method: Method, rest: String, index: Arc<str>) -> Response {
    // The admin API has its own JSON 404 (registered by the auth provider); this
    // guards the boundary should routes ever change.
    if rest == "api" || rest.starts_with("api/") {
        return AppError::NotFound.into_response();
    }
    if !matches!(method, Method::GET | Method::HEAD) {
        return AppError::MethodNotAllowed.into_response();
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
fn render_index(template: Option<std::borrow::Cow<'static, [u8]>>, base: &str) -> String {
    let Some(template) = template else {
        return PLACEHOLDER.to_owned();
    };
    let template = String::from_utf8_lossy(&template).into_owned();
    let config = serde_json::json!({ "basePath": base }).to_string();
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

    #[test]
    fn index_gets_base_and_escaped_config() {
        let template = b"<head><!-- rustcms:config --><script src=\"./assets/a.js\"></script></head>";
        let html = render_index(Some(template.as_slice().into()), "/back-office");
        assert_eq!(
            html,
            "<head><base href=\"/back-office/\"><meta name=\"rustcms-admin\" \
             content=\"{&quot;basePath&quot;:&quot;/back-office&quot;}\"><script src=\"./assets/a.js\"></script></head>"
        );
        assert!(render_index(None, "/admin").contains("not built"));
        assert_eq!(escape_attr("\"'<&>"), "&quot;&#39;&lt;&amp;&gt;");
    }
}
