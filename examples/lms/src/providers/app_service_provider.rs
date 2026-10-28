//! Jobs, seeders and console commands of the application.

use rustcms::async_trait;
use rustcms::prelude::*;

use crate::database::seeders;

pub struct AppServiceProvider;

#[async_trait]
impl ServiceProvider for AppServiceProvider {
    fn register(&self, app: &mut AppBuilder) {
        // `rustcms db:seed` runs `database` (it calls the others).
        app.seeder(seeders::DatabaseSeeder);
        app.seeder(seeders::role_seeder::RoleSeeder);
        app.seeder(seeders::user_seeder::UserSeeder);
        app.seeder(seeders::course_seeder::CourseSeeder);
        app.seeder(seeders::learner_seeder::LearnerSeeder);
        app.seeder(seeders::enrollment_seeder::EnrollmentSeeder);
        app.seeder(seeders::news_seeder::NewsSeeder);
        // rustcms:seeders
        // #region jobs
        // New enrollments queue a welcome mail.
        app.job::<crate::jobs::send_welcome::SendWelcome>();
        // #region mail-templates
        app.mail_template(rustcms::mail_template!("welcome", "en", html));
        app.mail_template(rustcms::mail_template!("welcome", "vi", html));
        // #endregion mail-templates
        // rustcms:jobs
        // #endregion jobs
        // rustcms:commands
    }
}
