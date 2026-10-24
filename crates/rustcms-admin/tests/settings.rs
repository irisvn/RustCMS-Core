//! Settings pages: authorization before the body, validation, versions,
//! encrypted secrets that never leave the store, audit. All databases.

mod support;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use rustcms_admin::{SettingField, SettingsMeta, SettingsPage};
use rustcms_auth::{ADMIN_GUARD, Actor, AuthState, RbacService};
use rustcms_core::models::{audit_log, setting};
use rustcms_core::settings::{self, Secret, Settings};
use rustcms_testing::{TestApp, TestClient, TestDb, TestResponse, db_test};
use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use support::app::{GeneralSettings, Users, setup};

fn url(app: &TestApp) -> String {
    app.admin_api("settings/general")
}

async fn put(app: &TestApp, client: &TestClient<'_>, version: &str, data: Value) -> TestResponse {
    client.fetch_csrf().await;
    client.send(Method::PUT, &url(app), Some(&json!({"version": version, "data": data}))).await
}

async fn put_raw(app: &TestApp, client: &TestClient<'_>, body: &str) -> TestResponse {
    client.fetch_csrf().await;
    let request = Request::builder()
        .method(Method::PUT)
        .uri(url(app))
        .header("content-type", "application/json")
        .body(Body::from(body.to_owned()))
        .unwrap();
    client.request(request).await
}

/// Gives `user` a role with `permissions` (admin guard).
async fn grant(app: &TestApp, user: &rustcms_core::models::user::Model, name: &str, permissions: &[&str]) {
    let auth = app.state().service::<AuthState>().unwrap();
    let rbac = RbacService::new(app.state().db(), &auth.resolver);
    let permissions: Vec<String> = permissions.iter().map(|p| (*p).to_owned()).collect();
    let role = rbac.create_role(Actor::System, ADMIN_GUARD, name, &permissions).await.unwrap();
    rbac.assign_role(Actor::System, user.id, role.id).await.unwrap();
}

async fn entries(app: &TestApp) -> Vec<audit_log::Model> {
    audit_log::Entity::find()
        .all(app.state().db())
        .await
        .unwrap()
        .into_iter()
        .filter(|e| e.subject_type == "settings")
        .collect()
}

async fn app(db: TestDb) -> (TestApp, Users) {
    setup(db).await
}

db_test!(defaults_are_shown_and_access_is_per_group, |db: TestDb| async move {
    let (app, users) = app(db).await;
    let admin = app.acting_as(&users.admin).await;
    let page = admin.get(&url(&app)).await;
    page.assert_status(StatusCode::OK);
    let body = page.json();
    assert_eq!(body["version"], json!("0"));
    assert_eq!(body["data"]["site_name"], json!("Academy"));
    assert_eq!(body["data"]["class_size"], json!(30));
    assert!(body["data"].get("smtp_password").is_none(), "secrets are not data");
    assert_eq!(body["secrets"], json!({"api_key": "unset", "smtp_password": "unset"}));
    assert_eq!(body["abilities"]["update"], json!(true));
    support::contract("settings.json", &body);
    let vi = app.acting_as(&users.admin).await.languages("vi");
    let translated = vi.get(&url(&app)).await.json();
    assert_eq!(translated["label"], json!("Chung"));
    support::contract("settings.vi.json", &translated);
    let panel = admin.get(&app.admin_api("panel")).await.json();
    assert_eq!(
        panel["settings"],
        json!([{"group": "general", "label": "General", "icon": "settings"}, {"group": "mail", "label": "Mail", "icon": "mail"}])
    );

    // No settings permission: 403 for any group (existing or not), no link.
    let member = app.acting_as(&users.member).await;
    member.get(&url(&app)).await.assert_status(StatusCode::FORBIDDEN);
    member.get(&app.admin_api("settings/nope")).await.assert_status(StatusCode::FORBIDDEN);
    assert_eq!(member.get(&app.admin_api("panel")).await.json()["settings"], json!([]));
    // Someone who may view settings learns that a group does not exist.
    admin.get(&app.admin_api("settings/nope")).await.assert_status(StatusCode::NOT_FOUND);

    // View only: read-only form, and no write.
    grant(&app, &users.member, "viewer", &["settings.general.view"]).await;
    let viewer = app.acting_as(&users.member).await;
    let page = viewer.get(&url(&app)).await.json();
    assert_eq!(page["abilities"]["update"], json!(false));
    let inputs = &page["form"]["components"][0]["components"];
    assert_eq!(inputs[0]["disabled"], json!(true));
    put(&app, &viewer, "0", json!({"site_name": "X"})).await.assert_status(StatusCode::FORBIDDEN);
});

db_test!(authorization_comes_before_the_body, |db: TestDb| async move {
    let (app, users) = app(db).await;
    let member = app.acting_as(&users.member).await;
    for body in ["{not json", r#"{"version": 1, "data": 5}"#, r#"{"version": "0", "data": {"site_name": null}}"#] {
        put_raw(&app, &member, body).await.assert_status(StatusCode::FORBIDDEN);
    }
    let admin = app.acting_as(&users.admin).await;
    put_raw(&app, &admin, "{not json").await.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    put_raw(&app, &admin, r#"{"version": 1, "data": {}}"#).await.assert_validation_error("version");
    put_raw(&app, &admin, r#"{"version": "0"}"#).await.assert_validation_error("data");
});

db_test!(saving_validates_versions_and_audits, |db: TestDb| async move {
    let (app, users) = app(db).await;
    let admin = app.acting_as(&users.admin).await;
    let saved = put(&app, &admin, "0", json!({"site_name": "LMS", "class_size": 25, "links": {"Docs": "/docs"}})).await;
    saved.assert_status(StatusCode::OK);
    let body = saved.json();
    assert_eq!((body["version"].clone(), body["data"]["site_name"].clone()), (json!("1"), json!("LMS")));
    let read: GeneralSettings = settings::get(app.state()).await.unwrap();
    assert_eq!((read.site_name.as_str(), read.class_size, read.links.len()), ("LMS", 25, 1));

    // Stale version: refused with the `stale` form issue, nothing written.
    let stale = put(&app, &admin, "0", json!({"site_name": "Old"})).await;
    stale.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(stale.json()["formIssues"][0]["code"], json!("stale"));
    // The same values: no new version, no entry.
    let same = put(&app, &admin, "1", json!({"site_name": "LMS"})).await;
    assert_eq!(same.json()["version"], json!("1"));
    let all = entries(&app).await;
    assert_eq!(all.len(), 1);
    assert_eq!(
        (all[0].event.as_str(), all[0].subject_key.as_deref(), all[0].actor_user_id),
        ("settings.updated", Some("general"), Some(users.admin.id))
    );
    assert_eq!(
        all[0].changes,
        Some(json!({"site_name": ["Academy", "LMS"], "class_size": [30, 25], "links": [{}, {"Docs": "/docs"}]}))
    );

    // Field rules and the application's check.
    let cases = [
        (json!({"site_name": null}), "site_name", "required"),
        (json!({"site_name": "x".repeat(21)}), "site_name", "max_length"),
        (json!({"class_size": "3"}), "class_size", "integer"),
        (json!({"class_size": 0}), "class_size", "out_of_range"),
        (json!({"level": "expert"}), "level", "not_option"),
        (json!({"links": {"a": "1", "b": "2", "c": "3", "d": "4"}}), "links", "max_items"),
        (json!({"starts": "tomorrow"}), "starts", "date"),
        (json!({"version": "1"}), "version", "not_allowed"),
        (json!({"smtp_password": ""}), "smtp_password", "empty"),
        (json!({"smtp_password": 5}), "smtp_password", "string"),
    ];
    for (data, field, code) in cases {
        let response = put(&app, &admin, "1", data.clone()).await;
        response.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(response.json()["issues"][field][0]["code"], json!(code), "{data}");
    }
    assert_eq!(entries(&app).await.len(), 1, "refused writes record nothing");
    assert_eq!(admin.get(&url(&app)).await.json()["version"], json!("1"));
});

db_test!(after_save_runs_in_the_save_with_old_and_new_values, |db: TestDb| async move {
    let (app, users) = app(db).await;
    let admin = app.acting_as(&users.admin).await;
    let saved = |entry: (&str, &str, bool)| {
        support::app::SAVED_GENERAL
            .lock()
            .unwrap()
            .iter()
            .any(|(b, a, secret, seen)| (b.as_str(), a.as_str(), *secret) == entry && *seen)
    };
    // The first save: the defaults before, the saved values after (secrets readable).
    put(&app, &admin, "0", json!({"site_name": "LMS", "smtp_password": "pw-1"})).await.assert_status(StatusCode::OK);
    assert!(saved(("Academy", "LMS", true)));
    put(&app, &admin, "1", json!({"site_name": "School"})).await.assert_status(StatusCode::OK);
    assert!(saved(("LMS", "School", true)));
    // Nothing changed: nothing written, no hook (no `School` → `School` call;
    // other tests of this process record their own names meanwhile).
    put(&app, &admin, "2", json!({"site_name": "School"})).await.assert_status(StatusCode::OK);
    let unchanged = |(b, a, _, _): &(String, String, bool, bool)| b == "School" && a == "School";
    assert!(!support::app::SAVED_GENERAL.lock().unwrap().iter().any(unchanged));
    // The hook fails: the save, its version and its entry roll back.
    put(&app, &admin, "2", json!({"site_name": "rollback"})).await.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
    let read: GeneralSettings = settings::get(app.state()).await.unwrap();
    assert_eq!(read.site_name, "School");
    assert_eq!(admin.get(&url(&app)).await.json()["version"], json!("2"));
    assert_eq!(entries(&app).await.len(), 2);
    // Its messages never quote a secret, as it was or as saved.
    let refused = put(&app, &admin, "2", json!({"site_name": "leak-old", "smtp_password": "pw-2"})).await;
    refused.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    let text = refused.text();
    assert!(!text.contains("pw-1") && !text.contains("pw-2") && text.contains("[redacted]"), "{text}");
    // A replacement containing the old secret is redacted whole, never in part.
    let refused = put(&app, &admin, "2", json!({"site_name": "leak-new", "smtp_password": "pw-1-new"})).await;
    refused.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    let text = refused.text();
    assert!(!text.contains("-new") && text.contains("[redacted]"), "{text}");
    // Any message the hook returns, not only validation errors.
    let refused = put(&app, &admin, "2", json!({"site_name": "leak-bad", "smtp_password": "pw-3"})).await;
    refused.assert_status(StatusCode::BAD_REQUEST);
    let text = refused.text();
    assert!(!text.contains("pw-3") && text.contains("[redacted]"), "{text}");
});

db_test!(a_stored_value_that_no_longer_fits_can_be_replaced, |db: TestDb| async move {
    let (app, users) = app(db).await;
    let admin = app.acting_as(&users.admin).await;
    put(&app, &admin, "0", json!({"site_name": "LMS"})).await.assert_status(StatusCode::OK);
    // The declaration changed since: the stored `class_size` is no longer an i32.
    let row = setting::Entity::find_by_id("general".to_owned()).one(app.state().db()).await.unwrap().unwrap();
    let mut value = row.value.clone();
    value["class_size"] = json!("thirty");
    let mut active: setting::ActiveModel = row.into();
    active.value = sea_orm::Set(value);
    sea_orm::ActiveModelTrait::update(active, app.state().db()).await.unwrap();
    // Replacing it saves; the hook sees the default as the value it replaced.
    put(&app, &admin, "1", json!({"class_size": 12})).await.assert_status(StatusCode::OK);
    let read: GeneralSettings = settings::get(app.state()).await.unwrap();
    assert_eq!((read.site_name.as_str(), read.class_size), ("LMS", 12));
    assert!(support::app::SAVED_GENERAL.lock().unwrap().iter().any(|(b, a, _, _)| b == "LMS" && a == "LMS"));

    // A field that was a secret and is an integer now: its stored secret is ignored too.
    let stored = settings::read(app.state().db(), app.state().keyring(), "general", false).await.unwrap();
    let txn = rustcms_core::db::begin_write(app.state().db()).await.unwrap();
    let secrets = std::collections::BTreeMap::from([("class_size".to_owned(), "was-a-secret".to_owned())]);
    let mut values = stored.values.clone();
    values.remove("class_size");
    settings::write(&txn, app.state().keyring(), "general", stored.version + 1, values, &secrets, None).await.unwrap();
    txn.commit().await.unwrap();
    let version = (stored.version + 1).to_string();
    put(&app, &admin, &version, json!({"class_size": 13})).await.assert_status(StatusCode::OK);
    let read: GeneralSettings = settings::get(app.state()).await.unwrap();
    assert_eq!(read.class_size, 13);
});

db_test!(secrets_are_encrypted_and_never_sent, |db: TestDb| async move {
    let (app, users) = app(db).await;
    let admin = app.acting_as(&users.admin).await;
    let (first, second) = ("hunter2-secret-value", "correct-horse-key");
    let saved = put(&app, &admin, "0", json!({"smtp_password": first, "api_key": second})).await;
    saved.assert_status(StatusCode::OK);
    assert_eq!(saved.json()["secrets"], json!({"api_key": "set", "smtp_password": "set"}));
    let read: GeneralSettings = settings::get(app.state()).await.unwrap();
    assert_eq!((read.smtp_password.expose(), read.api_key.expose()), (Some(first), Some(second)));
    // Serializing the struct never carries them.
    assert!(!serde_json::to_string(&read).unwrap().contains(first));
    assert!(!format!("{read:?}").contains(first));

    // An unrelated change keeps (and re-encrypts) them.
    put(&app, &admin, "1", json!({"site_name": "Other"})).await.assert_status(StatusCode::OK);
    let read: GeneralSettings = settings::get(app.state()).await.unwrap();
    assert_eq!(read.smtp_password.expose(), Some(first));
    // Clear one.
    let cleared = put(&app, &admin, "2", json!({"smtp_password": null})).await;
    assert_eq!(cleared.json()["secrets"]["smtp_password"], json!("unset"));

    // Nowhere in plaintext: responses, the row, the audit log.
    let row = setting::Entity::find_by_id("general".to_owned()).one(app.state().db()).await.unwrap().unwrap();
    let dump = format!("{:?} {:?} {}", row.value, row.secrets, admin.get(&url(&app)).await.text());
    let audit = format!("{:?}", entries(&app).await);
    for secret in [first, second] {
        assert!(!dump.contains(secret) && !audit.contains(secret), "{secret} leaked");
    }
    let changes: Vec<Value> = entries(&app).await.into_iter().filter_map(|e| e.changes).collect();
    assert_eq!(changes[0], json!({"smtp_password": [null, "[redacted]"], "api_key": [null, "[redacted]"]}));
    assert_eq!(changes[2], json!({"smtp_password": ["[redacted]", null]}));
});

db_test!(unreadable_secrets_must_be_replaced_or_cleared, |db: TestDb| async move {
    let (app, users) = app(db.clone()).await;
    let admin = app.acting_as(&users.admin).await;
    put(&app, &admin, "0", json!({"smtp_password": "one", "api_key": "two"})).await.assert_status(StatusCode::OK);

    // Another APP_KEY (rotated): the secrets cannot be read any more.
    let rotated = TestApp::new(support::app::build, db).await;
    let admin = rotated.acting_as(&users.admin).await;
    let page = admin.get(&url(&rotated)).await.json();
    assert_eq!(page["secrets"], json!({"api_key": "unreadable", "smtp_password": "unreadable"}));
    let err = settings::get::<GeneralSettings>(rotated.state()).await.unwrap_err().to_string();
    assert!(err.contains("general.") && err.contains("APP_KEY"), "{err}");

    // Not dropped silently: every unreadable secret is replaced or cleared first.
    let partial = put(&rotated, &admin, "1", json!({"site_name": "X", "smtp_password": "new"})).await;
    partial.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(partial.json()["issues"]["api_key"][0]["code"], json!("unreadable_secret"));
    assert!(partial.json()["issues"].get("smtp_password").is_none());
    let fixed = put(&rotated, &admin, "1", json!({"smtp_password": "new", "api_key": null})).await;
    fixed.assert_status(StatusCode::OK);
    let read: GeneralSettings = settings::get(rotated.state()).await.unwrap();
    assert_eq!((read.smtp_password.expose(), read.api_key.is_set()), (Some("new"), false));
});

db_test!(a_ciphertext_moved_to_another_field_does_not_decrypt, |db: TestDb| async move {
    let (app, users) = app(db).await;
    let admin = app.acting_as(&users.admin).await;
    put(&app, &admin, "0", json!({"smtp_password": "one"})).await.assert_status(StatusCode::OK);
    let conn = app.state().db();
    let row = setting::Entity::find_by_id("general".to_owned()).one(conn).await.unwrap().unwrap();
    let sealed = row.secrets.as_ref().unwrap()["smtp_password"].clone();
    let mut moved: setting::ActiveModel = row.into();
    moved.secrets = sea_orm::Set(Some(json!({"smtp_password": sealed, "api_key": sealed})));
    sea_orm::ActiveModelTrait::update(moved, conn).await.unwrap();
    let page = admin.get(&url(&app)).await.json();
    assert_eq!(page["secrets"], json!({"api_key": "unreadable", "smtp_password": "set"}));
});

db_test!(other_instances_see_a_change_at_once, |db: TestDb| async move {
    let (a, users) = app(db.clone()).await;
    // Through the figment: the test app's builder re-reads it.
    let config = rustcms_testing::test_config(db.url()).with("app.key", a.state().config().app.key.encode()).unwrap();
    let b = TestApp::with_config(support::app::build, config, db).await;
    let before: GeneralSettings = settings::get(b.state()).await.unwrap();
    assert_eq!(before.site_name, "Academy");
    let admin = a.acting_as(&users.admin).await;
    put(&a, &admin, "0", json!({"site_name": "Shared", "smtp_password": "s"})).await.assert_status(StatusCode::OK);
    let after: GeneralSettings = settings::get(b.state()).await.unwrap();
    assert_eq!((after.site_name.as_str(), after.smtp_password.expose()), ("Shared", Some("s")));
});

db_test!(simultaneous_first_writes_apply_once, |db: TestDb| async move {
    let (app, users) = app(db).await;
    let one = app.acting_as(&users.admin).await;
    let two = app.acting_as(&users.admin).await;
    one.fetch_csrf().await;
    two.fetch_csrf().await;
    let (path, first, second) = (
        url(&app),
        json!({"version": "0", "data": {"site_name": "One"}}),
        json!({"version": "0", "data": {"site_name": "Two"}}),
    );
    let (a, b) = tokio::join!(one.send(Method::PUT, &path, Some(&first)), two.send(Method::PUT, &path, Some(&second)));
    let statuses = [a.status, b.status];
    assert_eq!(statuses.iter().filter(|s| **s == StatusCode::OK).count(), 1, "{statuses:?}");
    let refused = if a.status == StatusCode::OK { &b } else { &a };
    refused.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(refused.json()["formIssues"][0]["code"], json!("stale"));
    assert_eq!(entries(&app).await.len(), 1);
    assert_eq!(one.get(&url(&app)).await.json()["version"], json!("1"));
});

db_test!(a_failed_entry_rolls_the_change_back, |db: TestDb| async move {
    let (app, users) = app(db).await;
    let admin = app.acting_as(&users.admin).await;
    app.state().db().execute_unprepared("DROP TABLE audit_logs").await.unwrap();
    put(&app, &admin, "0", json!({"site_name": "Lost"})).await.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
    assert!(setting::Entity::find().all(app.state().db()).await.unwrap().is_empty());
});

db_test!(revoked_access_is_seen_inside_the_write, |db: TestDb| async move {
    let (app, users) = app(db).await;
    grant(&app, &users.member, "settings-editor", &["settings.general.view", "settings.general.update"]).await;
    let auth = app.state().service::<AuthState>().unwrap();
    let member = rustcms_auth::entities::role::Entity::find().all(app.state().db()).await.unwrap();
    let role = member.iter().find(|r| r.name == "settings-editor").unwrap();
    // The principal of a request that started before the role was revoked.
    let fresh = rustcms_core::models::user::Entity::find_by_id(users.member.id).one(app.state().db()).await.unwrap();
    let principal = auth.resolver.principal(app.state().db(), ADMIN_GUARD, &fresh.unwrap(), None).await.unwrap();
    assert!(principal.can("settings.general.update"));
    RbacService::new(app.state().db(), &auth.resolver)
        .revoke_role(Actor::System, users.member.id, role.id)
        .await
        .unwrap();
    let (txn, current) = rustcms_auth::rbac::begin_as(app.state().db(), &principal).await.unwrap();
    assert!(!current.can("settings.general.update"), "grants are reloaded inside the write");
    txn.rollback().await.unwrap();
});

// Declarations that do not match their struct are refused at boot.

#[derive(Default, Serialize, Deserialize)]
struct Leaky {
    password: String,
}

impl Settings for Leaky {
    const GROUP: &'static str = "leaky";
}

#[async_trait::async_trait]
impl SettingsPage for Leaky {
    fn meta() -> SettingsMeta {
        SettingsMeta::new("Leaky")
    }

    fn fields() -> Vec<SettingField> {
        vec![SettingField::secret("password")]
    }
}

#[test]
#[should_panic(expected = "must have the type `Secret`")]
fn a_secret_must_be_a_secret() {
    let _ = rustcms_admin::AdminProvider::new().settings::<Leaky>();
}

#[derive(Default, Serialize, Deserialize)]
struct Partial {
    a: String,
    b: String,
}

impl Settings for Partial {
    const GROUP: &'static str = "partial";
}

#[async_trait::async_trait]
impl SettingsPage for Partial {
    fn meta() -> SettingsMeta {
        SettingsMeta::new("Partial")
    }

    fn fields() -> Vec<SettingField> {
        vec![SettingField::text("a")]
    }
}

#[test]
#[should_panic(expected = "struct field `b` has no SettingField")]
fn every_struct_field_is_declared() {
    let _ = rustcms_admin::AdminProvider::new().settings::<Partial>();
}

#[derive(Default, Serialize, Deserialize)]
struct WrongKind {
    size: String,
}

impl Settings for WrongKind {
    const GROUP: &'static str = "wrong";
}

#[async_trait::async_trait]
impl SettingsPage for WrongKind {
    fn meta() -> SettingsMeta {
        SettingsMeta::new("Wrong")
    }

    fn fields() -> Vec<SettingField> {
        vec![SettingField::integer("size")]
    }
}

#[test]
#[should_panic(expected = "does not hold a value of kind Integer")]
fn defaults_fit_their_kind() {
    let _ = rustcms_admin::AdminProvider::new().settings::<WrongKind>();
}

#[test]
#[should_panic(expected = "registered twice")]
fn a_group_is_registered_once() {
    let _ = rustcms_admin::AdminProvider::new().settings::<GeneralSettings>().settings::<GeneralSettings>();
}

#[test]
fn secrets_do_not_debug() {
    assert_eq!(format!("{:?}", Secret::new("x")), "Secret(set)");
}

/// A body that records whether the server read it.
struct Probe(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl http_body::Body for Probe {
    type Data = axum::body::Bytes;
    type Error = std::convert::Infallible;

    fn poll_frame(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
        std::task::Poll::Ready(Some(Ok(http_body::Frame::data(axum::body::Bytes::from_static(b"{}")))))
    }
}

db_test!(the_body_is_not_read_without_access, |db: TestDb| async move {
    let (app, users) = app(db).await;
    for (user, read) in [(&users.member, false), (&users.admin, true)] {
        let client = app.acting_as(user).await;
        client.fetch_csrf().await;
        let polled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let request = Request::builder()
            .method(Method::PUT)
            .uri(url(&app))
            .header("content-type", "application/json")
            .body(Body::new(Probe(polled.clone())))
            .unwrap();
        let response = client.request(request).await;
        assert_eq!(polled.load(std::sync::atomic::Ordering::SeqCst), read, "{}", response.status);
    }
});

db_test!(hook_messages_never_quote_a_secret, |db: TestDb| async move {
    let (app, users) = app(db).await;
    let admin = app.acting_as(&users.admin).await;
    put(&app, &admin, "0", json!({"smtp_password": "very-secret-1"})).await.assert_status(StatusCode::OK);
    let refused = put(&app, &admin, "1", json!({"site_name": "leak"})).await;
    refused.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    let text = refused.text();
    assert!(!text.contains("very-secret-1"), "{text}");
    assert!(text.contains("[redacted]"), "{text}");
    // A replacement sent with the same request is redacted too.
    let refused = put(&app, &admin, "1", json!({"site_name": "leak", "smtp_password": "very-secret-2"})).await;
    assert!(!refused.text().contains("very-secret-2"));
});

db_test!(an_altered_secrets_column_fails_closed, |db: TestDb| async move {
    let (app, users) = app(db).await;
    let admin = app.acting_as(&users.admin).await;
    put(&app, &admin, "0", json!({"smtp_password": "one"})).await.assert_status(StatusCode::OK);
    let conn = app.state().db();
    let row = setting::Entity::find_by_id("general".to_owned()).one(conn).await.unwrap().unwrap();
    let mut altered: setting::ActiveModel = row.into();
    altered.secrets = sea_orm::Set(Some(json!(["not", "an", "object"])));
    sea_orm::ActiveModelTrait::update(altered, conn).await.unwrap();
    admin.get(&url(&app)).await.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
    put(&app, &admin, "1", json!({"site_name": "X"})).await.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
    assert!(settings::get::<GeneralSettings>(app.state()).await.is_err());
});

db_test!(access_revoked_while_waiting_for_the_lock_is_refused, |db: TestDb| async move {
    let (app, users) = app(db).await;
    grant(&app, &users.member, "settings-editor", &["settings.general.view", "settings.general.update"]).await;
    let member = app.acting_as(&users.member).await;
    member.fetch_csrf().await;
    let auth = app.state().service::<AuthState>().unwrap();
    let admin = rustcms_core::models::user::Entity::find_by_id(users.admin.id).one(app.state().db()).await.unwrap();
    let principal = auth.resolver.principal(app.state().db(), ADMIN_GUARD, &admin.unwrap(), None).await.unwrap();
    // Another change holds the RBAC lock and revokes the member's role.
    let (txn, _) = rustcms_auth::rbac::begin_as(app.state().db(), &principal).await.unwrap();
    let path = url(&app);
    let body = json!({"version": "0", "data": {"site_name": "Late"}});
    let save = member.send(Method::PUT, &path, Some(&body));
    let revoke = async {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        rustcms_auth::entities::model_has_roles::Entity::delete_many()
            .filter(rustcms_auth::entities::model_has_roles::Column::ModelId.eq(users.member.id.to_string()))
            .exec(&txn)
            .await
            .unwrap();
        txn.commit().await.unwrap();
    };
    let (response, ()) = tokio::join!(save, revoke);
    response.assert_status(StatusCode::FORBIDDEN);
    assert!(setting::Entity::find().all(app.state().db()).await.unwrap().is_empty());
    assert!(entries(&app).await.is_empty());
});

// Declarations whose struct does not hold what they say.

macro_rules! misdeclared {
    ($test:ident, $name:ident, $expected:literal, { $($field:ident : $ty:ty = $default:expr),* }, $fields:expr) => {
        #[derive(Serialize, Deserialize)]
        struct $name { $($field: $ty),* }

        impl Default for $name {
            fn default() -> Self {
                Self { $($field: $default),* }
            }
        }

        impl Settings for $name {
            const GROUP: &'static str = "misdeclared";
        }

        #[async_trait::async_trait]
        impl SettingsPage for $name {
            fn meta() -> SettingsMeta {
                SettingsMeta::new("Misdeclared")
            }

            fn fields() -> Vec<SettingField> {
                $fields
            }
        }

        #[test]
        #[should_panic(expected = $expected)]
        fn $test() {
            let _ = rustcms_admin::AdminProvider::new().settings::<$name>();
        }
    };
}

misdeclared!(a_secret_declared_as_text_is_refused, HiddenSecret, "does not hold",
    { password: Secret = Secret::default() }, vec![SettingField::text("password").nullable()]);
misdeclared!(a_nullable_setting_needs_an_option, NotOption, "must be an `Option`",
    { name: String = String::new() }, vec![SettingField::text("name").nullable()]);
misdeclared!(an_option_must_be_declared_nullable, UndeclaredOption, "declare it `.nullable()`",
    { name: Option<String> = Some("x".into()) }, vec![SettingField::text("name")]);
misdeclared!(integers_are_i32, Wide, "must have the type `i32`",
    { size: i64 = 0 }, vec![SettingField::integer("size")]);
misdeclared!(secret_defaults_are_unset, PresetSecret, "must be unset",
    { key: Secret = Secret::new("preset") }, vec![SettingField::secret("key")]);
misdeclared!(selects_have_options, NoOptions, "has no options",
    { level: String = String::new() }, vec![SettingField::select("level", Vec::<(&str, &str)>::new())]);

#[derive(Default, Serialize, Deserialize)]
struct OtherOptions {
    level: String,
}

impl Settings for OtherOptions {
    const GROUP: &'static str = "other_options";
}

#[async_trait::async_trait]
impl SettingsPage for OtherOptions {
    fn meta() -> SettingsMeta {
        SettingsMeta::new("Other")
    }

    fn fields() -> Vec<SettingField> {
        vec![SettingField::select("level", [("", "None"), ("a", "A")])]
    }

    fn form() -> rustcms_admin::Form {
        rustcms_admin::Form::new()
            .schema(rustcms_admin::components![rustcms_admin::SelectInput::make("level").options([("c", "C")])])
    }
}

#[test]
#[should_panic(expected = "offers other values")]
fn a_form_offers_the_values_its_field_accepts() {
    let _ = rustcms_admin::AdminProvider::new().settings::<OtherOptions>();
}
