//! Actions with a form (posts `move`): admitted before the body, values
//! checked and canonical before the idempotency claim, references checked in
//! the action's transaction, options from the target's scope. SQLite,
//! PostgreSQL, MySQL.

mod support;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use rustcms_auth::{ADMIN_GUARD, Actor, AuthState, RbacService};
use rustcms_core::models::user;
use rustcms_testing::{Factory, TestApp, TestClient, TestDb, TestResponse, UserFactory, db_test};
use sea_orm::EntityTrait;
use serde_json::{Value, json};

use support::app::setup;
use support::post;

async fn mover(app: &TestApp) -> user::Model {
    let auth = app.state().service::<AuthState>().unwrap();
    let db = app.state().db();
    let rbac = RbacService::new(db, &auth.resolver);
    let user = UserFactory::new().name("Mover").create(db).await;
    let permissions: Vec<String> =
        ["posts.view_any", "posts.view", "posts.create", "posts.move", "categories.view_any", "categories.view"]
            .iter()
            .map(|p| (*p).to_owned())
            .collect();
    let role = rbac.create_role(Actor::System, ADMIN_GUARD, "mover", &permissions).await.unwrap();
    rbac.assign_role(Actor::System, user.id, role.id).await.unwrap();
    user
}

async fn create(client: &TestClient<'_>, app: &TestApp, path: &str, body: Value) -> String {
    let created = client.post_json(&app.admin_api(path), &body).await;
    created.assert_status(StatusCode::CREATED);
    created.json()["data"]["key"].as_str().unwrap().to_owned()
}

async fn send(client: &TestClient<'_>, app: &TestApp, body: &str, key: Option<&str>) -> TestResponse {
    let mut request = Request::builder()
        .method(Method::POST)
        .uri(app.admin_api("resources/posts/actions/move"))
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(key) = key {
        request = request.header("Idempotency-Key", key);
    }
    client.request(request.body(Body::from(body.to_owned())).unwrap()).await
}

async fn post_title(app: &TestApp, key: &str) -> (String, Option<i32>) {
    let row =
        post::Entity::find_by_id(key.parse::<uuid::Uuid>().unwrap()).one(app.state().db()).await.unwrap().unwrap();
    (row.title, row.category_id)
}

db_test!(action_forms_are_admitted_checked_and_canonical, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    let news = create(&admin, &app, "resources/categories/records", json!({"name": "News", "slug": "news"})).await;
    let hidden =
        create(&admin, &app, "resources/categories/records", json!({"name": "Hidden one", "slug": "hidden-one"})).await;
    let user = mover(&app).await;
    let client = app.acting_as(&user).await;
    let post = create(&client, &app, "resources/posts/records", json!({"title": "Mine"})).await;

    // No admission: 403 whatever the body.
    let editor = app.acting_as(&users.editor).await;
    for body in ["not json", "{}", r#"{"keys": 1}"#] {
        send(&editor, &app, body, None).await.assert_status(StatusCode::FORBIDDEN);
    }
    let unknown = editor.post_json(&app.admin_api("resources/posts/actions/nope"), &json!({"keys": []})).await;
    unknown.assert_status(StatusCode::NOT_FOUND);

    // The form's rules, then the reference (in scope for the user).
    let run = |data: Value| json!({"keys": [post], "data": data}).to_string();
    send(&client, &app, "not json", None).await.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    send(&client, &app, &run(json!({})), None).await.assert_validation_error("category_id");
    send(&client, &app, &run(json!({"category_id": news, "x": 1})), None).await.assert_validation_error("x");
    send(&client, &app, &run(json!({"category_id": news, "mode": "fly"})), None).await.assert_validation_error("mode");
    send(&client, &app, &run(json!({"category_id": news, "weight": "heavy"})), None)
        .await
        .assert_validation_error("weight");
    send(&client, &app, &run(json!({"category_id": news, "note": "x".repeat(11)})), None)
        .await
        .assert_validation_error("note");
    send(&client, &app, &run(json!({"category_id": "garbage"})), None).await.assert_validation_error("category_id");
    send(&client, &app, &run(json!({"category_id": hidden})), None).await.assert_validation_error("category_id");
    send(&client, &app, &run(json!({"category_id": "999999"})), None).await.assert_validation_error("category_id");
    assert_eq!(post_title(&app, &post).await, ("Mine".to_owned(), None));

    // Canonical data reaches the handler: every field, defaults, keys as text.
    let number: i32 = news.parse().unwrap();
    let first = send(&client, &app, &run(json!({"category_id": number, "weight": 2})), Some("move-1")).await;
    first.assert_status(StatusCode::OK);
    assert_eq!(first.json()["processed"], json!([post]));
    assert_eq!(post_title(&app, &post).await, ("\"move\"|2|null".to_owned(), Some(number)));
    // The same request written differently shares the claim: a replay, not a mismatch.
    let again = send(
        &client,
        &app,
        &run(json!({"category_id": news, "mode": "move", "weight": 2, "note": null})),
        Some("move-1"),
    )
    .await;
    again.assert_status(StatusCode::OK);
    assert_eq!(again.json()["replayed"], json!(true));
    // Even once the category is gone: the committed run is reported, not checked again.
    admin.send(Method::DELETE, &app.admin_api(&format!("resources/categories/records/{news}")), None).await;
    let replay = send(&client, &app, &run(json!({"category_id": number, "weight": 2})), Some("move-1")).await;
    replay.assert_status(StatusCode::OK);
    assert_eq!(replay.json()["replayed"], json!(true));

    // An action without a form takes no data.
    let publish = json!({"keys": [post], "data": {"x": 1}});
    let refused = admin.post_json(&app.admin_api("resources/posts/actions/publish"), &publish).await;
    refused.assert_validation_error("x");
});

db_test!(action_form_options_and_schema, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    let news = create(&admin, &app, "resources/categories/records", json!({"name": "News", "slug": "news"})).await;
    create(&admin, &app, "resources/categories/records", json!({"name": "Hidden one", "slug": "hidden-one"})).await;
    let user = mover(&app).await;
    let client = app.acting_as(&user).await;
    let options = |path: &str| app.admin_api(&format!("resources/posts/actions/{path}"));

    let found = client.get(&options("move/fields/category_id/options")).await;
    found.assert_status(StatusCode::OK);
    assert_eq!(found.json()["data"], json!([{"value": news, "label": "News"}]));
    client.get(&options("move/fields/note/options")).await.assert_status(StatusCode::NOT_FOUND);
    client.get(&options("move/fields/nope/options")).await.assert_status(StatusCode::NOT_FOUND);
    let editor = app.acting_as(&users.editor).await;
    editor.get(&options("move/fields/category_id/options")).await.assert_status(StatusCode::FORBIDDEN);

    let schema = client.get(&app.admin_api("resources/posts/schema")).await;
    let action =
        schema.json()["table"]["actions"].as_array().unwrap().iter().find(|a| a["name"] == json!("move")).cloned();
    let form = action.unwrap()["form"].clone();
    let fields: Vec<(Value, Value)> =
        form["components"].as_array().unwrap().iter().map(|i| (i["name"].clone(), i["type"].clone())).collect();
    assert_eq!(
        fields,
        [
            (json!("category_id"), json!("select")),
            (json!("mode"), json!("select")),
            (json!("weight"), json!("number_input")),
            (json!("note"), json!("text_input")),
        ]
    );
    assert_eq!(form["components"][0]["relationship"], json!({"resource": "categories"}));
    assert_eq!(form["components"][1]["default"], json!("move"));
    assert_eq!(schema.json()["schemaVersion"], json!(2));
});

db_test!(datetime_action_fields_are_instants_in_canonical_utc, |db: TestDb| async move {
    let (app, users) = setup(db).await;
    let admin = app.acting_as(&users.admin).await;
    let post = create(&admin, &app, "resources/posts/records", json!({"title": "Later"})).await;
    let path = app.admin_api("resources/posts/actions/schedule");
    let run = |at: Value| json!({"keys": [post], "data": {"at": at}});

    // Only RFC 3339 with an offset (ADR 0003): no dates, local times or numbers.
    for at in [json!("2026-10-12"), json!("2026-10-12T09:30:00"), json!(1_760_000_000), json!(null)] {
        admin.post_json(&path, &run(at.clone())).await.assert_validation_error("at");
    }
    // The handler gets the instant in UTC, whatever offset it was sent with.
    admin.post_json(&path, &run(json!("2026-10-12T09:30:00+07:00"))).await.assert_status(StatusCode::OK);
    let row =
        post::Entity::find_by_id(post.parse::<uuid::Uuid>().unwrap()).one(app.state().db()).await.unwrap().unwrap();
    assert_eq!(row.published_at.unwrap().to_rfc3339(), "2026-10-12T02:30:00+00:00");

    // The schema offers it as the form's instant picker.
    let schema = admin.get(&app.admin_api("resources/posts/schema")).await.json();
    let action =
        schema["table"]["actions"].as_array().unwrap().iter().find(|a| a["name"] == json!("schedule")).cloned();
    let input = action.unwrap()["form"]["components"][0].clone();
    assert_eq!((&input["type"], &input["valueType"]), (&json!("date_time_picker"), &json!("instant")));
});
