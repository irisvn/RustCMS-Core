//! Seeders: `rustcms db:seed` runs [`DatabaseSeeder`], which calls the others
//! in order (`rustcms make:seeder <Name>` adds them here). Each may run again:
//! it finds rows by a natural key before creating them.

use rustcms::database::Seeder;
use rustcms::{AppState, async_trait};

pub mod category_seeder;
pub mod post_seeder;
pub mod tag_seeder;
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
                &user_seeder::UserSeeder,
                &category_seeder::CategorySeeder,
                &post_seeder::PostSeeder,
                &tag_seeder::TagSeeder,
                // rustcms:seeder-calls
            ],
        )
        .await?;
        println!(
            "Sign in at /admin as admin@example.test or editor@example.test (password: {}).",
            user_seeder::PASSWORD
        );
        Ok(())
    }
}
