//! `db:seed`: complete, re-runnable, within the rules, never in production.

use lms::database::factories::learner_factory::LearnerFactory;
use lms::database::migrations::Migrator;
use lms::database::seeders::{self, DatabaseSeeder};
use lms::models::{course, enrollment, learner, lesson};
use lms::services::enrollment_service;
use rustcms::database::Seeder;
use rustcms_testing::{Factory, TestApp, TestDb, db_test};
use sea_orm::{EntityTrait, PaginatorTrait};

use crate::support::*;

db_test!(seeding_twice_changes_nothing, migrate = Migrator, |db: TestDb| async move {
    let app = seeded(db).await;
    let db = app.state().db();
    let counts = async || -> Vec<u64> {
        vec![
            course::Entity::find().count(db).await.unwrap(),
            lesson::Entity::find().count(db).await.unwrap(),
            learner::Entity::find().count(db).await.unwrap(),
            enrollment::Entity::find().count(db).await.unwrap(),
        ]
    };
    let before = counts().await;
    assert_eq!(before, vec![3, 9, 10, 10]);
    DatabaseSeeder.run(app.state()).await.unwrap();
    assert_eq!(counts().await, before);
});

db_test!(an_interrupted_seed_completes_within_the_rules, migrate = Migrator, |db: TestDb| async move {
    let app = TestApp::new(lms::bootstrap::build, db).await;
    let state = app.state();
    // A previous run stopped after the courses.
    for seeder in [
        &seeders::role_seeder::RoleSeeder as &dyn Seeder,
        &seeders::user_seeder::UserSeeder,
        &seeders::course_seeder::CourseSeeder,
    ] {
        seeder.run(state).await.unwrap();
    }
    // Meanwhile one course filled up (capacity 30 by default).
    let full = course(&app, "intro-to-rust").await;
    for _ in 0..full.capacity {
        let learner = LearnerFactory.create(state.db()).await;
        enrollment_service::enroll(state.db(), full.id, learner.id).await.unwrap().unwrap();
    }
    DatabaseSeeder.run(state).await.unwrap();
    assert_eq!(active(&app, full.id).await, full.capacity as u64, "the seeder never overfills a course");
    assert_eq!(learner::Entity::find().count(state.db()).await.unwrap(), 10 + full.capacity as u64);
});

db_test!(seeding_public_accounts_is_refused_in_production, migrate = Migrator, |db: TestDb| async move {
    let config = rustcms_testing::test_config(db.url()).with("app.env", "production").unwrap();
    let app = TestApp::with_config(lms::bootstrap::build, config, db).await;
    let refused = DatabaseSeeder.run(app.state()).await.unwrap_err();
    assert!(format!("{refused:#}").contains("refusing in production"), "{refused:#}");
    seeders::user_seeder::UserSeeder.run(app.state()).await.unwrap_err();
});
