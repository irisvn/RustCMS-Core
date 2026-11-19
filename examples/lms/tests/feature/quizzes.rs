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

db_test!(a_courses_form_enrolls_learners_in_rows, migrate = Migrator, |db: TestDb| async move {
    use lms::models::{enrollment, learner};
    let app = seeded(db).await;
    let admin = as_user(&app, "admin@example.test").await;
    // A course without enrollments yet, and a learner of none.
    let created = admin
        .post_json(
            &records(&app, "courses"),
            &json!({"title": "Rows", "slug": "rows", "capacity": 5, "published": true}),
        )
        .await;
    created.assert_status(StatusCode::CREATED);
    let course = key(&created);
    let learner =
        learner::Entity::find().order_by_asc(learner::Column::Id).one(app.state().db()).await.unwrap().unwrap();
    // Each row picks a learner (a `belongs_to` select); the seat rules run as for any enrollment.
    let row = json!({"enrollments": {"create": [{"ref": "n1", "data": {"learner_id": learner.id.to_string(), "status": "active"}}]}});
    admin.send(Method::PATCH, &record(&app, "courses", &course), Some(&row)).await.assert_status(StatusCode::OK);
    let enrolled = enrollment::Entity::find().all(app.state().db()).await.unwrap();
    assert!(enrolled.iter().any(|e| e.course_id.to_string() == course && e.learner_id == learner.id));
    // A learner that does not exist is refused in its row.
    let missing =
        json!({"enrollments": {"create": [{"ref": "n2", "data": {"learner_id": "999999", "status": "active"}}]}});
    let refused = admin.send(Method::PATCH, &record(&app, "courses", &course), Some(&missing)).await;
    assert!(errors(&refused).contains("enrollments.n2.learner_id"), "{}", refused.text());
});

db_test!(quiz_options_hold_an_image_and_an_explanation, migrate = Migrator, |db: TestDb| async move {
    let app = seeded(db).await;
    let admin = as_user(&app, "admin@example.test").await;
    let lesson = first_lesson(&app).await;
    // A file the option's image refers to must be in the media library.
    let missing = uuid_like();
    let sent = json!({"lesson_id": lesson.to_string(), "kind": "single", "prompt": "Pick", "options": {"create": [
        {"ref": "n1", "data": {"content": "a", "correct": true, "image": missing}},
    ]}});
    let refused = admin.post_json(&records(&app, "quiz_questions"), &sent).await;
    assert!(errors(&refused).contains("options.n1.image"), "{}", refused.text());
    // An explanation without images is a document like any rich text.
    let explained = json!({"type": "doc", "version": 1, "content": [{"type": "paragraph", "content": [{"type": "text", "text": "4 = 2 + 2"}]}]});
    let sent = json!({"lesson_id": lesson.to_string(), "kind": "single", "prompt": "Pick", "options": {"create": [
        {"ref": "n1", "data": {"content": "4", "correct": true, "explanation": explained}},
    ]}});
    admin.post_json(&records(&app, "quiz_questions"), &sent).await.assert_status(StatusCode::CREATED);
});

fn uuid_like() -> String {
    "0199d3a4-1b2c-7d3e-8f40-123456789abc".to_owned()
}
