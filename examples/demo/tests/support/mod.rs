//! Helpers shared by the test suites.

#![allow(dead_code)]

use rustcms::AppState;
use rustcms::database::Seeder;

/// Runs `db:seed` (the `database` seeder) on `state`.
pub async fn seed(state: &AppState) {
    demo::database::seeders::DatabaseSeeder.run(state).await.unwrap();
}
