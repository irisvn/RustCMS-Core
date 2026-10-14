//! Reading the audit log: the whole log for auditors, filtered in SQL (no
//! empty pages, no hidden counts); a record's history for its viewers, with
//! only the fields they may read. All databases.

mod support;

use axum::http::{Method, StatusCode};
use rustcms_auth::{ADMIN_GUARD, Actor, AuthState, RbacService};
use rustcms_core::audit::{AuditContext, AuditEntry};
use rustcms_core::models::{audit_log, user};
use rustcms_testing::{TestApp, TestClient, TestDb, db_test};
use sea_orm::EntityTrait;
use serde_json::{Value, json};
use support::app::{Users, setup};

async fn grant(app: &TestApp, user: &user::Model, name: &str, permissions: &[&str]) {
    let auth = app.state().service::<AuthState>().unwrap();
    let rbac = RbacService::new(app.state().db(), &auth.resolver);
    let permissions: Vec<String> = permissions.iter().map(|p| (*p).to_owned()).collect();
    let role = rbac.create_role(Actor::System, ADMIN_GUARD, name, &permissions).await.unwrap();
    rbac.assign_role(Actor::System, user.id, role.id).await.unwrap();
}

/// Writes an entry as a hook would (any event, any subject).
async fn entry(app: &TestApp, event: &str, subject: &str, key: &str, changes: Value, meta: Value) {
    let txn = rustcms_core::db::begin_write(app.state().db()).await.unwrap();
    let mut e = AuditEntry::new(event, subject).key(key).changes(changes);
    if !meta.is_null() {
        e = e.meta(meta);
    }
    rustcms_core::audit::record(&txn, &AuditContext::system(Some("test")), e).await.unwrap();
    txn.commit().await.unwrap();
}

async fn clear(app: &TestApp) {
    audit_log::Entity::delete_many().exec(app.state().db()).await.unwrap();
}

async fn create_post(app: &TestApp, client: &TestClient<'_>, title: &str) -> String {
    client.fetch_csrf().await;
    let created = client.post_json(&app.admin_api("resources/posts/records"), &json!({"title": title})).await;
    created.assert_status(StatusCode::CREATED);
    created.json()["data"]["key"].as_str().unwrap().to_owned()
}

fn events(page: &Value) -> Vec<String> {
    page["data"].as_array().unwrap().iter().map(|e| e["event"].as_str().unwrap().to_owned()).collect()
}

async fn fixture(db: TestDb) -> (TestApp, Users) {
    let (app, users) = setup(db).await;
    clear(&app).await;
    (app, users)
}

db_test!(the_log_is_for_auditors_and_checked_before_the_query, |db: TestDb| async move {
    let (app, users) = fixture(db).await;
    let url = app.admin_api("audit");
    let member = app.acting_as(&users.member).await;
    member.get(&url).await.assert_status(StatusCode::FORBIDDEN);
    member.get(&format!("{url}?per_page=oops&cursor=!!")).await.assert_status(StatusCode::FORBIDDEN);
    let editor = app.acting_as(&users.editor).await;
    editor.get(&url).await.assert_status(StatusCode::FORBIDDEN);
    assert_eq!(member.get(&app.admin_api("panel")).await.json()["system"]["audit"], json!(false));

    let admin = app.acting_as(&users.admin).await;
    assert_eq!(admin.get(&app.admin_api("panel")).await.json()["system"]["audit"], json!(true));
    for bad in [
        "per_page=0",
        "per_page=101",
        "cursor=zz",
        "actor=x",
        "from=yesterday",
        "subject_key=1",
        "event=a&event=b",
        "other=1",
    ] {
        admin.get(&format!("{url}?{bad}")).await.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    }
});

db_test!(auditors_page_through_everything_newest_first, |db: TestDb| async move {
    let (app, users) = fixture(db).await;
    let editor = app.acting_as(&users.editor).await;
    let mut keys = vec![];
    for title in ["a", "b", "c", "d", "e"] {
        keys.push(create_post(&app, &editor, title).await);
    }
    let admin = app.acting_as(&users.admin).await;
    let url = app.admin_api("audit");
    let mut seen = vec![];
    let mut next = Some(format!("{url}?subject_type=posts&per_page=2"));
    while let Some(page_url) = next.take() {
        let page = admin.get(&page_url).await;
        page.assert_status(StatusCode::OK);
        let body = page.json();
        // A new entry meanwhile is newer: it never shows up in later pages.
        entry(&app, "created", "posts", "late", json!({}), Value::Null).await;
        seen.extend(body["data"].as_array().unwrap().iter().map(|e| e["subjectKey"].as_str().unwrap().to_owned()));
        next = body["meta"]["nextCursor"].as_str().map(|c| format!("{url}?subject_type=posts&per_page=2&cursor={c}"));
    }
    keys.reverse();
    assert_eq!(seen, keys);
    let first = admin.get(&format!("{url}?subject_type=posts&subject_key={}", keys[0])).await.json();
    let e = &first["data"][0];
    assert_eq!((e["event"].clone(), e["actor"]["id"].clone()), (json!("created"), json!(users.editor.id.to_string())));
    assert_eq!(e["actor"]["name"], json!("Editor"), "an admin may list users");
    assert_eq!(e["actor"]["credential"], json!("session"));
    assert!(e["requestId"].is_string());
    let by_actor = admin.get(&format!("{url}?actor={}", users.editor.id)).await.json();
    assert_eq!(by_actor["data"].as_array().unwrap().len(), 5);
    let future = (chrono::Utc::now() + chrono::Duration::days(1)).to_rfc3339();
    let none = admin.get(&format!("{url}?from={}", urlencode(&future))).await.json();
    assert_eq!(none["data"], json!([]));
});

fn urlencode(text: &str) -> String {
    text.replace('+', "%2B").replace(':', "%3A")
}

db_test!(sign_in_entries_need_their_own_permission, |db: TestDb| async move {
    let (app, users) = fixture(db).await;
    grant(&app, &users.member, "auditor", &["audit.view_all"]).await;
    clear(&app).await;
    // Sign-in events (with addresses) and a data change, interleaved.
    for i in 0..3 {
        let txn = rustcms_core::db::begin_write(app.state().db()).await.unwrap();
        let request = rustcms_core::audit::RequestMeta { ip: Some("10.1.1.1".into()), ..Default::default() };
        let context = AuditContext::new(rustcms_core::audit::AuditActor::guest(), request);
        let signed_in = AuditEntry::new("auth.signed_in", "user").key(users.member.id);
        rustcms_core::audit::record(&txn, &context, signed_in).await.unwrap();
        txn.commit().await.unwrap();
        entry(&app, "created", "posts", &format!("p{i}"), json!({}), Value::Null).await;
    }
    let auditor = app.acting_as(&users.member).await;
    let url = app.admin_api("audit");
    // Every page is full until the last: nothing was filtered after the query.
    let page = auditor.get(&format!("{url}?per_page=2")).await.json();
    assert_eq!(events(&page), ["created", "created"]);
    let cursor = page["meta"]["nextCursor"].as_str().unwrap();
    let last = auditor.get(&format!("{url}?per_page=2&cursor={cursor}")).await.json();
    assert_eq!(events(&last), ["created"]);
    assert!(last["meta"]["nextCursor"].is_null());
    assert!(auditor.get(&format!("{url}?event=auth.signed_in")).await.json()["data"].as_array().unwrap().is_empty());
    // Names need `users.view_any`.
    assert!(page["data"][0]["actor"].get("name").is_none());

    grant(&app, &users.member, "auth-auditor", &["audit.view_auth"]).await;
    let auditor = app.acting_as(&users.member).await;
    let all = auditor.get(&format!("{url}?event=auth.signed_in")).await.json();
    assert_eq!(all["data"].as_array().unwrap().len(), 3);
    assert_eq!(all["data"][0]["ip"], json!("10.1.1.1"));
});

db_test!(a_record_history_shows_what_its_viewer_may_read, |db: TestDb| async move {
    let (app, users) = fixture(db).await;
    let editor = app.acting_as(&users.editor).await;
    let mine = create_post(&app, &editor, "Mine").await;
    editor.fetch_csrf().await;
    let record = app.admin_api(&format!("resources/posts/records/{mine}"));
    editor.send(Method::PATCH, &record, Some(&json!({"title": "Mine 2"}))).await.assert_status(StatusCode::OK);
    // An undeclared field in the entry, a relation parent, and an event an application wrote.
    entry(
        &app,
        "updated",
        "posts",
        &mine,
        json!({"title": ["Mine 2", "Mine 3"], "secret": ["a", "b"]}),
        json!({"relation": "x", "parent": "9"}),
    )
    .await;
    entry(&app, "posts.exported", "posts", &mine, json!({}), Value::Null).await;
    let history = format!("{record}/history");

    // `audit.view` is needed, and `history` says so in the schema.
    editor.get(&history).await.assert_status(StatusCode::FORBIDDEN);
    let schema = editor.get(&app.admin_api("resources/posts/schema?operation=view")).await.json();
    assert_eq!(schema["abilities"]["history"], json!(false));
    grant(&app, &users.editor, "historian", &["audit.view"]).await;
    let editor = app.acting_as(&users.editor).await;
    let schema = editor.get(&app.admin_api("resources/posts/schema?operation=view")).await.json();
    assert_eq!(schema["abilities"]["history"], json!(true));

    let page = editor.get(&history).await;
    page.assert_status(StatusCode::OK);
    let body = page.json();
    assert_eq!(events(&body), ["updated", "updated", "created"], "the application's own event is not shown");
    assert_eq!(body["data"][0]["changes"], json!({"title": ["Mine 2", "Mine 3"]}));
    assert!(body["data"][0].get("meta").is_none());
    assert!(body["data"][1].get("requestId").is_none() && body["data"][1].get("ip").is_none());

    // Another author's post is outside the editor's scope: as on the record page.
    let admin = app.acting_as(&users.admin).await;
    let theirs = create_post(&app, &admin, "Theirs").await;
    editor
        .get(&app.admin_api(&format!("resources/posts/records/{theirs}/history")))
        .await
        .assert_status(StatusCode::NOT_FOUND);
    // Malformed paging, once admitted.
    editor.get(&format!("{history}?cursor=zz")).await.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    editor.get(&format!("{history}?event=created")).await.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
});

db_test!(trashed_records_have_no_history_page, |db: TestDb| async move {
    let (app, users) = fixture(db).await;
    let admin = app.acting_as(&users.admin).await;
    admin.fetch_csrf().await;
    let note = admin.post_json(&app.admin_api("resources/notes/records"), &json!({"title": "n"})).await.json();
    let key = note["data"]["key"].as_str().unwrap();
    let record = app.admin_api(&format!("resources/notes/records/{key}"));
    admin.get(&format!("{record}/history")).await.assert_status(StatusCode::OK);
    admin.send(Method::DELETE, &record, None).await.assert_status(StatusCode::NO_CONTENT);
    admin.get(&format!("{record}/history")).await.assert_status(StatusCode::NOT_FOUND);
});

struct Shadow;

impl rustcms_admin::Resource for Shadow {
    type Entity = support::tag::Entity;
    const SLUG: &'static str = "user";

    fn meta() -> rustcms_admin::ResourceMeta {
        rustcms_admin::ResourceMeta::new("Shadow").key(rustcms_admin::KeyStrategy::Natural)
    }

    fn fields() -> Vec<rustcms_admin::Field> {
        vec![rustcms_admin::Field::column(support::tag::Column::Slug)]
    }
}

#[test]
#[should_panic(expected = "this slug is reserved")]
fn framework_subjects_are_reserved_slugs() {
    let _ = rustcms_admin::Registry::new().register::<Shadow>().finish();
}

db_test!(a_key_shaped_like_a_shortened_one_has_its_own_history, |db: TestDb| async move {
    let (app, users) = fixture(db).await;
    // A long key is stored shortened; a record whose real key is that text
    // must not see the long-key record's entries.
    let long = "k".repeat(300);
    entry(&app, "updated", "tags", &long, json!({"label": ["a", "b"]}), Value::Null).await;
    let shaped = rustcms_core::audit::subject_key(&long);
    let admin = app.acting_as(&users.admin).await;
    admin.fetch_csrf().await;
    admin
        .post_json(&app.admin_api("resources/tags/records"), &json!({"slug": shaped, "label": "lookalike"}))
        .await
        .assert_status(StatusCode::CREATED);
    let history = admin.get(&app.admin_api(&format!("resources/tags/records/{shaped}/history"))).await.json();
    assert_eq!(events(&history), ["created"], "{history}");
});

db_test!(entries_sharing_an_instant_page_through, |db: TestDb| async move {
    let (app, users) = fixture(db).await;
    // One batch: every entry has the same `at`.
    let txn = rustcms_core::db::begin_write(app.state().db()).await.unwrap();
    let batch: Vec<AuditEntry> = (0..5).map(|i| AuditEntry::new("created", "batch").key(i)).collect();
    rustcms_core::audit::record_all(&txn, &AuditContext::system(None), batch).await.unwrap();
    txn.commit().await.unwrap();
    let admin = app.acting_as(&users.admin).await;
    let url = app.admin_api("audit");
    let mut keys = vec![];
    let mut next = Some(format!("{url}?subject_type=batch&per_page=2"));
    while let Some(page) = next.take() {
        let body = admin.get(&page).await.json();
        keys.extend(body["data"].as_array().unwrap().iter().map(|e| e["subjectKey"].as_str().unwrap().to_owned()));
        next = body["meta"]["nextCursor"].as_str().map(|c| format!("{url}?subject_type=batch&per_page=2&cursor={c}"));
    }
    keys.sort();
    assert_eq!(keys, ["0", "1", "2", "3", "4"]);
});

db_test!(credential_ids_stay_out_of_meta, |db: TestDb| async move {
    let (app, users) = fixture(db).await;
    entry(&app, "jwt.reuse_detected", "user", "1", json!({}), json!({"family": "secret-family-id"})).await;
    let admin = app.acting_as(&users.admin).await;
    let page = admin.get(&app.admin_api("audit?event=jwt.reuse_detected")).await;
    assert!(!page.text().contains("secret-family-id"), "{}", page.text());
});
