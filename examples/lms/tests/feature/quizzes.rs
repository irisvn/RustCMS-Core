//! Quiz questions (ADR 0044): one form edits a question and its options or
//! blanks; switching a choice question to blanks removes its options when
//! saved; options are the rows without a blank; hints show where the
//! question explains and the option is correct.

use lms::database::migrations::Migrator;
use lms::models::{lesson, quiz_option};
use rustcms::axum::http::{Method, StatusCode};
use rustcms_testing::{TestDb, db_test};
use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QueryOrder};
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
    let of_question = || quiz_option::Column::QuizQuestionId.eq(question.parse::<i64>().unwrap());
    let options = quiz_option::Entity::find()
        .filter(of_question())
        .order_by_asc(quiz_option::Column::Position)
        .all(app.state().db())
        .await
        .unwrap();
    let shown: Vec<_> = options.iter().map(|o| (o.content.as_str(), o.hint.as_deref(), o.blank_id)).collect();
    assert_eq!(shown, [("4", Some("Count them"), None), ("5", None, None)]);
    // Switched to blanks in the same save as its first blank.
    let blank = admin
        .send(
            Method::PATCH,
            &record(&app, "quiz_questions", &question),
            Some(&json!({"kind": "fill_blank", "blanks": {"create": [{"ref": "b1", "data": {"label": "first", "choices": {
                "create": [{"ref": "c1", "data": {"content": "4", "correct": true, "hint": "Count"}}],
            }}}]}})),
        )
        .await;
    blank.assert_status(StatusCode::OK);
    // The switch removed the options; the blank is there.
    let options = quiz_option::Entity::find().filter(of_question()).filter(quiz_option::Column::BlankId.is_null());
    assert_eq!(options.all(app.state().db()).await.unwrap().len(), 0);
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

db_test!(a_blank_question_is_written_with_its_choices, migrate = Migrator, |db: TestDb| async move {
    use lms::models::quiz_blank;
    let app = seeded(db).await;
    let admin = as_user(&app, "admin@example.test").await;
    let lesson = first_lesson(&app).await;
    // The question, its blanks and their choices in one save (rows of rows, ADR 0044).
    let created = admin
        .post_json(
            &records(&app, "quiz_questions"),
            &json!({"lesson_id": lesson.to_string(), "kind": "fill_blank", "prompt": "___ builds, ___ installs", "blanks": {
                "create": [
                    {"ref": "b1", "data": {"label": "builds", "choices": {"create": [
                        {"ref": "c1", "data": {"content": "cargo", "correct": true}},
                        {"ref": "c2", "data": {"content": "make", "correct": false}},
                    ]}}},
                    {"ref": "b2", "data": {"label": "installs", "choices": {"create": [
                        {"ref": "c1", "data": {"content": "rustup", "correct": true}},
                    ]}}},
                ],
            }}),
        )
        .await;
    created.assert_status(StatusCode::CREATED);
    let question: i64 = key(&created).parse().unwrap();
    let db = app.state().db();
    let blanks = quiz_blank::Entity::find()
        .filter(quiz_blank::Column::QuizQuestionId.eq(question))
        .order_by_asc(quiz_blank::Column::Position)
        .all(db)
        .await
        .unwrap();
    assert_eq!(blanks.len(), 2);
    // Each choice names its blank and, from the server, its question (the double foreign key holds).
    let choices = quiz_option::Entity::find()
        .filter(quiz_option::Column::BlankId.eq(blanks[0].id))
        .order_by_asc(quiz_option::Column::Position)
        .all(db)
        .await
        .unwrap();
    let read: Vec<_> = choices.iter().map(|c| (c.content.as_str(), c.quiz_question_id, c.position)).collect();
    assert_eq!(read, [("cargo", question, 1), ("make", question, 2)]);
    // Choices are not the question's options.
    let options =
        admin.get(&format!("{}/relations/options", record(&app, "quiz_questions", question.to_string()))).await;
    assert_eq!(options.json()["meta"]["total"], json!(0));
    // Removing a blank removes its choices.
    let removed = json!({"blanks": {"delete": [blanks[1].id.to_string()]}});
    admin
        .send(Method::PATCH, &record(&app, "quiz_questions", question.to_string()), Some(&removed))
        .await
        .assert_status(StatusCode::OK);
    let left =
        quiz_option::Entity::find().filter(quiz_option::Column::QuizQuestionId.eq(question)).all(db).await.unwrap();
    assert_eq!(left.len(), 2);
    // A choice's blank is the server's: never moved by a client (here, out to the question's options).
    let moved = json!({"blank_id": null});
    let refused = admin.send(Method::PATCH, &record(&app, "quiz_options", left[0].id), Some(&moved)).await;
    assert!(errors(&refused).contains("blank_id"), "{}", refused.text());
});

db_test!(a_single_answer_question_has_exactly_one_correct_option, migrate = Migrator, |db: TestDb| async move {
    let app = seeded(db).await;
    let admin = as_user(&app, "admin@example.test").await;
    let lesson = first_lesson(&app).await;
    let question = |options: serde_json::Value| json!({"lesson_id": lesson.to_string(), "kind": "single", "prompt": "Pick one", "options": {"create": options}});
    // Two correct options, or none: refused at the options, nothing saved.
    let two = json!([
        {"ref": "n1", "data": {"content": "a", "correct": true}},
        {"ref": "n2", "data": {"content": "b", "correct": true}},
    ]);
    let refused = admin.post_json(&records(&app, "quiz_questions"), &question(two)).await;
    assert!(errors(&refused).contains("must have exactly one correct option"), "{}", refused.text());
    let refused = admin.post_json(&records(&app, "quiz_questions"), &question(json!([]))).await;
    assert!(errors(&refused).contains("options"), "{}", refused.text());
    let one = json!([
        {"ref": "n1", "data": {"content": "a", "correct": true}},
        {"ref": "n2", "data": {"content": "b", "correct": false}},
    ]);
    admin.post_json(&records(&app, "quiz_questions"), &question(one)).await.assert_status(StatusCode::CREATED);
    // A blank without a correct choice is refused at that blank.
    let blank = json!({"lesson_id": lesson.to_string(), "kind": "fill_blank", "prompt": "___", "blanks": {"create": [
        {"ref": "b1", "data": {"label": "it", "choices": {"create": [{"ref": "c1", "data": {"content": "x"}}]}}},
    ]}});
    let refused = admin.post_json(&records(&app, "quiz_questions"), &blank).await;
    assert!(errors(&refused).contains("blanks.b1.choices"), "{}", refused.text());
});

db_test!(the_prompt_keeps_the_blanks_in_step, migrate = Migrator, |db: TestDb| async move {
    let app = seeded(db).await;
    let admin = as_user(&app, "admin@example.test").await;
    let url = app.admin_api("resources/quiz_questions/form/react");
    // Two `___`, one blank: the second is added.
    let ask = |prompt: &str, blanks: serde_json::Value| {
        json!({"operation": "create", "changed": "prompt", "seq": 1,
            "state": {"kind": "fill_blank", "prompt": prompt, "blanks": blanks}})
    };
    let answer = admin.post_json(&url, &ask("a ___ b ___", json!([{"ref": "n1", "data": {"label": "one"}}]))).await;
    answer.assert_status(StatusCode::OK);
    let patch = answer.json()["patches"][0].clone();
    assert_eq!((patch["op"].clone(), patch["data"]["label"].clone()), (json!("add_relation_row"), json!("Blank 2")));
    // One `___`, two blanks: the extra one is flagged, not removed.
    let two = json!([{"ref": "n1", "data": {"label": "one"}}, {"ref": "n2", "data": {"label": "two"}}]);
    let answer = admin.post_json(&url, &ask("a ___", two)).await.json();
    assert_eq!(answer["patches"][0]["op"], json!("flag_relation_row"));
    assert_eq!(answer["patches"][0]["id"], json!("n2"));
});

db_test!(a_published_course_with_lessons_needs_a_summary, migrate = Migrator, |db: TestDb| async move {
    use lms::models::course;
    let app = seeded(db).await;
    let admin = as_user(&app, "admin@example.test").await;
    let seeded = course::Entity::find().order_by_asc(course::Column::Id).one(app.state().db()).await.unwrap().unwrap();
    let url = record(&app, "courses", seeded.id);
    // Published, with lessons: a summary is required (a rule computed on the server).
    let refused = admin.send(Method::PATCH, &url, Some(&json!({"summary": null, "published": true}))).await;
    assert!(errors(&refused).contains("summary"), "{}", refused.text());
    // Unpublished: none needed.
    let saved = admin.send(Method::PATCH, &url, Some(&json!({"summary": null, "published": false}))).await;
    saved.assert_status(StatusCode::OK);
    // A user who may edit courses but not list lessons is not held to it (the rule reads what they see).
    {
        use rustcms::auth::{ADMIN_GUARD, Actor, AuthState, RbacService};
        use rustcms_testing::Factory;
        let auth = app.state().service::<AuthState>().unwrap();
        let rbac = RbacService::new(app.state().db(), &auth.resolver);
        let permissions: Vec<String> =
            ["courses.view_any", "courses.view", "courses.update", lms::policies::permissions::ALL_COURSES]
                .map(str::to_owned)
                .to_vec();
        let role = rbac.create_role(Actor::System, ADMIN_GUARD, "course_editor", &permissions).await.unwrap();
        let user = rustcms_testing::UserFactory::new().create(app.state().db()).await;
        rbac.assign_role(Actor::System, user.id, role.id).await.unwrap();
        let editor = app.acting_as(&user).await;
        let saved = editor.send(Method::PATCH, &url, Some(&json!({"summary": null, "published": true}))).await;
        saved.assert_status(StatusCode::OK);
    }
    // A new course (no lessons yet) is created without one.
    let body = json!({"title": "New", "slug": "new-course", "capacity": 5, "published": true});
    admin.post_json(&records(&app, "courses"), &body).await.assert_status(StatusCode::CREATED);
});

fn uuid_like() -> String {
    "0199d3a4-1b2c-7d3e-8f40-123456789abc".to_owned()
}
