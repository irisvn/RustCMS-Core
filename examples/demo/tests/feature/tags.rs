//! Tags of posts (belongs-to-many) and "Move to category…" (an action with a
//! form) with the demo's policy: editors work on their own posts only, and
//! users who may not do either are refused before the body is read.

use demo::bootstrap;
use demo::database::migrations::Migrator;
use rustcms::axum::http::{Method, StatusCode};
use rustcms_testing::{Factory, TestApp, TestDb, UserFactory, db_test};
use serde_json::json;

use crate::support::seed;

db_test!(editors_tag_and_move_their_own_posts_only, migrate = Migrator, |db: TestDb| async move {
    let app = TestApp::new(bootstrap::build, db).await;
    seed(app.state()).await;
    let editor = app.login_as("editor@example.test", demo::database::seeders::user_seeder::PASSWORD).await;
    let admin = app.login_as("admin@example.test", demo::database::seeders::user_seeder::PASSWORD).await;
    let created = editor.post_json(&app.admin_api("resources/posts/records"), &json!({"title": "Mine"})).await;
    created.assert_status(StatusCode::CREATED);
    let mine = created.json()["data"]["key"].as_str().unwrap().to_owned();
    let theirs = admin.post_json(&app.admin_api("resources/posts/records"), &json!({"title": "Theirs"})).await;
    let theirs = theirs.json()["data"]["key"].as_str().unwrap().to_owned();
    let tags = admin.get(&app.admin_api("resources/tags/records?search=Rust")).await.json();
    let rust = tags["data"][0]["key"].as_str().unwrap().to_owned();
    let rel = |post: &str, rest: &str| app.admin_api(&format!("resources/posts/records/{post}/relations/tags{rest}"));

    // Their own post: tagged; someone else's: hidden (404).
    let attached = editor.post_json(&rel(&mine, "/attach"), &json!({"keys": [rust]})).await;
    attached.assert_status(StatusCode::OK);
    assert_eq!(attached.json()["processed"], json!([rust]));
    editor.post_json(&rel(&theirs, "/attach"), &json!({"keys": [rust]})).await.assert_status(StatusCode::NOT_FOUND);

    // A user with no permission: refused before the body, the post or the options.
    let nobody = UserFactory::new().create(app.state().db()).await;
    let nobody = app.acting_as(&nobody).await;
    for body in [json!("not an object"), json!({"keys": 5})] {
        nobody.post_json(&rel(&mine, "/attach"), &body).await.assert_status(StatusCode::FORBIDDEN);
        nobody
            .post_json(&app.admin_api("resources/posts/actions/move"), &body)
            .await
            .assert_status(StatusCode::FORBIDDEN);
    }
    nobody.get(&rel(&mine, "/options")).await.assert_status(StatusCode::FORBIDDEN);
    nobody.get(&format!("{}?keys={rust}", rel(&mine, "/options"))).await.assert_status(StatusCode::FORBIDDEN);
    let options = app.admin_api("resources/posts/actions/move/fields/category_id/options");
    nobody.get(&options).await.assert_status(StatusCode::FORBIDDEN);

    // The editor moves their post to a category they may see.
    let categories = editor.get(&options).await.json();
    let guides =
        categories["data"].as_array().unwrap().iter().find(|o| o["label"] == json!("Guides")).unwrap()["value"].clone();
    let moved = editor
        .post_json(
            &app.admin_api("resources/posts/actions/move"),
            &json!({"keys": [mine], "data": {"category_id": guides}}),
        )
        .await;
    moved.assert_status(StatusCode::OK);
    assert_eq!(moved.json()["processed"], json!([mine]));
    let post = editor.send(Method::GET, &app.admin_api(&format!("resources/posts/records/{mine}")), None).await;
    assert_eq!(post.json()["data"]["labels"]["category_id"], json!("Guides"));
});
