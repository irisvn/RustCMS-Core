use demo::bootstrap;
use demo::database::migrations::Migrator;
use rustcms::axum::http::StatusCode;
use rustcms::models::user;
use rustcms_testing::{TestApp, TestDb, db_test};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde_json::json;

async fn user_by_email(app: &TestApp, email: &str) -> user::Model {
    user::Entity::find().filter(user::Column::Email.eq(email)).one(app.state().db()).await.unwrap().unwrap()
}

db_test!(token_api_lists_and_creates_posts, migrate = Migrator, |db: TestDb| async move {
    let app = TestApp::new(bootstrap::build, db).await;
    crate::support::seed(app.state()).await;
    let admin = user_by_email(&app, "admin@example.test").await;

    // No token, or an admin session cookie: not this API.
    app.client().get("/api/v1/posts").await.assert_status(StatusCode::UNAUTHORIZED);
    app.acting_as(&admin).await.get("/api/v1/posts").await.assert_status(StatusCode::UNAUTHORIZED);

    // A token limited to reading.
    let reader = app.with_token(&admin, &["posts.view_any"]).await;
    let list = reader.get("/api/v1/posts").await;
    list.assert_status(StatusCode::OK).assert_json("/meta/total", json!(15)).assert_json("/meta/lastPage", json!(1));
    reader.get("/api/v1/posts?page=0").await.assert_validation_error("page");
    reader.get("/api/v1/posts?page=abc").await.assert_validation_error("page");
    // Refused before the body is read, whatever it is.
    reader.post_json("/api/v1/posts", &json!({"nonsense": true})).await.assert_status(StatusCode::FORBIDDEN);

    // A token that may create: drafts, owned by the token's user, not listed.
    let writer = app.with_token(&admin, &["*"]).await;
    let created = writer.post_json("/api/v1/posts", &json!({"title": "From the API", "body": "  "})).await;
    created
        .assert_status(StatusCode::CREATED)
        .assert_json("/published", json!(false))
        .assert_json("/body", json!(null));
    let id: i64 = created.json()["id"].as_str().unwrap().parse().unwrap();
    let stored = demo::models::post::Entity::find_by_id(id).one(app.state().db()).await.unwrap().unwrap();
    assert_eq!(stored.author_id, Some(admin.id));
    assert_eq!(writer.get("/api/v1/posts").await.json()["meta"]["total"], json!(15), "drafts are not public");
    // A post moved to the trash in the panel leaves the public list.
    let first = reader.get("/api/v1/posts").await.json()["data"][0]["id"].as_str().unwrap().to_owned();
    let panel = app.acting_as(&admin).await;
    panel
        .send(rustcms::axum::http::Method::DELETE, &app.admin_api(&format!("resources/posts/records/{first}")), None)
        .await
        .assert_status(StatusCode::NO_CONTENT);
    assert_eq!(reader.get("/api/v1/posts").await.json()["meta"]["total"], json!(14), "trashed posts are not public");
    for bad in [
        json!({"title": ""}),
        json!({"title": "x".repeat(201)}),
        json!({"title": "a\nb"}),
        json!({"title": "x", "author_id": 1}),
    ] {
        assert_eq!(writer.post_json("/api/v1/posts", &bad).await.status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}");
    }
});

db_test!(api_access_comes_from_api_guard_permissions_only, migrate = Migrator, |db: TestDb| async move {
    let app = TestApp::new(bootstrap::build, db).await;
    crate::support::seed(app.state()).await;
    // The editor may create posts in the admin panel, not through the API (no API-guard role).
    let editor = user_by_email(&app, "editor@example.test").await;
    let token = app.with_token(&editor, &["*"]).await;
    token.get("/api/v1/posts").await.assert_status(StatusCode::FORBIDDEN);
    token.post_json("/api/v1/posts", &json!({"title": "x"})).await.assert_status(StatusCode::FORBIDDEN);
    // Revoking the user's credentials ends the token at once.
    let admin = user_by_email(&app, "admin@example.test").await;
    let reader = app.with_token(&admin, &["posts.view_any"]).await;
    reader.get("/api/v1/posts").await.assert_status(StatusCode::OK);
    rustcms::auth::credentials::logout_everywhere(
        app.state().db(),
        &admin,
        &rustcms::audit::AuditContext::system(None),
    )
    .await
    .unwrap();
    reader.get("/api/v1/posts").await.assert_status(StatusCode::UNAUTHORIZED);
});
