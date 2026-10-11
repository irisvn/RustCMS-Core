//! Migrations. `rustcms make:migration <name>` adds modules here.

use rustcms::migration::{MigrationTrait, MigratorTrait};

mod m20261011_155203_create_posts_table;
// rustcms:migration-mods

pub struct Migrator;

impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        let mut migrations = rustcms::migration::framework();
        let app: Vec<Box<dyn MigrationTrait>> = vec![
            Box::new(m20261011_155203_create_posts_table::Migration),
            // rustcms:migrations
        ];
        migrations.extend(app);
        migrations
    }
}
