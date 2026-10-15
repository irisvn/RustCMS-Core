//! The course overview page and the seats slot follow the user's scope;
//! the theme and the progress cell are served.

use lms::database::migrations::Migrator;
use rustcms::axum::http::StatusCode;
use rustcms_testing::{TestDb, db_test};
use serde_json::json;

use crate::support::*;

db_test!(instructors_see_their_courses_on_the_overview, migrate = Migrator, |db: TestDb| async move {
    let app = seeded(db).await;
    let teacher = as_user(&app, "teacher@example.test").await;
    let page = teacher.get(&app.admin_api("pages/course-overview")).await;
    page.assert_status(StatusCode::OK);
    let html = page.json()["content"]["html"].as_str().unwrap().to_owned();
    assert!(html.contains("Introduction to Rust") && html.contains("Web services with Axum"), "{html}");
    assert!(!html.contains("Databases with SeaORM"), "another instructor's course: {html}");
    assert!(html.contains("<meter class=\"rc-meter\""), "{html}");
    let staff = as_user(&app, "staff@example.test").await;
    let html = staff.get(&app.admin_api("pages/course-overview")).await.json()["content"]["html"].to_string();
    assert!(html.contains("Databases with SeaORM"));
});

db_test!(the_seats_slot_follows_the_course, migrate = Migrator, |db: TestDb| async move {
    let app = seeded(db).await;
    let teacher = as_user(&app, "teacher@example.test").await;
    let (theirs, others) = (course(&app, "intro-to-rust").await, course(&app, "databases-with-seaorm").await);
    let slots = teacher.get(&app.admin_api(&format!("slots?resource=courses&key={}", theirs.id))).await;
    slots.assert_status(StatusCode::OK);
    let data = slots.json()["data"].clone();
    assert_eq!(data[0]["name"], json!("course-seats"));
    assert_eq!(data[0]["label"], json!("Seats"));
    let html = data[0]["content"]["html"].as_str().unwrap();
    assert!(html.contains(&format!("{} / {}", active(&app, theirs.id).await, theirs.capacity)), "{html}");
    teacher
        .get(&app.admin_api(&format!("slots?resource=courses&key={}", others.id)))
        .await
        .assert_status(StatusCode::NOT_FOUND);
});

db_test!(the_theme_and_the_progress_cell_are_served, migrate = Migrator, |db: TestDb| async move {
    let app = seeded(db).await;
    let html = app.get("/admin").await.text().replace("&quot;", "\"");
    assert!(
        html.contains("\"logo\":\"assets/ext/logo.") && html.contains("\"favicon\":\"assets/ext/favicon."),
        "{html}"
    );
    assert!(html.contains("assets/ext/theme.") && html.contains("assets/ext/lms-progress."), "{html}");
    let staff = as_user(&app, "staff@example.test").await;
    let schema = staff.get(&app.admin_api("resources/enrollments/schema?operation=create")).await;
    let columns = schema.json()["table"]["columns"].clone();
    let progress = columns.as_array().unwrap().iter().find(|c| c["name"] == "progress").unwrap().clone();
    assert_eq!(progress["cell"], json!("lms/progress"));
});
