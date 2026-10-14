use sea_orm_migration::prelude::*;

use crate::schema::{ColumnDefExt, create_index, create_table};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[sea_orm_migration::async_trait::async_trait]
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
                .table(Sessions::Table)
                .col(ColumnDef::new(Sessions::Id).string_len(128).not_null().primary_key())
                .col(ColumnDef::new(Sessions::Data).text().not_null())
                .col(ColumnDef::new(Sessions::ExpiresAt).instant(manager).not_null())
                .to_owned(),
        )
        .await?;
        create_index(
            manager,
            Sessions::Table,
            Index::create().name("sessions_expires_at_idx").table(Sessions::Table).col(Sessions::ExpiresAt).to_owned(),
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(Sessions::Table).if_exists().to_owned()).await
    }
}

#[derive(DeriveIden)]
enum Sessions {
    Table,
    Id,
    Data,
    ExpiresAt,
}
