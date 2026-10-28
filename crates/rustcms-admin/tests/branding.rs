//! Runtime branding (ADR 0033): cached per instance, invalidated at once,
//! served under `assets/brand/` with the panel's headers; the static theme
//! when the source fails or `admin.branding = "static"`.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use axum::http::StatusCode;
use rustcms_admin::{AdminBranding, AdminProvider, Branding, BrandingSource, Theme};
use rustcms_auth::AuthProvider;
use rustcms_core::{AppBuilder, AppError, AppResult, AppState, Config};
use rustcms_testing::{Factory, TestApp, TestDb, TestResponse, db_test};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01";
const WEBP: &[u8] = b"RIFF\x0c\0\0\0WEBPVP8 ";

/// What the fake source returns next (each test runs in its own process).
#[derive(Clone, Copy)]
enum Next {
    None,
    Named(&'static str),
    Fail,
    Slow,
}

static NEXT: Mutex<Next> = Mutex::new(Next::None);
static CALLS: AtomicUsize = AtomicUsize::new(0);
/// Loads left to hold: each signals `ENTERED` and waits for `RELEASE`.
static GATES: AtomicUsize = AtomicUsize::new(0);
static ENTERED: tokio::sync::Notify = tokio::sync::Notify::const_new();
static RELEASE: tokio::sync::Notify = tokio::sync::Notify::const_new();

/// Takes one of the `GATES`, if any is left.
fn take_gate() -> bool {
    let mut left = GATES.load(Ordering::SeqCst);
    while left > 0 {
        match GATES.compare_exchange(left, left - 1, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => return true,
            Err(now) => left = now,
        }
    }
    false
}

fn next(value: Next) {
    *NEXT.lock().unwrap() = value;
}

struct FakeSource;

#[async_trait]
impl BrandingSource for FakeSource {
    async fn load(&self, _: &AppState) -> AppResult<Option<Branding>> {
        CALLS.fetch_add(1, Ordering::SeqCst);
        let value = *NEXT.lock().unwrap();
        // What was read before the wait is what this load returns (a stale read).
        if take_gate() {
            ENTERED.notify_one();
            RELEASE.notified().await;
        }
        match value {
            Next::None => Ok(None),
            Next::Named(name) => Ok(Some(
                Branding::new()
                    .name(name)
                    .and_then(|b| b.logo(PNG, "image/png"))
                    .and_then(|b| b.logo_dark(WEBP, "image/webp"))
                    .and_then(|b| b.primary("#2563eb"))
                    .map_err(AppError::internal)?,
            )),
            Next::Fail => Err(AppError::internal("the settings table is gone")),
            Next::Slow => {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                Ok(None)
            }
        }
    }
}

fn build(config: Config) -> AppBuilder {
    let mut app = AppBuilder::new(config);
    app.provider(AuthProvider::new());
    app.provider(
        AdminProvider::new()
            .brand("Static CMS")
            .theme(Theme::new().primary("#111111").favicon(PNG, "image/png"))
            .extension_style("acme", ".x{color:red}")
            .branding(FakeSource),
    );
    app
}

async fn app(db: TestDb) -> TestApp {
    app_with_ttl(db, 30).await
}

async fn app_with_ttl(db: TestDb, ttl: u64) -> TestApp {
    db.migrate::<rustcms_migration::Migrator>().await;
    let config = rustcms_testing::test_config(db.url())
        .with("admin.branding_timeout_ms", 200)
        .and_then(|c| c.with("admin.branding_ttl_secs", ttl))
        .unwrap();
    TestApp::with_config(build, config, db).await
}

/// The index's configuration, decoded.
async fn config(app: &TestApp) -> serde_json::Value {
    let index = app.get("/admin").await;
    index.assert_status(StatusCode::OK);
    assert_eq!(index.headers["cache-control"], "no-store");
    let html = index.text();
    let content = html.split("name=\"rustcms-admin\" content=\"").nth(1).unwrap().split('"').next().unwrap();
    let json = content
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&");
    serde_json::from_str(&json).unwrap()
}

fn branding(app: &TestApp) -> std::sync::Arc<AdminBranding> {
    app.state().service::<AdminBranding>().unwrap()
}

fn asset_headers(response: &TestResponse, mime: &str) {
    response.assert_status(StatusCode::OK);
    assert_eq!(response.headers["content-type"], mime);
    assert_eq!(response.headers["x-content-type-options"], "nosniff");
    assert_eq!(response.headers["cross-origin-resource-policy"], "same-origin");
    assert_eq!(response.headers["content-disposition"], "inline");
    let csp = response.headers["content-security-policy"].to_str().unwrap();
    assert!(csp.contains("style-src 'self'") && !csp.contains("unsafe"), "{csp}");
}

db_test!(runtime_branding_reaches_the_index_and_its_assets, |db: TestDb| async move {
    next(Next::Named("Acme <Ops> \"&\""));
    let app = app(db).await;
    let config = config(&app).await;
    assert_eq!(config["brand"], serde_json::json!("Acme <Ops> \"&\""), "escaped in the attribute, intact once read");
    let theme = &config["theme"];
    let logo = theme["logo"].as_str().unwrap();
    assert!(logo.starts_with("assets/brand/logo?v="), "{logo}");
    assert!(theme["logoDark"].as_str().unwrap().starts_with("assets/brand/logo-dark?v="));
    assert!(
        theme["favicon"].as_str().unwrap().starts_with("assets/brand/favicon?v="),
        "the static favicon shows through"
    );
    // The theme's stylesheet first, then the extension's; no static theme file.
    let styles: Vec<&str> =
        config["extensions"]["styles"].as_array().unwrap().iter().map(|s| s.as_str().unwrap()).collect();
    assert_eq!(styles.len(), 2, "{styles:?}");
    assert!(styles[0].starts_with("assets/brand/theme.css?v=") && styles[1].starts_with("assets/ext/acme."));

    let css = app.get(&format!("/admin/{}", styles[0])).await;
    asset_headers(&css, "text/css; charset=utf-8");
    assert_eq!(css.headers["cache-control"], "public, max-age=31536000, immutable");
    assert!(css.text().contains("--primary:#2563eb;") && !css.text().contains("#111111"), "{}", css.text());
    let image = app.get(&format!("/admin/{logo}")).await;
    asset_headers(&image, "image/png");
    assert_eq!(image.body.as_ref(), PNG);
    asset_headers(&app.get(&format!("/admin/{}", theme["logoDark"].as_str().unwrap())).await, "image/webp");
    // Another instance's (or an older page's) version: the current bytes, not cached.
    let stale = app.get("/admin/assets/brand/logo?v=00000000").await;
    asset_headers(&stale, "image/png");
    assert_eq!(stale.headers["cache-control"], "no-cache");
    assert_eq!(app.get("/admin/assets/brand/logo").await.headers["cache-control"], "no-cache");
    let missing = app.get("/admin/assets/brand/nope?v=00000000").await;
    missing.assert_status(StatusCode::NOT_FOUND);
    for header in ["content-security-policy", "x-content-type-options", "cross-origin-resource-policy"] {
        assert!(missing.headers.contains_key(header), "{header} on a 404");
    }
    app.get("/admin/assets/brand/../index.html").await.assert_status(StatusCode::NOT_FOUND);
    let head =
        app.request(axum::http::Request::head(format!("/admin/{logo}")).body(axum::body::Body::empty()).unwrap()).await;
    head.assert_status(StatusCode::OK);
    let post =
        app.request(axum::http::Request::post(format!("/admin/{logo}")).body(axum::body::Body::empty()).unwrap()).await;
    post.assert_status(StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(post.headers["x-content-type-options"], "nosniff");
    assert_eq!(post.headers["cross-origin-resource-policy"], "same-origin");
    // Extension assets are served as before.
    app.get(&format!("/admin/{}", styles[1])).await.assert_status(StatusCode::OK);
});

db_test!(the_cache_loads_once_per_period_and_invalidate_takes_effect_at_once, |db: TestDb| async move {
    next(Next::Named("One"));
    let app = app(db).await;
    let before = CALLS.load(Ordering::SeqCst);
    // Three requests while the first load is held: one load for all.
    GATES.store(1, Ordering::SeqCst);
    let release = async {
        ENTERED.notified().await;
        tokio::task::yield_now().await;
        RELEASE.notify_one();
    };
    let (a, b, c, ()) = tokio::join!(config(&app), config(&app), config(&app), release);
    assert_eq!([&a["brand"], &b["brand"], &c["brand"]], [&serde_json::json!("One"); 3]);
    assert_eq!(CALLS.load(Ordering::SeqCst) - before, 1, "one load for concurrent requests");
    next(Next::Named("Two"));
    assert_eq!(config(&app).await["brand"], serde_json::json!("One"), "cached");
    branding(&app).invalidate();
    assert_eq!(config(&app).await["brand"], serde_json::json!("Two"));
    assert_eq!(CALLS.load(Ordering::SeqCst) - before, 2);
});

db_test!(a_load_racing_invalidate_is_not_kept, |db: TestDb| async move {
    next(Next::Named("Old"));
    let app = app(db).await;
    GATES.store(1, Ordering::SeqCst);
    let before = CALLS.load(Ordering::SeqCst);
    let request = config(&app);
    let race = async {
        ENTERED.notified().await;
        // Saved meanwhile: the load in flight read the old value.
        next(Next::Named("New"));
        branding(&app).invalidate();
        RELEASE.notify_one();
    };
    let (shown, ()) = tokio::join!(request, race);
    // The stale load is discarded and retried once.
    assert_eq!(shown["brand"], serde_json::json!("New"));
    assert_eq!(CALLS.load(Ordering::SeqCst) - before, 2);
    assert_eq!(config(&app).await["brand"], serde_json::json!("New"));
    assert_eq!(CALLS.load(Ordering::SeqCst) - before, 2, "the retry was kept");
});

db_test!(an_invalidation_during_the_retry_is_not_kept_either, |db: TestDb| async move {
    next(Next::Named("Old"));
    let app = app(db).await;
    GATES.store(2, Ordering::SeqCst);
    let before = CALLS.load(Ordering::SeqCst);
    let race = async {
        for value in ["Mid", "New"] {
            ENTERED.notified().await;
            next(Next::Named(value));
            branding(&app).invalidate();
            RELEASE.notify_one();
        }
    };
    let (shown, ()) = tokio::join!(config(&app), race);
    // The retry read "Mid" and is shown, but not kept: the next request loads again.
    assert_eq!(shown["brand"], serde_json::json!("Mid"));
    assert_eq!(CALLS.load(Ordering::SeqCst) - before, 2);
    assert_eq!(config(&app).await["brand"], serde_json::json!("New"));
    assert_eq!(CALLS.load(Ordering::SeqCst) - before, 3);
});

db_test!(the_branding_is_loaded_again_after_its_period, |db: TestDb| async move {
    next(Next::Named("One"));
    let app = app_with_ttl(db, 1).await;
    let before = CALLS.load(Ordering::SeqCst);
    config(&app).await;
    next(Next::Named("Two"));
    assert_eq!(config(&app).await["brand"], serde_json::json!("One"));
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    assert_eq!(config(&app).await["brand"], serde_json::json!("Two"));
    assert_eq!(CALLS.load(Ordering::SeqCst) - before, 2);
});

db_test!(a_failing_or_slow_source_shows_the_static_theme, |db: TestDb| async move {
    next(Next::Fail);
    let app = app(db).await;
    let before = CALLS.load(Ordering::SeqCst);
    let shown = config(&app).await;
    assert_eq!(shown["brand"], serde_json::json!("Static CMS"));
    let css = app.get(&format!("/admin/{}", shown["extensions"]["styles"][0].as_str().unwrap())).await;
    assert!(css.text().contains("--primary:#111111;"));
    assert!(shown["theme"].get("logo").is_none());
    config(&app).await;
    assert_eq!(CALLS.load(Ordering::SeqCst) - before, 1, "a failure is cached too");
    next(Next::Slow);
    branding(&app).invalidate();
    assert_eq!(config(&app).await["brand"], serde_json::json!("Static CMS"), "a timeout");
    next(Next::None);
    branding(&app).invalidate();
    assert_eq!(config(&app).await["brand"], serde_json::json!("Static CMS"));
});

db_test!(static_mode_never_calls_the_source, |db: TestDb| async move {
    next(Next::Named("Ignored"));
    // `RUSTCMS_ADMIN__BRANDING=static` sets it (the loader's mapping is tested in core).
    db.migrate::<rustcms_migration::Migrator>().await;
    let settings = rustcms_testing::test_config(db.url()).with("admin.branding", "static").unwrap();
    let app = TestApp::with_config(build, settings, db).await;
    let before = CALLS.load(Ordering::SeqCst);
    assert_eq!(config(&app).await["brand"], serde_json::json!("Static CMS"));
    let branding = branding(&app);
    assert!(branding.has_source() && !branding.is_runtime());
    assert_eq!(branding.probe(app.state()).await.unwrap(), None);
    assert_eq!(CALLS.load(Ordering::SeqCst), before);
});

db_test!(the_panel_response_names_the_current_brand, |db: TestDb| async move {
    next(Next::Named("Acme"));
    let app = app(db).await;
    let user = rustcms_testing::UserFactory::new().create(app.state().db()).await;
    let auth = app.state().service::<rustcms_auth::AuthState>().unwrap();
    let rbac = rustcms_auth::RbacService::new(app.state().db(), &auth.resolver);
    let role = rustcms_auth::entities::role::super_admin_id(app.state().db(), rustcms_auth::ADMIN_GUARD)
        .await
        .unwrap()
        .unwrap();
    rbac.assign_role(rustcms_auth::Actor::System, user.id, role).await.unwrap();
    let panel = app.acting_as(&user).await.get(&app.admin_api("panel")).await.json();
    assert_eq!(panel["brand"], serde_json::json!("Acme"));
    assert_eq!(branding(&app).probe(app.state()).await.unwrap().as_deref(), Some("Acme"));
});

#[test]
fn bad_branding_settings_stop_the_boot() {
    let settings: [(&str, serde_json::Value); 4] = [
        ("admin.branding", "off".into()),
        ("admin.branding_ttl_secs", 0.into()),
        ("admin.branding_ttl_secs", 3601.into()),
        ("admin.branding_timeout_ms", 0.into()),
    ];
    for (key, value) in settings {
        let failed = std::panic::catch_unwind(|| {
            let config = rustcms_testing::test_config("sqlite::memory:").with(key, value.clone()).unwrap();
            build(config);
        });
        assert!(failed.is_err(), "{key} = {value}");
    }
}

db_test!(the_index_carries_the_socket_message_limit, |db: TestDb| async move {
    next(Next::Named("Acme"));
    db.migrate::<rustcms_migration::Migrator>().await;
    // Static branding (rendered once) and runtime branding (per request) alike.
    for mode in ["static", "runtime"] {
        let settings = rustcms_testing::test_config(db.url())
            .with("admin.branding", mode)
            .and_then(|c| c.with("realtime.max_message_bytes", 4096))
            .unwrap();
        let app = TestApp::with_config(build, settings, db.clone()).await;
        for _ in 0..2 {
            assert_eq!(config(&app).await["realtime"], serde_json::json!({"maxMessageBytes": 4096}), "{mode}");
        }
        let index = app.get("/admin").await;
        let csp = index.headers["content-security-policy"].to_str().unwrap().to_owned();
        assert!(csp.starts_with("default-src 'self'; script-src 'self';") && !csp.contains("unsafe"), "{csp}");
    }
    // Beyond 1 MiB: what the server applies.
    let settings = rustcms_testing::test_config(db.url()).with("realtime.max_message_bytes", 1u64 << 30).unwrap();
    let app = TestApp::with_config(build, settings, db.clone()).await;
    assert_eq!(config(&app).await["realtime"], serde_json::json!({"maxMessageBytes": 1024 * 1024}));
});
