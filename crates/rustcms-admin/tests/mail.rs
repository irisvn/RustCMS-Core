//! Mail: queued with the change (outbox), rendered and sent by the memory
//! driver; the Mail settings page within the operator's policy, the password
//! kept only for the server it was given for; the test mail admitted before
//! its body, bounded, answering codes. SQLite, PostgreSQL, MySQL.

mod support;

use axum::http::{Method, StatusCode};
use rustcms_core::mail::{self, MailJob, MailSettings};
use rustcms_core::settings;
use rustcms_testing::{TestApp, TestDb, db_test};
use serde_json::{Value, json};

use support::app::setup;

async fn put(
    app: &TestApp,
    client: &rustcms_testing::TestClient<'_>,
    version: &str,
    data: Value,
) -> rustcms_testing::TestResponse {
    client.send(Method::PUT, &app.admin_api("settings/mail"), Some(&json!({"version": version, "data": data}))).await
}

async fn audit_entries(app: &TestApp) -> Vec<rustcms_core::models::audit_log::Model> {
    use sea_orm::EntityTrait;
    rustcms_core::models::audit_log::Entity::find().all(app.state().db()).await.unwrap()
}

fn server() -> Value {
    json!({
        "host": "smtp.example.com", "port": 587, "encryption": "starttls", "username": "app",
        "password": "hunter2-secret", "from_address": "no-reply@example.com", "from_name": "Test CMS",
    })
}

db_test!(mails_are_queued_with_the_change_and_sent_by_the_driver, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let job = MailJob {
        to: vec!["learner@example.com".into()],
        template: "rustcms.test".into(),
        locale: "vi".into(),
        data: json!({}),
    };
    // Rolled back: no job, no mail.
    let txn = rustcms_core::db::begin_write(app.state().db()).await.unwrap();
    mail::send_later(&txn, job.clone()).await.unwrap();
    txn.rollback().await.unwrap();
    rustcms_core::queue::run_until_idle(app.state(), None, 10).await.unwrap();
    assert!(app.mails().is_empty());

    // Not configured yet: failed at once (not retried), saying so.
    let txn = rustcms_core::db::begin_write(app.state().db()).await.unwrap();
    mail::send_later(&txn, job.clone()).await.unwrap();
    txn.commit().await.unwrap();
    rustcms_core::queue::run_until_idle(app.state(), None, 10).await.unwrap();
    assert!(app.mails().is_empty());
    use sea_orm::EntityTrait;
    let jobs = rustcms_core::models::job::Entity::find().all(app.state().db()).await.unwrap();
    assert_eq!(jobs.len(), 1);
    assert!(jobs[0].failed_at.is_some() && jobs[0].attempts == 1, "not retried: {jobs:?}");
    assert!(jobs[0].last_error.as_deref().unwrap_or_default().starts_with("mail_not_configured"), "{jobs:?}");

    // A sender (the memory driver needs no server).
    let admin = app.acting_as(&users.admin).await;
    let saved = put(&app, &admin, "0", json!({"from_address": "no-reply@example.com", "from_name": "Test CMS"})).await;
    saved.assert_status(StatusCode::OK);
    let txn = rustcms_core::db::begin_write(app.state().db()).await.unwrap();
    mail::send_later(&txn, job).await.unwrap();
    txn.commit().await.unwrap();
    rustcms_core::queue::run_until_idle(app.state(), None, 10).await.unwrap();
    let mails = app.take_mails();
    assert_eq!(mails.len(), 1);
    assert_eq!(mails[0].to, ["learner@example.com"]);
    assert_eq!(mails[0].from, "Test CMS <no-reply@example.com>");
    assert!(mails[0].subject.contains("thư thử"), "{}", mails[0].subject);
    assert!(mails[0].text.contains("http://localhost:8000"), "{}", mails[0].text);
    assert!(mails[0].message_id.ends_with("@localhost"), "{}", mails[0].message_id);
});

db_test!(mail_settings_follow_the_policy_and_bind_the_password, |db: TestDb| async move {
    let config =
        rustcms_testing::test_config(db.url()).with("mail.allowed_hosts", ["smtp.example.com", "*.mail.test"]).unwrap();
    let (app, users) = support::app::setup_with(db, config).await;
    let admin = app.acting_as(&users.admin).await;

    let mut outside = server();
    outside["host"] = json!("10.0.0.1");
    put(&app, &admin, "0", outside).await.assert_validation_error("host");
    let mut insecure = server();
    insecure["encryption"] = json!("none");
    put(&app, &admin, "0", insecure).await.assert_validation_error("encryption");
    let mut port = server();
    port["port"] = json!(25);
    put(&app, &admin, "0", port).await.assert_validation_error("port");

    put(&app, &admin, "0", server()).await.assert_status(StatusCode::OK);
    let read: MailSettings = settings::get(app.state()).await.unwrap();
    assert_eq!(read.password.expose(), Some("hunter2-secret"));
    // Another server without the password again: the saved one is not sent there.
    let moved = put(&app, &admin, "1", json!({"host": "relay.mail.test"})).await;
    moved.assert_status(StatusCode::OK);
    assert_eq!(moved.json()["secrets"]["password"], json!("unset"));
    let read: MailSettings = settings::get(app.state()).await.unwrap();
    assert!(!read.password.is_set());
    // Responses and the audit log never carry it.
    let audit = format!("{:?}", audit_entries(&app).await);
    assert!(!audit.contains("hunter2"), "{audit}");
});

db_test!(test_mails_are_admitted_bounded_and_answer_codes, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    let editor = app.acting_as(&users.editor).await;
    let url = app.admin_api("settings/mail/test");
    editor.post_json(&url, &json!({"to": "a@example.com"})).await.assert_status(StatusCode::FORBIDDEN);
    let page = admin.get(&app.admin_api("settings/mail")).await.json();
    assert_eq!(page["abilities"]["test"], json!(true));

    // Nothing saved: a code, no mail.
    let answer = admin.post_json(&url, &json!({"to": "a@example.com"})).await;
    answer.assert_status(StatusCode::OK);
    assert_eq!(answer.json(), json!({"ok": false, "code": "mail_not_configured"}));
    admin.post_json(&url, &json!({"to": "not an address"})).await.assert_validation_error("to");
    admin.post_json(&url, &json!({"to": "a@example.com\r\nBcc: x@evil.test"})).await.assert_validation_error("to");

    put(&app, &admin, "0", server()).await.assert_status(StatusCode::OK);
    let sent = admin.post_json(&url, &json!({"to": "Alice@Example.com"})).await;
    assert_eq!(sent.json(), json!({"ok": true}));
    let mails = app.take_mails();
    assert_eq!((mails.len(), mails[0].to.clone()), (1, vec!["Alice@Example.com".to_owned()]));
    // Five a minute per user (four sent so far, refused ones included).
    admin.post_json(&url, &json!({"to": "a@example.com"})).await.assert_status(StatusCode::OK);
    admin.post_json(&url, &json!({"to": "a@example.com"})).await.assert_status(StatusCode::TOO_MANY_REQUESTS);
    // The audit entry names the recipient masked.
    let audit = format!("{:?}", audit_entries(&app).await);
    assert!(audit.contains("A***@Example.com") && !audit.contains("Alice@"), "{audit}");
});

/// A real SMTP server (Mailpit): `RUSTCMS_TEST_SMTP=127.0.0.1:51025`.
fn smtp_server() -> Option<(String, u16)> {
    let Ok(server) = std::env::var("RUSTCMS_TEST_SMTP") else {
        let required = std::env::var("RUSTCMS_TEST_REQUIRE").unwrap_or_default();
        assert!(
            !required.split(',').any(|b| b.trim() == "smtp"),
            "RUSTCMS_TEST_SMTP is required (RUSTCMS_TEST_REQUIRE) but not set"
        );
        eprintln!("skipped: RUSTCMS_TEST_SMTP is not set");
        return None;
    };
    let (host, port) = server.rsplit_once(':').expect("RUSTCMS_TEST_SMTP=host:port");
    Some((host.to_owned(), port.parse().expect("port")))
}

db_test!(smtp_sends_through_a_real_server_and_requires_starttls, |db: TestDb| async move {
    let Some((host, port)) = smtp_server() else { return };
    let config = rustcms_testing::test_config(db.url())
        .with("mail.driver", "smtp")
        .and_then(|c| c.with("mail.allow_insecure", true))
        .and_then(|c| c.with("mail.allowed_ports", [port]))
        .unwrap();
    let (app, users) = support::app::setup_with(db, config).await;
    let admin = app.acting_as(&users.admin).await;
    let url = app.admin_api("settings/mail/test");
    let plain = json!({"host": host, "port": port, "encryption": "none", "username": "",
        "from_address": "no-reply@example.com", "from_name": "Test CMS"});
    put(&app, &admin, "0", plain).await.assert_status(StatusCode::OK);
    let sent = admin.post_json(&url, &json!({"to": "smtp-test@example.com"})).await;
    assert_eq!(sent.json(), json!({"ok": true}));
    // STARTTLS required: a server that does not offer it is refused, nothing sent in clear.
    let starttls = put(&app, &admin, "1", json!({"encryption": "starttls"})).await;
    starttls.assert_status(StatusCode::OK);
    let refused = admin.post_json(&url, &json!({"to": "smtp-test@example.com"})).await;
    assert_eq!(refused.json(), json!({"ok": false, "code": "smtp_tls"}));
});

/// A TLS SMTP server (Mailpit, STARTTLS, any credentials) and the root it is
/// signed with: `RUSTCMS_TEST_SMTP_TLS=localhost:51026`, `RUSTCMS_TEST_SMTP_TLS_CA=…`.
fn tls_server() -> Option<(String, u16, String)> {
    let (Ok(server), Ok(ca)) = (std::env::var("RUSTCMS_TEST_SMTP_TLS"), std::env::var("RUSTCMS_TEST_SMTP_TLS_CA"))
    else {
        let required = std::env::var("RUSTCMS_TEST_REQUIRE").unwrap_or_default();
        assert!(
            !required.split(',').any(|b| b.trim() == "smtp_tls"),
            "RUSTCMS_TEST_SMTP_TLS and RUSTCMS_TEST_SMTP_TLS_CA are required (RUSTCMS_TEST_REQUIRE) but not set"
        );
        eprintln!("skipped: RUSTCMS_TEST_SMTP_TLS is not set");
        return None;
    };
    let (host, port) = server.rsplit_once(':').expect("RUSTCMS_TEST_SMTP_TLS=host:port");
    Some((host.to_owned(), port.parse().expect("port"), ca))
}

db_test!(smtp_over_tls_authenticates_and_verifies_the_certificate, |db: TestDb| async move {
    let Some((host, port, ca)) = tls_server() else { return };
    let config = rustcms_testing::test_config(db.url())
        .with("mail.driver", "smtp")
        .and_then(|c| c.with("mail.allowed_ports", [port]))
        .and_then(|c| c.with("mail.tls_ca_file", ca.as_str()))
        .unwrap();
    let (app, users) = support::app::setup_with(db, config).await;
    let admin = app.acting_as(&users.admin).await;
    let url = app.admin_api("settings/mail/test");
    let secret = "tls-password-never-shown";
    let server = json!({"host": host, "port": port, "encryption": "starttls", "username": "app",
        "password": secret, "from_address": "no-reply@example.com", "from_name": "Test CMS"});
    put(&app, &admin, "0", server).await.assert_status(StatusCode::OK);
    // STARTTLS, the certificate checked against the trusted root, AUTH: sent.
    let sent = admin.post_json(&url, &json!({"to": "tls-test@example.com"})).await;
    assert_eq!(sent.json(), json!({"ok": true}));
    // The same server by another name than its certificate's: refused before AUTH.
    let renamed = put(&app, &admin, "1", json!({"host": "127.0.0.1", "password": secret})).await;
    renamed.assert_status(StatusCode::OK);
    let refused = admin.post_json(&url, &json!({"to": "tls-test@example.com"})).await;
    assert_eq!(refused.json(), json!({"ok": false, "code": "smtp_tls"}));
    let audit = format!("{:?}", audit_entries(&app).await);
    assert!(!audit.contains(secret), "{audit}");
});

db_test!(smtp_over_tls_refuses_an_untrusted_certificate, |db: TestDb| async move {
    let Some((host, port, _)) = tls_server() else { return };
    // No extra root: the self-signed certificate is not trusted.
    let config = rustcms_testing::test_config(db.url())
        .with("mail.driver", "smtp")
        .and_then(|c| c.with("mail.allowed_ports", [port]))
        .unwrap();
    let (app, users) = support::app::setup_with(db, config).await;
    let admin = app.acting_as(&users.admin).await;
    let server = json!({"host": host, "port": port, "encryption": "starttls", "username": "app",
        "password": "x", "from_address": "no-reply@example.com"});
    put(&app, &admin, "0", server).await.assert_status(StatusCode::OK);
    let refused = admin.post_json(&app.admin_api("settings/mail/test"), &json!({"to": "tls-test@example.com"})).await;
    assert_eq!(refused.json(), json!({"ok": false, "code": "smtp_tls"}));
});
