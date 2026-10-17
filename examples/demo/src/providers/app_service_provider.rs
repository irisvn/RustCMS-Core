//! Jobs, seeders and console commands of the application
//! (`rustcms make:job`, `make:seeder`, `make:command` add them here).

use rustcms::async_trait;
use rustcms::prelude::*;

use crate::database::seeders;

pub struct AppServiceProvider;

#[async_trait]
impl ServiceProvider for AppServiceProvider {
    fn register(&self, app: &mut AppBuilder) {
        // The live page's counter (one producer for every viewer).
        let ticker = crate::services::ticker::Ticker::new(&app.shutdown());
        app.bind(ticker);
        // `rustcms db:seed` runs `database` (it calls the others).
        app.seeder(seeders::DatabaseSeeder);
        app.seeder(seeders::user_seeder::UserSeeder);
        app.seeder(seeders::category_seeder::CategorySeeder);
        app.seeder(seeders::post_seeder::PostSeeder);
        // rustcms:seeders
        // rustcms:jobs
        // rustcms:commands
    }
}
