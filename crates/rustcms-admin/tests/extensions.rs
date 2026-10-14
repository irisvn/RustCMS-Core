//! Extension scripts and styles: served as immutable, content-hashed assets
//! (before sign-in), listed in the panel's runtime configuration.

mod support;

use axum::http::StatusCode;
use rustcms_testing::{TestDb, db_test};
use support::app::setup;

db_test!(extensions_are_served_and_announced, |db: TestDb| async move {
    let (app, _) = setup(db).await;
    let page = app.get("/admin/login").await;
    let html = page.text();
    // Hash = first 8 hex of SHA-256 of the body.
    let script = "assets/ext/test-widgets.";
    assert!(html.contains("&quot;extensions&quot;"), "{html}");
    let start = html.find(script).expect("script listed");
    let file: String = html[start..].chars().take_while(|c| *c != '&').collect();
    assert!(file.ends_with(".js"), "{file}");

    // Served without a session, with the panel's security headers.
    let served = app.get(&format!("/admin/{file}")).await;
    served.assert_status(StatusCode::OK);
    assert_eq!(served.text(), "window.rustcmsTest = 1;");
    assert_eq!(served.headers["content-type"], "text/javascript; charset=utf-8");
    assert!(served.headers["cache-control"].to_str().unwrap().contains("immutable"));
    assert_eq!(served.headers["x-content-type-options"], "nosniff");
    // Another hash (an older deployment) or name: not found.
    app.get("/admin/assets/ext/test-widgets.00000000.js").await.assert_status(StatusCode::NOT_FOUND);
    app.get("/admin/assets/ext/other.js").await.assert_status(StatusCode::NOT_FOUND);
});

#[test]
#[should_panic(expected = "invalid extension name")]
fn extension_names_are_checked() {
    let _ = rustcms_admin::AdminProvider::new().extension_script("../evil", "x");
}

#[test]
#[should_panic(expected = "added twice")]
fn extensions_are_added_once() {
    let _ = rustcms_admin::AdminProvider::new().extension_script("a", "x").extension_script("a", "y");
}
