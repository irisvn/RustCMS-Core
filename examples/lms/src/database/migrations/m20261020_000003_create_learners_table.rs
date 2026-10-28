//! Learners: people taking courses (no sign-in; not users).

use rustcms::migration::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    /// Schema change and migration record commit together, also on SQLite
    /// (whose default is no transaction), so a failed run can be retried.
    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }

    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            manager,
            Table::create()
                .table(Learners::Table)
                .col(ColumnDef::new(Learners::Id).big_integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(Learners::Name).string_len(120).not_null())
                .col(ColumnDef::new(Learners::Email).string_len(254).not_null())
                .col(ColumnDef::new(Learners::Phone).string_len(32).null())
                .col(ColumnDef::new(Learners::CreatedAt).instant_now(manager).not_null())
                .index(Index::create().name("learners_email_key").col(Learners::Email).unique())
                .to_owned(),
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_table(manager, Learners::Table).await
    }
}

#[derive(DeriveIden)]
enum Learners {
    Table,
    Id,
    Name,
    Email,
    Phone,
    CreatedAt,
}
