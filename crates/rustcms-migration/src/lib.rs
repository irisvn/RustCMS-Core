//! Migrations owned by the framework.
//!
//! Applications chain them before their own:
//!
//! ```rust,ignore
//! fn migrations() -> Vec<Box<dyn MigrationTrait>> {
//!     let mut all = rustcms_migration::migrations();
//!     all.extend(app_migrations());
//!     all
//! }
//! ```
//!
//! Released migrations are never edited; changes ship as new migrations.
#![warn(missing_docs)]

use sea_orm_migration::prelude::*;

mod m20261011_000001_create_users_table;
mod m20261011_000002_create_sessions_table;

/// Framework migrations, in order.
pub fn migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![
        Box::new(m20261011_000001_create_users_table::Migration),
        Box::new(m20261011_000002_create_sessions_table::Migration),
    ]
}

/// Migrator running only the framework migrations (tests, tools).
pub struct Migrator;

impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        migrations()
    }
}
