use sea_orm_migration::prelude::*;

use crate::schema::{ColumnDefExt, create_index, create_table};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[sea_orm_migration::async_trait::async_trait]
impl MigrationTrait for Migration {
    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }

    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            manager,
            Table::create()
                .table(IdempotencyKeys::Table)
                .if_not_exists()
                .col(ColumnDef::new(IdempotencyKeys::Id).string_len(64).not_null().primary_key())
                .col(ColumnDef::new(IdempotencyKeys::Owner).string_len(255).not_null())
                .col(ColumnDef::new(IdempotencyKeys::RequestHash).string_len(64).not_null())
                .col(ColumnDef::new(IdempotencyKeys::Result).text())
                .col(ColumnDef::new(IdempotencyKeys::CreatedAt).instant(manager).not_null())
                .col(ColumnDef::new(IdempotencyKeys::ExpiresAt).instant(manager).not_null())
                .to_owned(),
        )
        .await?;
        for (name, column) in [
            ("idempotency_keys_owner_idx", IdempotencyKeys::Owner),
            ("idempotency_keys_expires_at_idx", IdempotencyKeys::ExpiresAt),
        ] {
            create_index(
                manager,
                IdempotencyKeys::Table,
                Index::create().if_not_exists().name(name).table(IdempotencyKeys::Table).col(column).to_owned(),
            )
            .await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(IdempotencyKeys::Table).if_exists().to_owned()).await
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
