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
mod m20261015_000008_create_locks_table;
mod m20261015_000009_create_auth_generations_table;
mod m20261015_000010_create_jwt_tables;
mod m20261016_000011_add_locale_to_users;
mod m20261017_000012_create_audit_logs_table;
mod m20261018_000013_create_settings_table;
mod m20261019_000014_create_jobs_tables;
mod m20261023_000015_create_exports_table;
mod m20261023_000016_create_imports_table;
mod m20261024_000017_add_import_receipts;
mod m20261025_000018_add_invitations_to_users;
mod m20261025_000019_create_user_tokens_table;
mod m20261101_000020_create_schedule_tables;
mod m20261120_000021_add_run_history;
mod m20261201_000022_add_alt_to_media;
pub mod schema;

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
        Box::new(m20261015_000008_create_locks_table::Migration),
        Box::new(m20261015_000009_create_auth_generations_table::Migration),
        Box::new(m20261015_000010_create_jwt_tables::Migration),
        Box::new(m20261016_000011_add_locale_to_users::Migration),
        Box::new(m20261017_000012_create_audit_logs_table::Migration),
        Box::new(m20261018_000013_create_settings_table::Migration),
        Box::new(m20261019_000014_create_jobs_tables::Migration),
        Box::new(m20261023_000015_create_exports_table::Migration),
        Box::new(m20261023_000016_create_imports_table::Migration),
        Box::new(m20261024_000017_add_import_receipts::Migration),
        Box::new(m20261025_000018_add_invitations_to_users::Migration),
        Box::new(m20261025_000019_create_user_tokens_table::Migration),
        Box::new(m20261101_000020_create_schedule_tables::Migration),
        Box::new(m20261120_000021_add_run_history::Migration),
        Box::new(m20261201_000022_add_alt_to_media::Migration),
    ]
}

/// Migrator running only the framework migrations (tests, tools).
pub struct Migrator;

impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        migrations()
    }
}
