//! Seats: one enrollment per learner and course, never more active ones than
//! the capacity, also under concurrent writes; the welcome job goes with it.

use lms::database::factories::{course_factory::CourseFactory, learner_factory::LearnerFactory};
use lms::database::migrations::Migrator;
use lms::models::enrollment;
use rustcms::axum::http::{Method, StatusCode};
use rustcms_testing::{Factory, TestDb, db_test};
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};
use serde_json::json;

use crate::support::*;

db_test!(seats_bound_active_enrollments, migrate = Migrator, |db: TestDb| async move {
    let app = seeded(db).await;
    let staff = as_user(&app, "staff@example.test").await;
    let db = app.state().db();
    let course = CourseFactory::new().capacity(1).create(db).await;
    let (ann, bob) = (LearnerFactory.create(db).await, LearnerFactory.create(db).await);

    let first = enroll(&app, &staff, course.id, ann.id).await;
    first.assert_status(StatusCode::CREATED);
    let ann_enrollment = key(&first);
    assert!(errors(&enroll(&app, &staff, course.id, bob.id).await).contains("has no seats left"));
    // Already in: said before the seats.
    assert!(errors(&enroll(&app, &staff, course.id, ann.id).await).contains("already enrolled"));
    // Withdrawing frees the seat; coming back needs one again.
    patch(&app, &staff, &ann_enrollment, json!({"status": "withdrawn"})).await.assert_status(StatusCode::OK);
    enroll(&app, &staff, course.id, bob.id).await.assert_status(StatusCode::CREATED);
    let back = patch(&app, &staff, &ann_enrollment, json!({"status": "active"})).await;
    assert!(errors(&back).contains("has no seats left"));
    assert!(
        errors(&patch(&app, &staff, &ann_enrollment, json!({"status": "paused"})).await)
            .contains("active or withdrawn")
    );
    assert_eq!(active(&app, course.id).await, 1);
    // A key spelled another way (`0…`) is refused as input, never a server error.
    let spelled = CourseFactory::new().create(db).await;
    let body = json!({"course_id": format!("0{}", spelled.id), "learner_id": bob.id.to_string()});
    staff.post_json(&records(&app, "enrollments"), &body).await.assert_validation_error("course_id");
    // One row per learner and course.
    let rows = enrollment::Entity::find().filter(enrollment::Column::CourseId.eq(course.id)).count(db).await.unwrap();
    assert_eq!(rows, 2);

    // Moving to another course needs a seat there.
    let other = CourseFactory::new().capacity(1).create(db).await;
    let carl = LearnerFactory.create(db).await;
    enroll(&app, &staff, other.id, carl.id).await.assert_status(StatusCode::CREATED);
    let moved = patch(&app, &staff, &ann_enrollment, json!({"course_id": other.id.to_string()})).await;
    moved.assert_status(StatusCode::OK); // withdrawn: no seat needed
    let moved = patch(&app, &staff, &ann_enrollment, json!({"status": "active"})).await;
    assert!(errors(&moved).contains("has no seats left"));

    // The capacity never drops below the active enrollments, nor out of range.
    let url = record(&app, "courses", course.id);
    staff.send(Method::PATCH, &url, Some(&json!({"capacity": 2}))).await.assert_status(StatusCode::OK);
    let dan = LearnerFactory.create(db).await;
    enroll(&app, &staff, course.id, dan.id).await.assert_status(StatusCode::CREATED);
    let lower = staff.send(Method::PATCH, &url, Some(&json!({"capacity": 1}))).await;
    assert!(errors(&lower).contains("below the number of active enrollments"));
    for bad in [json!(0), json!(10_001)] {
        staff.send(Method::PATCH, &url, Some(&json!({"capacity": bad}))).await.assert_validation_error("capacity");
    }
});

db_test!(the_last_seat_goes_to_one_request, migrate = Migrator, |db: TestDb| async move {
    let app = seeded(db).await;
    let staff = as_user(&app, "staff@example.test").await;
    let other = as_user(&app, "admin@example.test").await;
    let db = app.state().db();

    // Two new enrollments.
    let course = CourseFactory::new().capacity(1).create(db).await;
    let (ann, bob) = (LearnerFactory.create(db).await, LearnerFactory.create(db).await);
    let (a, b) = tokio::join!(enroll(&app, &staff, course.id, ann.id), enroll(&app, &other, course.id, bob.id));
    assert_eq!(
        statuses(&a, &b),
        [StatusCode::CREATED, StatusCode::UNPROCESSABLE_ENTITY],
        "{} / {}",
        a.text(),
        b.text()
    );
    assert_eq!(active(&app, course.id).await, 1);

    // The same learner twice.
    let roomy = CourseFactory::new().capacity(5).create(db).await;
    let (a, b) = tokio::join!(enroll(&app, &staff, roomy.id, ann.id), enroll(&app, &other, roomy.id, ann.id));
    assert_eq!(
        statuses(&a, &b),
        [StatusCode::CREATED, StatusCode::UNPROCESSABLE_ENTITY],
        "{} / {}",
        a.text(),
        b.text()
    );

    // A new enrollment against a learner coming back.
    let race = CourseFactory::new().capacity(1).create(db).await;
    let back = key(&enroll(&app, &staff, race.id, ann.id).await);
    patch(&app, &staff, &back, json!({"status": "withdrawn"})).await.assert_status(StatusCode::OK);
    let (a, b) =
        tokio::join!(patch(&app, &staff, &back, json!({"status": "active"})), enroll(&app, &other, race.id, bob.id));
    let [first, second] = statuses(&a, &b);
    assert!(first.is_success() && second == StatusCode::UNPROCESSABLE_ENTITY, "{} / {}", a.text(), b.text());
    assert_eq!(active(&app, race.id).await, 1);

    // Two learners coming back for one seat.
    let pair = CourseFactory::new().capacity(2).create(db).await;
    let first = key(&enroll(&app, &staff, pair.id, ann.id).await);
    let second = key(&enroll(&app, &staff, pair.id, bob.id).await);
    for enrollment in [&first, &second] {
        patch(&app, &staff, enrollment, json!({"status": "withdrawn"})).await.assert_status(StatusCode::OK);
    }
    let carl = LearnerFactory.create(db).await;
    enroll(&app, &staff, pair.id, carl.id).await.assert_status(StatusCode::CREATED);
    let (a, b) = tokio::join!(
        patch(&app, &staff, &first, json!({"status": "active"})),
        patch(&app, &other, &second, json!({"status": "active"}))
    );
    assert_eq!(statuses(&a, &b), [StatusCode::OK, StatusCode::UNPROCESSABLE_ENTITY], "{} / {}", a.text(), b.text());
    assert_eq!(active(&app, pair.id).await, 2);
});

db_test!(deleting_a_course_while_its_enrollments_change_never_deadlocks, migrate = Migrator, |db: TestDb| async move {
    let app = seeded(db).await;
    let staff = as_user(&app, "staff@example.test").await;
    let other = as_user(&app, "admin@example.test").await;
    let db = app.state().db();
    for round in 0..4 {
        let course = CourseFactory::new().capacity(5).create(db).await;
        let mut enrollments = Vec::new();
        for _ in 0..3 {
            let learner = LearnerFactory.create(db).await;
            enrollments.push(key(&enroll(&app, &staff, course.id, learner.id).await));
        }
        let changes = async {
            let mut out = Vec::new();
            for enrollment in &enrollments {
                out.push(patch(&app, &staff, enrollment, json!({"status": "withdrawn"})).await);
            }
            out
        };
        let course_url = record(&app, "courses", course.id);
        let delete = other.send(Method::DELETE, &course_url, None);
        let (changed, deleted) = tokio::join!(changes, delete);
        deleted.assert_status(StatusCode::NO_CONTENT);
        for response in changed {
            assert!(
                response.status == StatusCode::OK || response.status == StatusCode::NOT_FOUND,
                "round {round}: {} {}",
                response.status,
                response.text()
            );
        }
    }
});

db_test!(enrolling_queues_a_welcome, migrate = Migrator, |db: TestDb| async move {
    use rustcms::models::job;
    let app = seeded(db).await;
    // The seeded enrollments queued theirs: run them first.
    rustcms::queue::run_until_idle(app.state(), None, 100).await.unwrap();
    let staff = as_user(&app, "staff@example.test").await;
    let db = app.state().db();
    let course = CourseFactory::new().capacity(1).create(db).await;
    let (ann, bob) = (LearnerFactory.create(db).await, LearnerFactory.create(db).await);
    let jobs = async || job::Entity::find().filter(job::Column::Kind.eq("lms.enrolled")).count(db).await.unwrap();
    enroll(&app, &staff, course.id, ann.id).await.assert_status(StatusCode::CREATED);
    assert_eq!(jobs().await, 1);
    // Refused by the rules, or failing after the job was queued (a learner
    // that does not exist): nothing queued, the job rolls back with it.
    enroll(&app, &staff, course.id, bob.id).await.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    let roomy = CourseFactory::new().create(db).await;
    enroll(&app, &staff, roomy.id, 999_999).await.assert_validation_error("learner_id");
    assert_eq!(jobs().await, 1);
    assert_eq!(rustcms::queue::run_until_idle(app.state(), None, 10).await.unwrap(), 1);
    assert_eq!(jobs().await, 0, "done");
});

db_test!(the_enroll_action_asks_for_a_learner_and_keeps_the_rules, migrate = Migrator, |db: TestDb| async move {
    let app = seeded(db).await;
    let staff = as_user(&app, "staff@example.test").await;
    let db = app.state().db();
    let course = CourseFactory::new().capacity(1).create(db).await;
    let (ann, bob) = (LearnerFactory.create(db).await, LearnerFactory.create(db).await);
    let run = |learner: serde_json::Value| json!({"keys": [course.id.to_string()], "data": {"learner_id": learner}});
    let action = app.admin_api("resources/courses/actions/enroll");

    staff.post_json(&action, &run(json!(null))).await.assert_validation_error("learner_id");
    let done = staff.post_json(&action, &run(json!(ann.id.to_string()))).await;
    done.assert_status(StatusCode::OK);
    assert_eq!(done.json()["processed"], json!([course.id.to_string()]));
    assert_eq!(active(&app, course.id).await, 1);
    // The same rules as any enrollment: once per learner, within the capacity.
    staff.post_json(&action, &run(json!(ann.id))).await.assert_validation_error("learner_id");
    let full = staff.post_json(&action, &run(json!(bob.id))).await;
    full.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    assert!(full.text().contains("has no seats left"), "{}", full.text());
    assert_eq!(active(&app, course.id).await, 1);
    // Instructors do not create enrollments: refused before anything is read.
    let teacher = as_user(&app, "teacher@example.test").await;
    teacher.post_json(&action, &json!([])).await.assert_status(StatusCode::FORBIDDEN);
});

db_test!(staff_export_enrollments_as_csv, migrate = Migrator, |db: TestDb| async move {
    let app = seeded(db).await;
    let staff = as_user(&app, "staff@example.test").await;
    let db = app.state().db();
    let course = CourseFactory::new().capacity(5).create(db).await;
    let learner = LearnerFactory.create(db).await;
    enroll(&app, &staff, course.id, learner.id).await.assert_status(StatusCode::CREATED);
    let url = app.admin_api(&format!("resources/enrollments/export?filter[course_id][eq]={}", course.id));
    let export = staff.get(&url).await;
    export.assert_status(StatusCode::OK);
    let text = export.text();
    let lines: Vec<&str> = text.split("\r\n").filter(|l| !l.is_empty()).collect();
    assert!(lines[0].starts_with("course_id,learner_id,status"), "{text}");
    assert_eq!(lines.len(), 2, "{text}");
    // Instructors do not export enrollments.
    let teacher = as_user(&app, "teacher@example.test").await;
    teacher.get(&url).await.assert_status(StatusCode::FORBIDDEN);
});

db_test!(staff_import_learners_matched_by_email, migrate = Migrator, |db: TestDb| async move {
    let app = seeded(db).await;
    let staff = as_user(&app, "staff@example.test").await;
    let db = app.state().db();
    let known = LearnerFactory.create(db).await;
    let csv = format!("name,email\r\nRenamed,{}\r\nNew Learner,new.learner@example.test\r\n", known.email);
    let request = rustcms::axum::http::Request::builder()
        .method(Method::POST)
        .uri(app.admin_api("resources/learners/imports"))
        .header("Content-Type", "text/csv")
        .body(rustcms::axum::body::Body::from(csv))
        .unwrap();
    let uploaded = staff.request(request).await;
    uploaded.assert_status(StatusCode::CREATED);
    let id = uploaded.json()["id"].as_str().unwrap().to_owned();
    let mapping = json!({"mapping": {"name": "name", "email": "email"}});
    staff.post_json(&app.admin_api(&format!("imports/{id}/run")), &mapping).await.assert_status(StatusCode::OK);
    // The seeded welcomes are queued before it.
    rustcms::queue::run_until_idle(app.state(), None, 50).await.unwrap();
    let shown = staff.get(&app.admin_api(&format!("imports/{id}"))).await.json();
    assert_eq!((shown["created"].clone(), shown["updated"].clone()), (json!(1), json!(1)), "{shown}");
    let renamed = lms::models::learner::Entity::find_by_id(known.id).one(db).await.unwrap().unwrap();
    assert_eq!(renamed.name, "Renamed");
    // Instructors do not import learners.
    let teacher = as_user(&app, "teacher@example.test").await;
    let request = rustcms::axum::http::Request::builder()
        .method(Method::POST)
        .uri(app.admin_api("resources/learners/imports"))
        .body(rustcms::axum::body::Body::from("name\r\nx\r\n"))
        .unwrap();
    teacher.request(request).await.assert_status(StatusCode::FORBIDDEN);
});
