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
mod m20261012_000003_create_personal_access_tokens_table;
mod m20261012_000004_create_rbac_tables;
mod m20261012_000005_add_lineage_to_sessions;
mod m20261013_000006_create_idempotency_keys_table;
mod m20261014_000007_create_media_tables;

/// Framework migrations, in order.
pub fn migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![
        Box::new(m20261011_000001_create_users_table::Migration),
        Box::new(m20261011_000002_create_sessions_table::Migration),
        Box::new(m20261012_000003_create_personal_access_tokens_table::Migration),
        Box::new(m20261012_000004_create_rbac_tables::Migration),
        Box::new(m20261012_000005_add_lineage_to_sessions::Migration),
        Box::new(m20261013_000006_create_idempotency_keys_table::Migration),
        Box::new(m20261014_000007_create_media_tables::Migration),
    ]
}

/// Migrator running only the framework migrations (tests, tools).
pub struct Migrator;

impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        migrations()
    }
}
