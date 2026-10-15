//! Helpers shared by the test suites.

#![allow(dead_code)]

use lms::bootstrap;
use lms::models::{course, enrollment};
use rustcms::axum::http::{Method, StatusCode};
use rustcms::database::Seeder;
use rustcms_testing::{TestApp, TestClient, TestDb, TestResponse};
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};
use serde_json::json;

/// The app on `db`, seeded (`db:seed`).
pub async fn seeded(db: TestDb) -> TestApp {
    let app = TestApp::new(bootstrap::build, db).await;
    lms::database::seeders::DatabaseSeeder.run(app.state()).await.unwrap();
    app
}

/// Signed in as the seeded account `email`.
pub async fn as_user<'a>(app: &'a TestApp, email: &str) -> TestClient<'a> {
    let user = lms::database::seeders::user_seeder::find(app.state(), email).await.unwrap();
    app.acting_as(&user).await
}

/// The seeded course `slug`.
pub async fn course(app: &TestApp, slug: &str) -> course::Model {
    course::Entity::find().filter(course::Column::Slug.eq(slug)).one(app.state().db()).await.unwrap().unwrap()
}

pub fn records(app: &TestApp, slug: &str) -> String {
    app.admin_api(&format!("resources/{slug}/records"))
}

pub fn record(app: &TestApp, slug: &str, key: impl std::fmt::Display) -> String {
    format!("{}/{key}", records(app, slug))
}

pub async fn enroll(app: &TestApp, client: &TestClient<'_>, course: i64, learner: i64) -> TestResponse {
    let body = json!({"course_id": course.to_string(), "learner_id": learner.to_string()});
    client.post_json(&records(app, "enrollments"), &body).await
}

pub async fn patch(app: &TestApp, client: &TestClient<'_>, enrollment: &str, body: serde_json::Value) -> TestResponse {
    client.send(Method::PATCH, &record(app, "enrollments", enrollment), Some(&body)).await
}

pub fn key(response: &TestResponse) -> String {
    response.json()["data"]["key"].as_str().unwrap().to_owned()
}

/// The field errors of a 422, as text.
pub fn errors(response: &TestResponse) -> String {
    response.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    let body = response.json();
    format!("{} {}", body["errors"], body["formErrors"])
}

/// Statuses of two concurrent requests, sorted.
pub fn statuses(a: &TestResponse, b: &TestResponse) -> [StatusCode; 2] {
    let mut out = [a.status, b.status];
    out.sort();
    out
}

pub async fn active(app: &TestApp, course: i64) -> u64 {
    enrollment::Entity::find()
        .filter(enrollment::Column::CourseId.eq(course))
        .filter(enrollment::Column::Status.eq(enrollment::ACTIVE))
        .count(app.state().db())
        .await
        .unwrap()
}

/// The keys a list answers.
pub async fn listed(client: &TestClient<'_>, url: &str) -> Vec<String> {
    let page = client.get(&format!("{url}?per_page=100")).await;
    page.assert_status(StatusCode::OK);
    page.json()["data"].as_array().unwrap().iter().map(|r| r["key"].as_str().unwrap().to_owned()).collect()
}
