//! CSV imports of notes (import key `title`): admitted before the file is
//! read, checked within limits, previewed without writing, run once through
//! the usual create and update paths, each row once even when the job runs
//! again. SQLite, PostgreSQL, MySQL.

mod support;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use rustcms_auth::{ADMIN_GUARD, Actor, AuthState, RbacService};
use rustcms_core::models::user;
use rustcms_testing::{Factory, TestApp, TestClient, TestDb, TestResponse, UserFactory, db_test};
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};
use serde_json::{Value, json};

use support::app::setup;
use support::note;

const IMPORTER: [&str; 5] = ["notes.view_any", "notes.view", "notes.create", "notes.update", "notes.import"];

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

async fn upload(client: &TestClient<'_>, app: &TestApp, slug: &str, csv: &str) -> TestResponse {
    let request = Request::builder()
        .method(Method::POST)
        .uri(app.admin_api(&format!("resources/{slug}/imports")))
        .header(header::CONTENT_TYPE, "text/csv")
        .body(Body::from(csv.to_owned()))
        .unwrap();
    client.request(request).await
}

async fn work(app: &TestApp) {
    rustcms_core::queue::run_until_idle(app.state(), None, 10).await.unwrap();
}

async fn notes(app: &TestApp) -> Vec<String> {
    let mut titles: Vec<String> =
        note::Entity::find().all(app.state().db()).await.unwrap().into_iter().map(|n| n.title).collect();
    titles.sort();
    titles
}

async fn audited_writes(app: &TestApp) -> u64 {
    use rustcms_core::models::audit_log;
    audit_log::Entity::find()
        .filter(audit_log::Column::Event.is_in(["created", "updated"]))
        .count(app.state().db())
        .await
        .unwrap()
}

db_test!(imports_create_and_update_rows_once, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let (importer, _) = user_with(&app, "Importer", &IMPORTER).await;
    let client = app.acting_as(&importer).await;
    let admin = app.acting_as(&users.admin).await;
    let existing = admin.post_json(&app.admin_api("resources/notes/records"), &json!({"title": "Kept"})).await;
    existing.assert_status(StatusCode::CREATED);

    let csv = "Title,Extra\r\nFirst,x\r\n,y\r\nKept,z\r\n\"Sec, ond\",w\r\n";
    let uploaded = upload(&client, &app, "notes", csv).await;
    uploaded.assert_status(StatusCode::CREATED);
    let body = uploaded.json();
    let id = body["id"].as_str().unwrap().to_owned();
    assert_eq!((body["header"].clone(), body["rows"].clone()), (json!(["Title", "Extra"]), json!(4)));
    // Columns matched with fields by name or label, any case.
    assert_eq!(body["mapping"], json!({"Title": "title"}));
    assert!(body["fields"].as_array().unwrap().iter().any(|f| f["name"] == json!("title") && f["key"] == json!(true)));

    // The preview checks every row, writes nothing.
    let mapping = json!({"mapping": {"Title": "title"}});
    let preview = client.post_json(&app.admin_api(&format!("imports/{id}/preview")), &mapping).await;
    preview.assert_status(StatusCode::OK);
    let preview = preview.json();
    assert_eq!(
        (preview["rows"].clone(), preview["valid"].clone(), preview["keyed"].clone()),
        (json!(4), json!(3), json!(3))
    );
    assert_eq!(preview["report"][0]["row"], json!(3));
    assert_eq!(preview["report"][0]["code"], json!("required"));
    assert_eq!(notes(&app).await, ["Kept"]);
    // A mapping to a field the user may not set, or to an unknown column.
    let bad = client
        .post_json(&app.admin_api(&format!("imports/{id}/preview")), &json!({"mapping": {"Title": "deleted_at"}}))
        .await;
    bad.assert_validation_error("mapping.Title");
    let bad =
        client.post_json(&app.admin_api(&format!("imports/{id}/run")), &json!({"mapping": {"Nope": "title"}})).await;
    bad.assert_validation_error("mapping.Nope");

    // Run once; a second run is refused.
    let run = client.post_json(&app.admin_api(&format!("imports/{id}/run")), &mapping).await;
    run.assert_status(StatusCode::OK);
    assert_eq!(run.json()["status"], json!("running"));
    let again = client.post_json(&app.admin_api(&format!("imports/{id}/run")), &mapping).await;
    again.assert_status(StatusCode::CONFLICT);
    work(&app).await;
    let shown = client.get(&app.admin_api(&format!("imports/{id}"))).await.json();
    assert_eq!(
        (shown["status"].clone(), shown["created"].clone(), shown["updated"].clone(), shown["failed"].clone()),
        (json!("done"), json!(2), json!(1), json!(1)),
        "{shown}"
    );
    assert_eq!(shown["report"], json!([{"row": 3, "field": "title", "message": "is required", "code": "required"}]));
    assert_eq!(notes(&app).await, ["First", "Kept", "Sec, ond"]);

    // Audit: one summary.
    let audited = rustcms_core::models::audit_log::Entity::find()
        .filter(rustcms_core::models::audit_log::Column::Event.eq("imported"))
        .count(app.state().db())
        .await
        .unwrap();
    assert_eq!(audited, 1);

    // No idempotency key of the user's spent (their quota is for their requests).
    use sea_orm::ConnectionTrait;
    let db = app.state().db();
    let keys = db
        .query_one_raw(sea_orm::Statement::from_string(
            db.get_database_backend(),
            "SELECT COUNT(*) AS n FROM idempotency_keys",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<i64>("", "n")
        .unwrap();
    assert_eq!(keys, 0);

    // The job running again (a crash before it finished): rows written are
    // skipped by their receipts (a row created is not updated now), counted as before.
    let writes = audited_writes(&app).await;
    rustcms_admin::imports::entity::Entity::update_many()
        .col_expr(rustcms_admin::imports::entity::Column::Status, sea_orm::sea_query::Expr::value("running"))
        .col_expr(rustcms_admin::imports::entity::Column::Created, sea_orm::sea_query::Expr::value(0))
        .col_expr(rustcms_admin::imports::entity::Column::Updated, sea_orm::sea_query::Expr::value(0))
        .exec(app.state().db())
        .await
        .unwrap();
    let job = rustcms_admin::imports::ImportJob { id: id.parse().unwrap() };
    rustcms_core::queue::enqueue(app.state().db(), &job).await.unwrap();
    work(&app).await;
    assert_eq!(notes(&app).await, ["First", "Kept", "Sec, ond"], "no row written twice");
    assert_eq!(audited_writes(&app).await, writes, "no create or update ran again");
    let shown = client.get(&app.admin_api(&format!("imports/{id}"))).await.json();
    assert_eq!((shown["created"].clone(), shown["updated"].clone()), (json!(2), json!(1)), "{shown}");

    // Someone else's import: as if there were none.
    admin.get(&app.admin_api(&format!("imports/{id}"))).await.assert_status(StatusCode::NOT_FOUND);
});

db_test!(imports_are_admitted_before_the_file_and_bounded, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let member = app.acting_as(&users.member).await;
    let editor = app.acting_as(&users.editor).await; // may create notes, not import
    for client in [&member, &editor] {
        upload(client, &app, "notes", "garbage\u{0}\u{0}").await.assert_status(StatusCode::FORBIDDEN);
    }
    let admin = app.acting_as(&users.admin).await;
    upload(&admin, &app, "posts", "title\r\nx\r\n").await.assert_status(StatusCode::NOT_FOUND);
    let (importer, role) = user_with(&app, "Importer", &IMPORTER).await;
    let client = app.acting_as(&importer).await;
    for bad in ["", "\u{feff}", "a,a\r\n1,2\r\n", "a,b\r\n1\r\n"] {
        upload(&client, &app, "notes", bad).await.assert_validation_error("file");
    }
    let wide: Vec<String> = (0..101).map(|i| format!("c{i}")).collect();
    upload(&client, &app, "notes", &format!("{}\r\n", wide.join(","))).await.assert_validation_error("file");
    let request = Request::builder()
        .method(Method::POST)
        .uri(app.admin_api("resources/notes/imports"))
        .body(Body::from(vec![b't', b'i', 0xff, b'\r', b'\n']))
        .unwrap();
    client.request(request).await.assert_validation_error("file");

    // Permission withdrawn before the job runs: it stops, saying why.
    let uploaded = upload(&client, &app, "notes", "title\r\nLater\r\n").await;
    let id = uploaded.json()["id"].as_str().unwrap().to_owned();
    client
        .post_json(&app.admin_api(&format!("imports/{id}/run")), &json!({"mapping": {"title": "title"}}))
        .await
        .assert_status(StatusCode::OK);
    let auth = app.state().service::<AuthState>().unwrap();
    let permissions: Vec<String> =
        ["notes.view_any", "notes.view", "notes.create"].iter().map(|p| (*p).to_owned()).collect();
    RbacService::new(app.state().db(), &auth.resolver)
        .set_role_permissions(Actor::System, role, &permissions)
        .await
        .unwrap();
    work(&app).await;
    let row = rustcms_admin::imports::entity::Entity::find_by_id(id.parse::<uuid::Uuid>().unwrap())
        .one(app.state().db())
        .await
        .unwrap()
        .unwrap();
    assert_eq!((row.status.as_str(), row.error.as_deref()), ("failed", Some("revoked")));
    assert!(notes(&app).await.is_empty());
    let _: Value = json!(null);
});

db_test!(imports_replay_uploads_and_never_write_once_expired, |db: TestDb| async move {
    let (app, _) = setup(db).await;
    let (importer, _) = user_with(&app, "Importer", &IMPORTER).await;
    let client = app.acting_as(&importer).await;
    let keyed = |csv: &str, key: &str| {
        Request::builder()
            .method(Method::POST)
            .uri(app.admin_api("resources/notes/imports"))
            .header(header::CONTENT_TYPE, "text/csv")
            .header("idempotency-key", key)
            .body(Body::from(csv.to_owned()))
            .unwrap()
    };
    // A retried upload (its answer lost): the same import.
    let first = client.request(keyed("title\r\nLate\r\n", "upload-1")).await;
    first.assert_status(StatusCode::CREATED);
    let again = client.request(keyed("title\r\nLate\r\n", "upload-1")).await;
    again.assert_status(StatusCode::CREATED);
    let id = first.json()["id"].as_str().unwrap().to_owned();
    assert_eq!(again.json()["id"], json!(id));
    client.request(keyed("title\r\nOther\r\n", "upload-1")).await.assert_validation_error("idempotency_key");

    // Run, then the import expires before its job runs: nothing is written.
    client
        .post_json(&app.admin_api(&format!("imports/{id}/run")), &json!({"mapping": {"title": "title"}}))
        .await
        .assert_status(StatusCode::OK);
    use rustcms_admin::imports::entity;
    let past = chrono::Utc::now() - chrono::Duration::minutes(1);
    entity::Entity::update_many()
        .col_expr(entity::Column::ExpiresAt, sea_orm::sea_query::Expr::value(past))
        .exec(app.state().db())
        .await
        .unwrap();
    work(&app).await;
    let row =
        entity::Entity::find_by_id(id.parse::<uuid::Uuid>().unwrap()).one(app.state().db()).await.unwrap().unwrap();
    assert_eq!((row.status.as_str(), row.error.as_deref()), ("failed", Some("unfinished")));
    assert!(notes(&app).await.is_empty());
});

db_test!(imports_abandoned_by_start_time_not_upload_time, |db: TestDb| async move {
    let (app, _) = setup(db).await;
    let (importer, _) = user_with(&app, "Importer", &IMPORTER).await;
    let client = app.acting_as(&importer).await;
    let id = upload(&client, &app, "notes", "title\r\nA\r\n").await.json()["id"].as_str().unwrap().to_owned();
    use rustcms_admin::imports::entity;
    // Uploaded long ago, run just now: not abandoned.
    let old = chrono::Utc::now() - chrono::Duration::hours(20);
    entity::Entity::update_many()
        .col_expr(entity::Column::CreatedAt, sea_orm::sea_query::Expr::value(old))
        .exec(app.state().db())
        .await
        .unwrap();
    client
        .post_json(&app.admin_api(&format!("imports/{id}/run")), &json!({"mapping": {"title": "title"}}))
        .await
        .assert_status(StatusCode::OK);
    let storage = app.state().service::<std::sync::Arc<dyn rustcms_core::storage::Storage>>().unwrap();
    rustcms_admin::imports::prune(app.state().db(), storage.as_ref().as_ref()).await.unwrap();
    work(&app).await;
    let row =
        entity::Entity::find_by_id(id.parse::<uuid::Uuid>().unwrap()).one(app.state().db()).await.unwrap().unwrap();
    assert_eq!(row.status, "done");
    assert_eq!(notes(&app).await, ["A"]);
});
