//! News: slugs, roles (editors write news only, viewers read and export),
//! tags linked with their own ability; lessons moved up and down in place.

use lms::database::migrations::Migrator;
use lms::models::lesson;
use rustcms::axum::http::{Method, StatusCode};
use rustcms_testing::{TestDb, db_test};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
use serde_json::json;

use crate::support::*;

db_test!(news_get_a_slug_and_refuse_a_taken_one, migrate = Migrator, |db: TestDb| async move {
    let app = seeded(db).await;
    let editor = as_user(&app, "editor@example.test").await;
    let created = editor.post_json(&records(&app, "news"), &json!({"title": "Khai giảng khoá học mới"})).await;
    created.assert_status(StatusCode::CREATED);
    assert_eq!(created.json()["data"]["data"]["slug"], json!("khai-giang-khoa-hoc-moi"));
    let twin = editor.post_json(&records(&app, "news"), &json!({"title": "Khai giảng khoá học mới"})).await;
    assert!(errors(&twin).contains("slug"), "{}", twin.text());
    let chosen = editor.post_json(&records(&app, "news"), &json!({"title": "Other", "slug": "my-own"})).await;
    assert_eq!(chosen.json()["data"]["data"]["slug"], json!("my-own"));
    // A slug cleared while editing is made again from the title.
    let key = chosen.json()["data"]["key"].as_str().unwrap().to_owned();
    let cleared =
        editor.send(Method::PATCH, &format!("{}/{key}", records(&app, "news")), Some(&json!({"slug": " "}))).await;
    cleared.assert_status(StatusCode::OK);
    assert_eq!(cleared.json()["data"]["data"]["slug"], json!("other"));
});

db_test!(editors_write_news_only_and_viewers_read, migrate = Migrator, |db: TestDb| async move {
    let app = seeded(db).await;
    let editor = as_user(&app, "editor@example.test").await;
    editor.get(&records(&app, "courses")).await.assert_status(StatusCode::FORBIDDEN);
    editor.get(&records(&app, "news")).await.assert_status(StatusCode::OK);
    let viewer = as_user(&app, "viewer@example.test").await;
    let listed = viewer.get(&records(&app, "news")).await.json();
    assert_eq!(listed["meta"]["total"], json!(3));
    viewer.post_json(&records(&app, "news"), &json!({"title": "Nope"})).await.assert_status(StatusCode::FORBIDDEN);
    viewer.post_json(&records(&app, "courses"), &json!({"title": "Nope"})).await.assert_status(StatusCode::FORBIDDEN);
    let export = viewer.get(&app.admin_api("resources/enrollments/export")).await;
    export.assert_status(StatusCode::OK);
    // The news' images, whoever uploaded them (viewing only).
    viewer.get(&records(&app, "media")).await.assert_status(StatusCode::OK);
});

db_test!(tags_are_linked_with_their_own_ability, migrate = Migrator, |db: TestDb| async move {
    let app = seeded(db).await;
    let editor = as_user(&app, "editor@example.test").await;
    let article = key(&editor.post_json(&records(&app, "news"), &json!({"title": "Tagged"})).await);
    let tag = key(&editor.post_json(&records(&app, "tags"), &json!({"name": "fresh"})).await);
    let attach = format!("{}/relations/tags/attach", record(&app, "news", &article));
    editor.post_json(&attach, &json!({"keys": [tag]})).await.assert_status(StatusCode::OK);
    // Viewers read the news but may not link tags.
    let viewer = as_user(&app, "viewer@example.test").await;
    viewer.post_json(&attach, &json!({"keys": [tag]})).await.assert_status(StatusCode::FORBIDDEN);
});

async fn positions(app: &rustcms_testing::TestApp, course: i64) -> Vec<(String, i32)> {
    lesson::Entity::find()
        .filter(lesson::Column::CourseId.eq(course))
        .order_by_asc(lesson::Column::Position)
        .order_by_asc(lesson::Column::Id)
        .all(app.state().db())
        .await
        .unwrap()
        .into_iter()
        .map(|l| (l.title, l.position))
        .collect()
}

db_test!(lessons_move_up_and_down_within_their_course, migrate = Migrator, |db: TestDb| async move {
    let app = seeded(db).await;
    let rust = course(&app, "intro-to-rust").await;
    // Ties (two lessons at the same position) are put in a total order first.
    lesson::Entity::update_many()
        .col_expr(lesson::Column::Position, sea_orm::sea_query::Expr::value(1))
        .filter(lesson::Column::CourseId.eq(rust.id))
        .filter(lesson::Column::Title.eq("Lesson 2"))
        .exec(app.state().db())
        .await
        .unwrap();
    let third = lesson::Entity::find()
        .filter(lesson::Column::CourseId.eq(rust.id))
        .filter(lesson::Column::Title.eq("Lesson 3"))
        .one(app.state().db())
        .await
        .unwrap()
        .unwrap();
    let teacher = as_user(&app, "teacher@example.test").await;
    let up = app.admin_api("resources/lessons/actions/move_up");
    teacher.post_json(&up, &json!({"keys": [third.id.to_string()]})).await.assert_status(StatusCode::OK);
    assert_eq!(
        positions(&app, rust.id).await,
        [("Lesson 1".to_owned(), 1), ("Lesson 3".to_owned(), 2), ("Lesson 2".to_owned(), 3)]
    );
    // The first one up again: nothing moves.
    let first = lesson::Entity::find()
        .filter(lesson::Column::CourseId.eq(rust.id))
        .filter(lesson::Column::Title.eq("Lesson 1"))
        .one(app.state().db())
        .await
        .unwrap()
        .unwrap();
    teacher.post_json(&up, &json!({"keys": [first.id.to_string()]})).await.assert_status(StatusCode::OK);
    assert_eq!(positions(&app, rust.id).await[0], ("Lesson 1".to_owned(), 1));
    // Another instructor's lesson: as if it did not exist, nothing moves.
    let tutor = as_user(&app, "tutor@example.test").await;
    let down = app.admin_api("resources/lessons/actions/move_down");
    let refused = tutor.post_json(&down, &json!({"keys": [first.id.to_string()]})).await.json();
    assert_eq!((refused["processed"].clone(), refused["missing"].clone()), (json!([]), json!([first.id.to_string()])));
    assert_eq!(positions(&app, rust.id).await[0], ("Lesson 1".to_owned(), 1));
});
