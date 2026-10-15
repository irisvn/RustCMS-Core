//! Migrations, one per change (`rustcms make:migration <name>` adds them here).

use rustcms::migration::{MigrationTrait, MigratorTrait};

mod m20261020_000001_create_courses_table;
mod m20261020_000002_create_lessons_table;
mod m20261020_000003_create_learners_table;
mod m20261020_000004_create_enrollments_table;
// rustcms:migration-mods

pub struct Migrator;

impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        let mut migrations = rustcms::migration::framework();
        let app: Vec<Box<dyn MigrationTrait>> = vec![
            Box::new(m20261020_000001_create_courses_table::Migration),
            Box::new(m20261020_000002_create_lessons_table::Migration),
            Box::new(m20261020_000003_create_learners_table::Migration),
            Box::new(m20261020_000004_create_enrollments_table::Migration),
            // rustcms:migrations
        ];
        migrations.extend(app);
        migrations
    }
}
