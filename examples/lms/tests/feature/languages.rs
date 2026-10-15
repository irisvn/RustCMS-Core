//! Vietnamese: labels, the rules' messages and constraint messages.

use lms::database::factories::{course_factory::CourseFactory, learner_factory::LearnerFactory};
use lms::database::migrations::Migrator;
use rustcms::axum::http::StatusCode;
use rustcms_testing::{Factory, TestApp, TestDb, db_test};
use serde_json::json;

use crate::support::*;

db_test!(rules_speak_vietnamese, migrate = Migrator, |db: TestDb| async move {
    let app = seeded(db).await;
    let staff = as_user(&app, "staff@example.test").await.languages("vi");
    let db = app.state().db();
    let course = CourseFactory::new().capacity(1).create(db).await;
    let (ann, bob) = (LearnerFactory.create(db).await, LearnerFactory.create(db).await);
    enroll(&app, &staff, course.id, ann.id).await.assert_status(StatusCode::CREATED);
    let full = enroll(&app, &staff, course.id, bob.id).await;
    assert_eq!(full.json()["errors"]["course_id"], json!(["đã hết chỗ"]));
    let again = enroll(&app, &staff, course.id, ann.id).await;
    assert_eq!(again.json()["errors"]["learner_id"], json!(["đã ghi danh khoá học này"]));
    // A unique constraint, as the database reports it.
    let body = json!({"name": "Twin", "email": ann.email});
    let twin = staff.post_json(&records(&app, "learners"), &body).await;
    assert_eq!(twin.json()["errors"]["email"], json!(["đã được đăng ký"]), "{}", twin.text());
});

db_test!(every_label_speaks_vietnamese, migrate = Migrator, |db: TestDb| async move {
    let app = TestApp::new(lms::bootstrap::build, db).await;
    let state = app.state();
    let panel = state.service::<rustcms::admin::AdminPanel>().unwrap();
    let auth = state.service::<rustcms::auth::AuthState>().unwrap();
    let groups: Vec<String> = auth.permissions.all().into_values().flatten().collect();
    assert_eq!(panel.missing_translations("vi", &groups), Vec::<String>::new());
});
