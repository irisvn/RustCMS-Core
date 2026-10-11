//! Groups a session with every session re-issued from it (`lineage` = id of the
//! first one), so signing out ends re-issues created concurrently. `NULL` (rows
//! created before this migration) means the session is its own lineage.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[sea_orm_migration::async_trait::async_trait]
impl MigrationTrait for Migration {
    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }

    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Sessions::Table)
                    .add_column(ColumnDef::new(Sessions::Lineage).string_len(64).null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create().name("sessions_lineage_idx").table(Sessions::Table).col(Sessions::Lineage).to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_index(Index::drop().name("sessions_lineage_idx").table(Sessions::Table).to_owned()).await?;
        manager.alter_table(Table::alter().table(Sessions::Table).drop_column(Sessions::Lineage).to_owned()).await
    }
}

#[derive(DeriveIden)]
enum Sessions {
    Table,
    Lineage,
}
