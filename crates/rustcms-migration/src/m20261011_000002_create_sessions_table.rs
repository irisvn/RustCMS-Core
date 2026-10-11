use sea_orm_migration::prelude::*;

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
        manager
            .create_table(
                Table::create()
                    .table(Sessions::Table)
                    .col(ColumnDef::new(Sessions::Id).string_len(128).not_null().primary_key())
                    .col(ColumnDef::new(Sessions::Data).text().not_null())
                    .col(ColumnDef::new(Sessions::ExpiresAt).timestamp_with_time_zone().not_null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("sessions_expires_at_idx")
                    .table(Sessions::Table)
                    .col(Sessions::ExpiresAt)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(Sessions::Table).to_owned()).await
    }
}

#[derive(DeriveIden)]
enum Sessions {
    Table,
    Id,
    Data,
    ExpiresAt,
}
