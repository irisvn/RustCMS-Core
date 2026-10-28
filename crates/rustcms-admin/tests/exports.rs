//! CSV exports of posts: the list as the user sees it (scope, filters,
//! readable fields), text protected against formulas, background exports
//! checked again at each batch and at download. SQLite, PostgreSQL, MySQL.

mod support;

use axum::http::{Method, StatusCode};
use rustcms_auth::{ADMIN_GUARD, Actor, AuthState, RbacService};
use rustcms_core::models::user;
use rustcms_testing::{Factory, TestApp, TestClient, TestDb, UserFactory, db_test};
use serde_json::{Value, json};

use support::app::setup;

const EXPORTER: [&str; 5] = ["posts.view_any", "posts.view", "posts.create", "posts.delete", "posts.export"];

/// A user holding `permissions` through a role of their own (role id too).
async fn user_with(app: &TestApp, name: &str, permissions: &[&str]) -> (user::Model, i64) {
    let auth = app.state().service::<AuthState>().unwrap();
    let db = app.state().db();
    let rbac = RbacService::new(db, &auth.resolver);
    let user = UserFactory::new().name(name).create(db).await;
    let permissions: Vec<String> = permissions.iter().map(|p| (*p).to_owned()).collect();
    let role = rbac.create_role(Actor::System, ADMIN_GUARD, &name.to_lowercase(), &permissions).await.unwrap();
    rbac.assign_role(Actor::System, user.id, role.id).await.unwrap();
    (user, role.id)
}

async fn set_permissions(app: &TestApp, role: i64, permissions: &[&str]) {
    let auth = app.state().service::<AuthState>().unwrap();
    let rbac = RbacService::new(app.state().db(), &auth.resolver);
    let permissions: Vec<String> = permissions.iter().map(|p| (*p).to_owned()).collect();
    rbac.set_role_permissions(Actor::System, role, &permissions).await.unwrap();
}

async fn post(client: &TestClient<'_>, app: &TestApp, title: &str) -> String {
    let created = client.post_json(&app.admin_api("resources/posts/records"), &json!({"title": title})).await;
    created.assert_status(StatusCode::CREATED);
    created.json()["data"]["key"].as_str().unwrap().to_owned()
}

/// Lines of a CSV body (header first).
fn lines(text: &str) -> Vec<String> {
    text.split("\r\n").filter(|l| !l.is_empty()).map(str::to_owned).collect()
}

async fn work(app: &TestApp) {
    rustcms_core::queue::run_until_idle(app.state(), None, 10).await.unwrap();
}

db_test!(exports_show_the_list_as_the_user_sees_it, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let (exporter, _) = user_with(&app, "Exporter", &EXPORTER).await;
    let client = app.acting_as(&exporter).await;
    post(&client, &app, "Plain").await;
    post(&client, &app, "=HYPERLINK(\"http://evil\")").await;
    let admin = app.acting_as(&users.admin).await;
    post(&admin, &app, "Theirs").await;

    let export = client.get(&app.admin_api("resources/posts/export?sort=title")).await;
    export.assert_status(StatusCode::OK);
    assert_eq!(export.header("content-type").as_deref(), Some("text/csv; charset=utf-8"));
    assert!(export.header("content-disposition").unwrap().starts_with("attachment"));
    let lines = lines(&export.text());
    // Readable fields only (`secret` is not declared), the user's own posts only.
    assert_eq!(lines[0], "title,category_id,author_id,price,published,published_at");
    assert_eq!(lines.len(), 3, "{lines:?}");
    // A formula is written as text (sorted first: `=` comes before letters).
    assert!(lines[1].starts_with("\"'=HYPERLINK(\"\"http://evil\"\")\","), "{lines:?}");
    assert!(lines[2].starts_with("Plain,"), "{lines:?}");
    assert!(!export.text().contains("Theirs"));
    // Filters and search like the list.
    let filtered = client.get(&app.admin_api("resources/posts/export?search=Plain")).await;
    assert_eq!(lines_of(&filtered).len(), 2);
    let bad = client.get(&app.admin_api("resources/posts/export?filter[secret][eq]=x")).await;
    bad.assert_status(StatusCode::UNPROCESSABLE_ENTITY);

    // Without `posts.export` (or anything): 403, whatever the query.
    let editor = app.acting_as(&users.editor).await;
    editor.get(&app.admin_api("resources/posts/export")).await.assert_status(StatusCode::FORBIDDEN);
    let member = app.acting_as(&users.member).await;
    member.get(&app.admin_api("resources/posts/export?filter[x][y]=z")).await.assert_status(StatusCode::FORBIDDEN);
    // Not exportable: 404.
    admin.get(&app.admin_api("resources/notes/export")).await.assert_status(StatusCode::NOT_FOUND);
    // The schema says whether the user may export.
    let schema = client.get(&app.admin_api("resources/posts/schema?operation=view")).await;
    assert_eq!(schema.json()["abilities"]["export"], json!(true));
    let schema = editor.get(&app.admin_api("resources/posts/schema?operation=view")).await;
    assert_eq!(schema.json()["abilities"]["export"], json!(false));
});

fn lines_of(response: &rustcms_testing::TestResponse) -> Vec<String> {
    response.assert_status(StatusCode::OK);
    lines(&response.text())
}

db_test!(background_exports_are_checked_again_at_download, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let (exporter, role) = user_with(&app, "Exporter", &EXPORTER).await;
    let client = app.acting_as(&exporter).await;
    let first = post(&client, &app, "First").await;
    post(&client, &app, "Second").await;

    let created = client.post_json(&app.admin_api("resources/posts/exports?sort=title"), &json!({})).await;
    created.assert_status(StatusCode::CREATED);
    let id = created.json()["id"].as_str().unwrap().to_owned();
    assert_eq!(created.json()["status"], json!("pending"));
    let file = app.admin_api(&format!("exports/{id}/file"));
    client.get(&file).await.assert_status(StatusCode::CONFLICT);
    work(&app).await;
    let listed = client.get(&app.admin_api("exports")).await.json();
    assert_eq!((listed[0]["status"].clone(), listed[0]["rows"].clone()), (json!("done"), json!(2)));
    let downloaded = client.get(&file).await;
    let rows = lines_of(&downloaded);
    assert_eq!(rows.len(), 3);
    assert!(rows[1].starts_with("First,") && rows[2].starts_with("Second,"), "{rows:?}");

    // Someone else's export, or none: alike.
    let admin = app.acting_as(&users.admin).await;
    admin.get(&file).await.assert_status(StatusCode::NOT_FOUND);
    admin
        .get(&app.admin_api(&format!("exports/{}/file", uuid::Uuid::now_v7())))
        .await
        .assert_status(StatusCode::NOT_FOUND);
    assert_eq!(admin.get(&app.admin_api("exports")).await.json(), json!([]));

    // A record of the file no longer visible: stale.
    client.send(Method::DELETE, &app.admin_api(&format!("resources/posts/records/{first}")), None).await;
    let stale = client.get(&file).await;
    stale.assert_status(StatusCode::CONFLICT);
    assert_eq!(stale.json()["code"], json!("export_stale"));

    // Export permission withdrawn: 403.
    let again = client.post_json(&app.admin_api("resources/posts/exports"), &json!({})).await;
    let again = again.json()["id"].as_str().unwrap().to_owned();
    work(&app).await;
    set_permissions(&app, role, &["posts.view_any", "posts.view"]).await;
    client.get(&app.admin_api(&format!("exports/{again}/file"))).await.assert_status(StatusCode::FORBIDDEN);
});

db_test!(background_exports_reload_their_user, |db: TestDb| async move {
    let (app, _users) = setup(db).await;
    let (exporter, role) = user_with(&app, "Exporter", &EXPORTER).await;
    let client = app.acting_as(&exporter).await;
    post(&client, &app, "Only").await;
    // Permission withdrawn before the job runs: it fails, saying why.
    client.post_json(&app.admin_api("resources/posts/exports"), &json!({})).await.assert_status(StatusCode::CREATED);
    set_permissions(&app, role, &["posts.view_any", "posts.view", "posts.create"]).await;
    work(&app).await;
    let listed: Value = client.get(&app.admin_api("exports")).await.json();
    assert_eq!(listed[0]["status"], json!("failed"));
    assert_eq!(listed[0]["error"], json!("revoked"));
    // At most three waiting at once; a retried request (same key) is the same export.
    set_permissions(&app, role, &EXPORTER).await;
    let client = app.acting_as(&exporter).await;
    let keyed = |key: &str| {
        axum::http::Request::builder()
            .method(Method::POST)
            .uri(app.admin_api("resources/posts/exports?sort=title"))
            .header("Idempotency-Key", key)
            .header("Content-Type", "application/json")
            .body(axum::body::Body::from("{}"))
            .unwrap()
    };
    let first = client.request(keyed("export-1")).await;
    first.assert_status(StatusCode::CREATED);
    let again = client.request(keyed("export-1")).await;
    assert_eq!(again.json()["id"], first.json()["id"]);
    // The query is the URL's: a body is refused rather than ignored.
    client
        .post_json(&app.admin_api("resources/posts/exports"), &json!({"query": {"search": "x"}}))
        .await
        .assert_validation_error("body");
    for _ in 0..2 {
        client
            .post_json(&app.admin_api("resources/posts/exports"), &json!({}))
            .await
            .assert_status(StatusCode::CREATED);
    }
    client
        .post_json(&app.admin_api("resources/posts/exports"), &json!({}))
        .await
        .assert_status(StatusCode::TOO_MANY_REQUESTS);
    // A disabled account: the job refuses.
    let auth = app.state().service::<AuthState>().unwrap();
    RbacService::new(app.state().db(), &auth.resolver)
        .set_user_active(Actor::System, exporter.id, false)
        .await
        .unwrap();
    work(&app).await;
    let rows = rustcms_admin::exports::entity::Entity::find_all(app.state().db()).await;
    assert!(rows.iter().filter(|r| r.status != "failed").count() == 0, "{rows:?}");
});

trait FindAll {
    async fn find_all(db: &sea_orm::DatabaseConnection) -> Vec<rustcms_admin::exports::entity::Model>;
}

impl FindAll for rustcms_admin::exports::entity::Entity {
    async fn find_all(db: &sea_orm::DatabaseConnection) -> Vec<rustcms_admin::exports::entity::Model> {
        use sea_orm::EntityTrait;
        rustcms_admin::exports::entity::Entity::find().all(db).await.unwrap()
    }
}

db_test!(pruning_deletes_every_object_of_an_export, |db: TestDb| async move {
    let (app, _) = setup(db).await;
    let (exporter, _) = user_with(&app, "Exporter", &EXPORTER).await;
    let client = app.acting_as(&exporter).await;
    post(&client, &app, "First").await;
    let created = client.post_json(&app.admin_api("resources/posts/exports"), &json!({})).await;
    let id: uuid::Uuid = created.json()["id"].as_str().unwrap().parse().unwrap();
    work(&app).await;
    use rustcms_admin::exports::{entity, object};
    use rustcms_core::storage::{Storage, StorageKey};
    use sea_orm::EntityTrait;
    let db = app.state().db();
    let row = entity::Entity::find_by_id(id).one(db).await.unwrap().unwrap();
    let published = StorageKey::parse(row.storage_key.as_deref().unwrap()).unwrap();
    let recorded = object::Entity::find().all(db).await.unwrap();
    assert_eq!(recorded.len(), 1, "the run recorded its object before uploading it");
    assert_eq!(recorded[0].storage_key, published.as_str());
    let storage = app.state().service::<std::sync::Arc<dyn Storage>>().unwrap();
    assert!(storage.open(&published).await.is_ok());
    // Past its expiry and the pruning grace.
    let past = chrono::Utc::now() - chrono::Duration::hours(2);
    entity::Entity::update_many()
        .col_expr(entity::Column::ExpiresAt, sea_orm::sea_query::Expr::value(past))
        .exec(db)
        .await
        .unwrap();
    assert_eq!(rustcms_admin::exports::prune(db, storage.as_ref().as_ref()).await.unwrap(), 1);
    assert!(storage.open(&published).await.is_err());
    assert!(object::Entity::find().all(db).await.unwrap().is_empty());
});

/// Tags as an exportable resource whose visibility is its permission alone
/// (no per-record policy): only the export limit bounds it.
struct LabelResource;

impl rustcms_admin::Resource for LabelResource {
    type Entity = support::tag::Entity;
    const SLUG: &'static str = "labels";

    fn meta() -> rustcms_admin::ResourceMeta {
        rustcms_admin::ResourceMeta::new("Label").title("label").key(rustcms_admin::KeyStrategy::Natural).exportable()
    }

    fn fields() -> Vec<rustcms_admin::Field> {
        use support::tag::Column;
        vec![
            rustcms_admin::Field::column(Column::Slug).required(),
            rustcms_admin::Field::column(Column::Label).required(),
        ]
    }
}

fn build_with_labels(config: rustcms_core::Config) -> rustcms_core::AppBuilder {
    support::app::build_extended(config, |admin| admin.resource::<LabelResource>())
}

/// `count` more posts of `author`, inserted in batches.
async fn insert_posts(app: &TestApp, author: i64, from: usize, count: usize) {
    use sea_orm::{ActiveValue::Set, EntityTrait};
    use support::post;
    for start in (from..from + count).step_by(500) {
        let rows = (start..(start + 500).min(from + count)).map(|n| post::ActiveModel {
            id: Set(uuid::Uuid::now_v7()),
            title: Set(format!("Post {n:05}")),
            category_id: Set(None),
            author_id: Set(author),
            price: Set(None),
            published: Set(false),
            published_at: Set(None),
            secret: Set(String::new()),
        });
        post::Entity::insert_many(rows).exec(app.state().db()).await.unwrap();
    }
}

/// `count` more tags, inserted in batches.
async fn insert_tags(app: &TestApp, from: usize, count: usize) {
    use sea_orm::{ActiveValue::Set, EntityTrait};
    use support::tag;
    for start in (from..from + count).step_by(500) {
        let rows = (start..(start + 500).min(from + count))
            .map(|n| tag::ActiveModel { slug: Set(format!("t{n:05}")), label: Set(format!("Tag {n}")) });
        tag::Entity::insert_many(rows).exec(app.state().db()).await.unwrap();
    }
}

/// Asks for a background export of `path`, runs it, and returns it as listed.
async fn export_later(client: &TestClient<'_>, app: &TestApp, path: &str) -> Value {
    let created = client.post_json(&app.admin_api(path), &json!({})).await;
    created.assert_status(StatusCode::CREATED);
    let id = created.json()["id"].as_str().unwrap().to_owned();
    work(app).await;
    let listed: Value = client.get(&app.admin_api("exports")).await.json();
    listed.as_array().unwrap().iter().find(|e| e["id"] == json!(id)).unwrap().clone()
}

/// A failed export: its reason listed, no file to download nor stored.
async fn assert_refused(client: &TestClient<'_>, app: &TestApp, export: &Value, reason: &str) {
    use rustcms_admin::exports::{entity, object};
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    assert_eq!((export["status"].clone(), export["error"].clone()), (json!("failed"), json!(reason)), "{export}");
    let id: uuid::Uuid = export["id"].as_str().unwrap().parse().unwrap();
    let file = client.get(&app.admin_api(&format!("exports/{id}/file"))).await;
    file.assert_status(StatusCode::CONFLICT);
    assert_eq!(file.json()["code"], json!("export_not_ready"));
    let row = entity::Entity::find_by_id(id).one(app.state().db()).await.unwrap().unwrap();
    assert_eq!((row.storage_key, row.keys), (None, None));
    let objects = object::Entity::find().filter(object::Column::ExportId.eq(id)).all(app.state().db()).await.unwrap();
    assert!(objects.is_empty(), "{objects:?}");
}

db_test!(a_background_export_over_its_row_limit_says_so, |db: TestDb| async move {
    let config = rustcms_testing::test_config(db.url());
    let (app, _) = support::app::setup_built(db, config, build_with_labels).await;
    let (exporter, _) = user_with(&app, "Exporter", &["labels.view_any", "labels.view", "labels.export"]).await;
    let client = app.acting_as(&exporter).await;
    let max = rustcms_admin::exports::MAX_ROWS;
    // Exactly at the limit: the export is made.
    insert_tags(&app, 0, max).await;
    let done = export_later(&client, &app, "resources/labels/exports").await;
    assert_eq!((done["status"].clone(), done["rows"].clone()), (json!("done"), json!(max)), "{done}");
    // One more: refused as too many to export, not as revoked.
    insert_tags(&app, max, 1).await;
    let over = export_later(&client, &app, "resources/labels/exports").await;
    assert_refused(&client, &app, &over, "too_many_to_export").await;
});

db_test!(a_background_export_over_the_scan_limit_says_so, |db: TestDb| async move {
    // Posts have a per-record policy: every candidate is checked, at most 10,000.
    let (app, _) = setup(db).await;
    let (exporter, role) = user_with(&app, "Exporter", &EXPORTER).await;
    let client = app.acting_as(&exporter).await;
    let max = rustcms_admin::exports::MAX_ROWS;
    insert_posts(&app, exporter.id, 0, max).await;
    let done = export_later(&client, &app, "resources/posts/exports").await;
    assert_eq!((done["status"].clone(), done["rows"].clone()), (json!("done"), json!(max)), "{done}");
    insert_posts(&app, exporter.id, max, 1).await;
    let over = export_later(&client, &app, "resources/posts/exports").await;
    assert_refused(&client, &app, &over, "too_many_matches").await;
    // Other refusals stay `revoked`, also over the limit (the permission withdrawn first).
    let created = client.post_json(&app.admin_api("resources/posts/exports"), &json!({})).await;
    let id = created.json()["id"].as_str().unwrap().to_owned();
    set_permissions(&app, role, &["posts.view_any", "posts.view"]).await;
    work(&app).await;
    set_permissions(&app, role, &EXPORTER).await;
    let client = app.acting_as(&exporter).await;
    let listed: Value = client.get(&app.admin_api("exports")).await.json();
    let revoked = listed.as_array().unwrap().iter().find(|e| e["id"] == json!(id)).unwrap().clone();
    assert_refused(&client, &app, &revoked, "revoked").await;
});

db_test!(pruning_touches_only_its_own_disk, |db: TestDb| async move {
    use rustcms_admin::exports::{entity, object, prune};
    use rustcms_core::storage::{Storage, StorageKey};
    use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter};
    let (app, _) = setup(db).await;
    let (exporter, _) = user_with(&app, "Exporter", &EXPORTER).await;
    let client = app.acting_as(&exporter).await;
    post(&client, &app, "First").await;
    // On this app's disk (`local`).
    let local_export = export_later(&client, &app, "resources/posts/exports").await;
    let local_id: uuid::Uuid = local_export["id"].as_str().unwrap().parse().unwrap();
    let db = app.state().db();
    let local = app.state().service::<std::sync::Arc<dyn Storage>>().unwrap();
    let local: &dyn Storage = local.as_ref().as_ref();
    let other = support::OtherDisk::new();
    let template = entity::Entity::find_by_id(local_id).one(db).await.unwrap().unwrap();
    let past = chrono::Utc::now() - chrono::Duration::hours(2);
    let record = |id, disk: &str, key: &StorageKey| object::ActiveModel {
        storage_key: Set(key.as_str().to_owned()),
        export_id: Set(id),
        disk: Set(disk.to_owned()),
        created_at: Set(past),
    };
    // An export of the other disk.
    let theirs = uuid::Uuid::now_v7();
    let their_key = other.put(b"title\r\n").await;
    let mut row: entity::ActiveModel = template.clone().into();
    row.id = Set(theirs);
    row.disk = Set(Some("s3".into()));
    row.storage_key = Set(Some(their_key.as_str().to_owned()));
    row.insert(db).await.unwrap();
    record(theirs, "s3", &their_key).insert(db).await.unwrap();
    // An export of this disk with a run's object on the other (moved meanwhile).
    let moved = uuid::Uuid::now_v7();
    let moved_local = StorageKey::generate("csv").unwrap();
    let moved_other = other.put(b"title\r\n").await;
    let mut row: entity::ActiveModel = template.clone().into();
    row.id = Set(moved);
    row.storage_key = Set(Some(moved_local.as_str().to_owned()));
    row.insert(db).await.unwrap();
    record(moved, "local", &moved_local).insert(db).await.unwrap();
    record(moved, "s3", &moved_other).insert(db).await.unwrap();
    entity::Entity::update_many()
        .col_expr(entity::Column::ExpiresAt, sea_orm::sea_query::Expr::value(past))
        .exec(db)
        .await
        .unwrap();
    let objects = |id: uuid::Uuid| async move {
        let found = object::Entity::find().filter(object::Column::ExportId.eq(id)).all(db).await.unwrap();
        found.into_iter().map(|o| o.disk).collect::<Vec<_>>()
    };
    let exists = |id| async move { entity::Entity::find_by_id(id).one(db).await.unwrap().is_some() };

    // This disk: its export and its objects only.
    assert_eq!(prune(db, local).await.unwrap(), 1);
    assert!(!exists(local_id).await && objects(local_id).await.is_empty());
    assert!(exists(theirs).await, "the other disk's export is kept");
    assert_eq!(objects(theirs).await, ["s3"]);
    assert!(other.exists(&their_key).await.unwrap());
    assert!(exists(moved).await, "kept while an object of the other disk is left");
    assert_eq!(objects(moved).await, ["s3"]);
    assert!(other.exists(&moved_other).await.unwrap());
    // The other disk: its export, and its object of the moved one.
    assert_eq!(prune(db, &other).await.unwrap(), 1);
    assert!(!exists(theirs).await && objects(theirs).await.is_empty());
    assert!(!other.exists(&their_key).await.unwrap() && !other.exists(&moved_other).await.unwrap());
    assert!(exists(moved).await && objects(moved).await.is_empty());
    // This disk again: the moved export goes too.
    assert_eq!(prune(db, local).await.unwrap(), 1);
    assert!(!exists(moved).await);
});
