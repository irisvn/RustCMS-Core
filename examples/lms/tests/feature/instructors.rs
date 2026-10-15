//! Instructors work on the courses they teach: lists, direct ids, options,
//! relations and writes; progress is all they change on an enrollment.

use lms::database::factories::learner_factory::LearnerFactory;
use lms::database::migrations::Migrator;
use lms::models::enrollment;
use rustcms::axum::http::{Method, StatusCode};
use rustcms_testing::{Factory, TestDb, db_test};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde_json::json;

use crate::support::*;

db_test!(instructors_see_their_courses_only, migrate = Migrator, |db: TestDb| async move {
    let app = seeded(db).await;
    let teacher = as_user(&app, "teacher@example.test").await;
    let staff = as_user(&app, "staff@example.test").await;
    let (theirs, others) = (course(&app, "intro-to-rust").await, course(&app, "databases-with-seaorm").await);

    // Lists: what they teach. Seeded learners take one course each, so add one
    // in both of their courses: listed once.
    let axum = course(&app, "web-services-with-axum").await;
    let both = LearnerFactory.create(app.state().db()).await;
    for course in [theirs.id, axum.id] {
        enroll(&app, &staff, course, both.id).await.assert_status(StatusCode::CREATED);
    }
    assert_eq!(listed(&teacher, &records(&app, "courses")).await.len(), 2);
    assert_eq!(listed(&staff, &records(&app, "courses")).await.len(), 3);
    assert_eq!(listed(&teacher, &records(&app, "lessons")).await.len(), 6);
    assert_eq!(listed(&teacher, &records(&app, "enrollments")).await.len(), 9);
    let learners = listed(&teacher, &records(&app, "learners")).await;
    let mut unique = learners.clone();
    unique.sort();
    unique.dedup();
    assert_eq!((learners.len(), unique.len()), (8, 8), "each learner once: {learners:?}");

    // Other courses do not exist for them: ids, relations, options.
    teacher.get(&record(&app, "courses", others.id)).await.assert_status(StatusCode::NOT_FOUND);
    let relation = app.admin_api(&format!("resources/courses/records/{}/relations/enrollments", others.id));
    teacher.get(&relation).await.assert_status(StatusCode::NOT_FOUND);
    let options = teacher.get(&app.admin_api("resources/lessons/fields/course_id/options")).await;
    options.assert_status(StatusCode::OK);
    let offered: Vec<String> =
        options.json()["data"].as_array().unwrap().iter().map(|o| o["value"].to_string().replace('"', "")).collect();
    assert!(offered.contains(&theirs.id.to_string()) && !offered.contains(&others.id.to_string()), "{offered:?}");
    let foreign = enrollment::Entity::find()
        .filter(enrollment::Column::CourseId.eq(others.id))
        .one(app.state().db())
        .await
        .unwrap()
        .unwrap();
    teacher.get(&record(&app, "enrollments", foreign.id)).await.assert_status(StatusCode::NOT_FOUND);
    patch(&app, &teacher, &foreign.id.to_string(), json!({"progress": 99})).await.assert_status(StatusCode::NOT_FOUND);
    teacher.get(&record(&app, "learners", foreign.learner_id)).await.assert_status(StatusCode::NOT_FOUND);
    // Nor as a lesson's course, directly or through a course page.
    let body = json!({"course_id": others.id.to_string(), "title": "Sneaky"});
    teacher.post_json(&records(&app, "lessons"), &body).await.assert_validation_error("course_id");
    let through = app.admin_api(&format!("resources/courses/records/{}/relations/lessons", others.id));
    teacher.post_json(&through, &json!({"title": "Sneaky"})).await.assert_status(StatusCode::NOT_FOUND);
    let mine = app.admin_api(&format!("resources/courses/records/{}/relations/lessons", theirs.id));
    let overridden = json!({"title": "Moved?", "course_id": others.id.to_string()});
    teacher.post_json(&mine, &overridden).await.assert_validation_error("course_id");
    teacher.post_json(&mine, &json!({"title": "Extra"})).await.assert_status(StatusCode::CREATED);
});

db_test!(instructors_record_progress_and_nothing_else, migrate = Migrator, |db: TestDb| async move {
    let app = seeded(db).await;
    let teacher = as_user(&app, "teacher@example.test").await;
    let staff = as_user(&app, "staff@example.test").await;
    let theirs = course(&app, "intro-to-rust").await;
    let own = enrollment::Entity::find()
        .filter(enrollment::Column::CourseId.eq(theirs.id))
        .one(app.state().db())
        .await
        .unwrap()
        .unwrap();
    let own_key = own.id.to_string();

    let updated = patch(&app, &teacher, &own_key, json!({"progress": 55})).await;
    updated.assert_status(StatusCode::OK);
    // The answer says what they may do next: update, not delete.
    assert_eq!(updated.json()["data"]["abilities"]["update"], json!(true));
    assert_eq!(updated.json()["data"]["abilities"]["delete"], json!(false));
    patch(&app, &teacher, &own_key, json!({"progress": 101})).await.assert_validation_error("progress");
    let status = patch(&app, &teacher, &own_key, json!({"status": "withdrawn"})).await;
    assert!(status.status.is_client_error(), "{}", status.text());
    let stored = enrollment::Entity::find_by_id(own.id).one(app.state().db()).await.unwrap().unwrap();
    assert_eq!((stored.progress, stored.status.as_str()), (55, enrollment::ACTIVE));

    // Enrolling, deleting and editing learners: refused before any lookup
    // (403 whether the record exists or not).
    let learner = LearnerFactory.create(app.state().db()).await;
    enroll(&app, &teacher, theirs.id, learner.id).await.assert_status(StatusCode::FORBIDDEN);
    for key in [own_key.as_str(), "999999"] {
        teacher
            .send(Method::DELETE, &record(&app, "enrollments", key), None)
            .await
            .assert_status(StatusCode::FORBIDDEN);
    }
    let rename = json!({"name": "X"});
    teacher
        .send(Method::PATCH, &record(&app, "learners", own.learner_id), Some(&rename))
        .await
        .assert_status(StatusCode::FORBIDDEN);
    // Nobody sets progress when enrolling.
    let body = json!({"course_id": theirs.id.to_string(), "learner_id": learner.id.to_string(), "progress": 50});
    staff.post_json(&records(&app, "enrollments"), &body).await.assert_validation_error("progress");

    // Only staff choose who teaches; once reassigned, the former instructor
    // no longer reaches the course's enrollments.
    let tutor = lms::database::seeders::user_seeder::find(app.state(), "tutor@example.test").await.unwrap();
    let url = record(&app, "courses", theirs.id);
    let reassign = json!({"instructor_id": tutor.id.to_string()});
    let refused = teacher.send(Method::PATCH, &url, Some(&reassign)).await;
    assert!(refused.status.is_client_error(), "{}", refused.text());
    staff
        .send(Method::PATCH, &url, Some(&json!({"instructor_id": "999999"})))
        .await
        .assert_validation_error("instructor_id");
    staff.send(Method::PATCH, &url, Some(&reassign)).await.assert_status(StatusCode::OK);
    patch(&app, &teacher, &own_key, json!({"progress": 60})).await.assert_status(StatusCode::NOT_FOUND);
    assert_eq!(listed(&teacher, &records(&app, "courses")).await.len(), 1);
});
