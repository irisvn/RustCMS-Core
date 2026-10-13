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
            .create_table(
                Table::create()
                    .table(IdempotencyKeys::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(IdempotencyKeys::Id).string_len(64).not_null().primary_key())
                    .col(ColumnDef::new(IdempotencyKeys::Owner).string_len(255).not_null())
                    .col(ColumnDef::new(IdempotencyKeys::RequestHash).string_len(64).not_null())
                    .col(ColumnDef::new(IdempotencyKeys::Result).text())
                    .col(ColumnDef::new(IdempotencyKeys::CreatedAt).timestamp_with_time_zone().not_null())
                    .col(ColumnDef::new(IdempotencyKeys::ExpiresAt).timestamp_with_time_zone().not_null())
                    .to_owned(),
            )
            .await?;
        for (name, column) in [
            ("idempotency_keys_owner_idx", IdempotencyKeys::Owner),
            ("idempotency_keys_expires_at_idx", IdempotencyKeys::ExpiresAt),
        ] {
            manager
                .create_index(
                    Index::create().if_not_exists().name(name).table(IdempotencyKeys::Table).col(column).to_owned(),
                )
                .await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(IdempotencyKeys::Table).to_owned()).await
    }
}

#[derive(DeriveIden)]
enum IdempotencyKeys {
    Table,
    Id,
    Owner,
    RequestHash,
    Result,
    CreatedAt,
    ExpiresAt,
}
