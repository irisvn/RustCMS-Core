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
