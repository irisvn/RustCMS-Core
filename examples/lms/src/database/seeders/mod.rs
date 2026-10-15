//! Seeders: `rustcms db:seed` runs [`DatabaseSeeder`], which calls the others
//! in order (`rustcms make:seeder <Name>` adds them here). Each may run again:
//! it finds rows by a natural key before creating them.

use rustcms::database::Seeder;
use rustcms::{AppState, async_trait};

pub mod course_seeder;
pub mod enrollment_seeder;
pub mod learner_seeder;
pub mod role_seeder;
pub mod user_seeder;
// rustcms:seeder-mods

pub struct DatabaseSeeder;

#[async_trait]
impl Seeder for DatabaseSeeder {
    fn name(&self) -> &'static str {
        "database"
    }

    async fn run(&self, state: &AppState) -> anyhow::Result<()> {
        rustcms::database::call(
            state,
            &[
                &role_seeder::RoleSeeder,
                &user_seeder::UserSeeder,
                &course_seeder::CourseSeeder,
                &learner_seeder::LearnerSeeder,
                &enrollment_seeder::EnrollmentSeeder,
                // rustcms:seeder-calls
            ],
        )
        .await?;
        println!("Sign in at /admin as admin@, staff@ or teacher@example.test (password: {}).", user_seeder::PASSWORD);
        Ok(())
    }
}
