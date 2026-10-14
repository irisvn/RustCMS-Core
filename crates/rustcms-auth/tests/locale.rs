//! The interface language: saved per user, else the browser's, else
//! `app.locale`; choosing one is not a credential change.

mod support;

use axum::http::StatusCode;
use rustcms_migration::Migrator;
use rustcms_testing::{TestDb, db_test};
use serde_json::json;
use support::{PASSWORD, reload, setup};

db_test!(the_language_follows_the_user_then_the_browser, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    let me = app.admin_api("auth/me");
    let locale = app.admin_api("auth/locale");

    // Nothing chosen: the browser's best offered language, else the default.
    let browser = app.acting_as(&seed.member).await.languages("fr, vi;q=0.8, en;q=0.5");
    browser
        .get(&me)
        .await
        .assert_status(StatusCode::OK)
        .assert_json("/locale", json!("vi"))
        .assert_json("/preferredLocale", json!(null))
        .assert_json("/locales", json!(["en", "vi"]));
    app.acting_as(&seed.member).await.get(&me).await.assert_json("/locale", json!("en"));

    // A choice is saved on the account and wins over the browser, on any device.
    let before = reload(&app, &seed.member).await;
    let client = app.acting_as(&seed.member).await.languages("en");
    client
        .post_json(&locale, &json!({"locale": "vi"}))
        .await
        .assert_status(StatusCode::OK)
        .assert_json("/locale", json!("vi"));
    client.get(&me).await.assert_json("/locale", json!("vi")).assert_json("/preferredLocale", json!("vi"));
    let after = reload(&app, &seed.member).await;
    assert_eq!(after.locale.as_deref(), Some("vi"));
    // Not a credential change: sessions and tokens live on.
    assert_eq!(after.auth_version, before.auth_version);
    client.get(&me).await.assert_status(StatusCode::OK);

    // `null` goes back to following the browser.
    client.post_json(&locale, &json!({"locale": null})).await.assert_json("/locale", json!("en"));
    assert_eq!(reload(&app, &seed.member).await.locale, None);

    // Only offered languages; malformed bodies are refused.
    client.post_json(&locale, &json!({"locale": "fr"})).await.assert_validation_error("locale");
    client.post_json(&locale, &json!({"locale": "<script>"})).await.assert_validation_error("locale");
    let extra = client.post_json(&locale, &json!({"locale": "vi", "admin": true})).await;
    assert!(extra.status.is_client_error());
    assert_eq!(reload(&app, &seed.member).await.locale, None);
});

db_test!(choosing_a_language_needs_an_admin_session, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    let locale = app.admin_api("auth/locale");
    let body = json!({"locale": "vi"});
    // Guests, API tokens and other origins change nothing.
    let guest = app.client();
    guest.fetch_csrf().await;
    guest.post_json(&locale, &body).await.assert_status(StatusCode::UNAUTHORIZED);
    let token = app.with_token(&seed.member, &["*"]).await;
    let status = token.post_json(&locale, &body).await.status;
    assert!(matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN), "{status}");
    let foreign = app.acting_as(&seed.member).await.origin(Some("https://evil.example"));
    foreign.post_json(&locale, &body).await.assert_status(StatusCode::FORBIDDEN);
    let no_csrf = app.acting_as(&seed.member).await;
    no_csrf.set_csrf(None);
    no_csrf.post_json(&locale, &body).await.assert_status(StatusCode::FORBIDDEN);
    assert_eq!(reload(&app, &seed.member).await.locale, None);
});

db_test!(a_removed_language_falls_back, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db.clone()).await;
    app.acting_as(&seed.member).await.post_json(&app.admin_api("auth/locale"), &json!({"locale": "vi"})).await;
    // The deployment no longer offers Vietnamese: the saved choice is ignored, not an error.
    let mut config = rustcms_testing::test_config(db.url());
    config.app.key = app.state().config().app.key.clone();
    config.app.locales = vec!["en".into()];
    let english = rustcms_testing::TestApp::with_config(support::build, config, db).await;
    english
        .acting_as(&seed.member)
        .await
        .languages("vi")
        .get(&english.admin_api("auth/me"))
        .await
        .assert_json("/locale", json!("en"))
        .assert_json("/preferredLocale", json!("vi"));
});

db_test!(refused_sign_ins_carry_the_same_issue, migrate = Migrator, |db: TestDb| async move {
    let (app, seed) = setup(db).await;
    let client = app.client();
    client.fetch_csrf().await;
    let login = app.admin_api("auth/login");
    let wrong = client.post_json(&login, &json!({"email": seed.member.email, "password": "nope"})).await;
    let unknown = client.post_json(&login, &json!({"email": "ghost@example.test", "password": PASSWORD})).await;
    assert_eq!(wrong.json()["issues"], json!({"email": [{"code": "bad_credentials", "params": {}}]}));
    assert_eq!(wrong.json(), unknown.json());
});
