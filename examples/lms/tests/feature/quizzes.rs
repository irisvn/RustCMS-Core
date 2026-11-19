//! Quiz questions (ADR 0044): one form edits a question and its options or
//! blanks; switching a choice question to blanks removes its options when
//! saved; options are the rows without a blank; hints show where the
//! question explains and the option is correct.

use lms::database::migrations::Migrator;
use lms::models::{lesson, quiz_option};
use rustcms::axum::http::{Method, StatusCode};
use rustcms_testing::{TestDb, db_test};
use sea_orm::{ConnectionTrait, EntityTrait, QueryOrder};
use serde_json::json;

use crate::support::*;

async fn first_lesson(app: &rustcms_testing::TestApp) -> i64 {
    lesson::Entity::find().order_by_asc(lesson::Column::Id).one(app.state().db()).await.unwrap().unwrap().id
}

db_test!(a_question_switched_to_blanks_loses_its_options, migrate = Migrator, |db: TestDb| async move {
    let app = seeded(db).await;
    let admin = as_user(&app, "admin@example.test").await;
    let lesson = first_lesson(&app).await;
    let created = admin
        .post_json(
            &records(&app, "quiz_questions"),
            &json!({"lesson_id": lesson.to_string(), "kind": "single", "prompt": "2 + 2?", "explain": true, "options": {
                "create": [
                    {"ref": "n1", "data": {"content": "4", "correct": true, "hint": "Count them"}},
                    {"ref": "n2", "data": {"content": "5", "correct": false, "hint": "dropped"}},
                ],
                "order": [{"new": "n1"}, {"new": "n2"}],
            }}),
        )
        .await;
    created.assert_status(StatusCode::CREATED);
    let question = key(&created);
    // Hints only on correct options; the options are the rows without a blank.
    let options =
        quiz_option::Entity::find().order_by_asc(quiz_option::Column::Position).all(app.state().db()).await.unwrap();
    let shown: Vec<_> = options.iter().map(|o| (o.content.as_str(), o.hint.as_deref(), o.blank_id)).collect();
    assert_eq!(shown, [("4", Some("Count them"), None), ("5", None, None)]);
    // Switched to blanks in the same save as its first blank.
    let blank = admin
        .send(
            Method::PATCH,
            &record(&app, "quiz_questions", &question),
            Some(&json!({"kind": "fill_blank", "blanks": {"create": [{"ref": "b1", "data": {"label": "first"}}]}})),
        )
        .await;
    blank.assert_status(StatusCode::OK);
    // The switch removed the options; the blank is there.
    assert_eq!(quiz_option::Entity::find().all(app.state().db()).await.unwrap().len(), 0);
    let blanks = admin.get(&format!("{}/relations/blanks", record(&app, "quiz_questions", &question))).await.json();
    assert_eq!(blanks["data"][0]["data"]["label"], json!("first"));
    // A choice of that blank, stored meanwhile, is not one of the options.
    let blank_id = blanks["data"][0]["key"].as_str().unwrap();
    app.state()
        .db()
        .execute_unprepared(&format!(
            "INSERT INTO quiz_options (quiz_question_id, blank_id, content, correct, position) VALUES ({question}, \
             {blank_id}, 'four', true, 1)"
        ))
        .await
        .unwrap();
    let listed = admin.get(&format!("{}/relations/options", record(&app, "quiz_questions", &question))).await.json();
    assert_eq!(listed["meta"]["total"], json!(0));
});
